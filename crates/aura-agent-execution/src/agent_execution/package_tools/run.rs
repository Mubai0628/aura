//! One package program call, on a blocking worker: re-verify the authority and the
//! install, hold the content, run it in the call's Job, record the terminal once.
use super::super::*;
use super::installed_tool_for;
use aura_core_base::append_core_log;
use aura_domain_common::jcs::canonical_sha256;
use aura_package_service::package_program::{ProgramContext, ProgramInvocationIdentity, run_case};
use aura_package_service::{package_lifecycle, package_program};
use aura_persistence_v2::packages;
use aura_tool_contract::ToolResultContractV1;
use aura_tool_domain::PackageConformanceReasonV1 as Reason;
use serde_json::{Value, json};
use std::sync::atomic::AtomicBool;

pub(super) struct Inputs {
    pub start_lease: tokio::sync::OwnedRwLockReadGuard<()>,
    pub powershell: Option<Arc<aura_backend_windows_user::compat::CompatBackend>>,
    pub environment: Vec<(std::ffi::OsString, std::ffi::OsString)>,
}

struct Started {
    tool: super::InstalledPackageTool,
    content: packages::LoadedPackageContent,
    timeout_ms: u64,
}

struct Observed {
    output: Value,
    error: Result<(), String>,
    started: bool,
    stop_confirmed: bool,
    evidence: Value,
}

pub(super) fn run(
    service: &Arc<AgentExecution>,
    call: &CallContext,
    request: &DispatchRequest,
    cancelled: &AtomicBool,
    inputs: Inputs,
) -> anyhow::Result<()> {
    let operation = request.operation.operation_id.to_string();
    let writes = call.validated.definition.execution_semantics.may_create_persistent_effects;
    let Inputs { start_lease, powershell, environment } = inputs;
    // The durable start: authority, frozen source, the bound operation and the live
    // install are all checked again; only then is the content retained for this call.
    let checked = service.persistence.with_session_permission_authority(
        call.snapshot.session_id,
        |mode| -> anyhow::Result<Started> {
            anyhow::ensure!(
                service.operation_permitted_now(
                    &call.operation,
                    &call.snapshot,
                    mode.mode,
                    mode.revision
                )?,
                "package.permission_withdrawn: 这次会话的权限在调用开始前变了，程序没有运行。可以重新调用，会按新的权限检查。"
            );
            service.verify_source(call)?;
            let mut store = service.journal.lock()?;
            let db = store.database.connection();
            aura_persistence_v2::agent::checkpoint::verify_start_operation(
                db,
                request.operation.operation_id,
            )?;
            let tool = installed_tool_for(db, &call.validated.definition)?;
            let content = packages::load_content(db, &tool.version)?;
            let timeout_ms =
                aura_persistence_v2::tool::execution::batch(db, call.batch.tool_batch_id)?
                    .plan
                    .calls
                    .iter()
                    .find(|plan| plan.tool_call_id == call.validated.call.tool_call_id)
                    .map(|plan| plan.timeout_ms)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "agent.plan_call_missing: Aura could not find this call in its turn's \
                             plan, so it did not start the program. Call it again."
                        )
                    })?;
            let fingerprint = tool.version.package_fingerprint.0.hex().to_owned();
            store.database.unit_of_work(|tx| {
                packages::references::retain_for_call(tx, &fingerprint, &operation)
            })?;
            Ok(Started { tool, content, timeout_ms })
        },
    )?;
    let started = match checked {
        Ok(value) => value,
        Err(error) => {
            drop(start_lease);
            // Nothing started: a refused start is a plain failure with no effect.
            return finish(service, request, writes, Observed::refused(model_safe(&error)));
        }
    };
    drop(start_lease);
    let fingerprint = started.tool.version.package_fingerprint.0.hex().to_owned();
    let observed = execute(service, call, request, &started, cancelled, powershell, environment)
        .unwrap_or_else(|error| Observed {
            // A failure before the Job existed (workspace, claims, staging) started nothing.
            output: Value::Null,
            error: Err(model_safe(&error)),
            started: false,
            stop_confirmed: true,
            evidence: Value::Null,
        });
    let released = service.journal.lock().map_err(anyhow::Error::from).and_then(|mut store| {
        store.database.unit_of_work(|tx| {
            packages::references::release_for_call(tx, &fingerprint, &operation)?;
            packages::lifecycle::collect_unreferenced(tx, UtcTimestamp::now())
        })?;
        Ok(())
    });
    if let Err(error) = released {
        append_core_log(
            "package.call_reference_release_failed",
            json!({"reason": error.to_string()}),
        );
    }
    finish(service, request, writes, observed)
}

impl Observed {
    fn refused(message: String) -> Self {
        Self {
            output: Value::Null,
            error: Err(message),
            started: false,
            stop_confirmed: true,
            evidence: Value::Null,
        }
    }
}

/// Why the program did not run, as the model reads it: Core's line for a code the runner gave,
/// a package line Core wrote, the shared sentence for a known refusal under its own code, an
/// `agent.*` line Core wrote, or one line with the first words of the cause, bounded.
fn model_safe(error: &anyhow::Error) -> String {
    use aura_persistence_v2::tool::refusal_text::{code_in, known, readable_line};
    let chain: Vec<String> = error.chain().map(ToString::to_string).collect();
    let line = |family: &str| {
        chain.iter().find_map(|text| {
            readable_line(text).filter(|line| line.starts_with(family)).map(str::to_owned)
        })
    };
    super::program::start_failure_text(&chain)
        .map(str::to_owned)
        .or_else(|| line("package."))
        .or_else(|| {
            chain.iter().find_map(|text| {
                let sentence = known(text)?;
                Some(format!("{}: {sentence}", code_in(text).unwrap_or("package.preparation_failed")))
            })
        })
        .or_else(|| line("agent."))
        .unwrap_or_else(|| {
            let cause = chain
                .iter()
                .find(|text| text.as_str() != "package.preparation_failed")
                .map_or("", String::as_str);
            format!(
                "package.preparation_failed: 程序启动前的准备没有完成（{}），程序没有运行。可以重新调用；反复出现时告诉用户。",
                bounded(cause)
            )
        })
}

/// Another component's words inside Core's sentence: one line of at most 160 bytes, without
/// control characters or braces.
fn bounded(text: &str) -> String {
    let mut bounded = String::new();
    for c in text.lines().next().unwrap_or_default().trim().chars() {
        if bounded.len() + c.len_utf8() > 160 {
            bounded.push('…');
            break;
        }
        bounded.push(if c.is_control() || c == '{' || c == '}' { ' ' } else { c });
    }
    bounded
}

fn execute(
    service: &Arc<AgentExecution>,
    call: &CallContext,
    request: &DispatchRequest,
    started: &Started,
    cancelled: &AtomicBool,
    powershell: Option<Arc<aura_backend_windows_user::compat::CompatBackend>>,
    environment: Vec<(std::ffi::OsString, std::ffi::OsString)>,
) -> anyhow::Result<Observed> {
    let tool = &started.tool;
    let arguments = &call.operation.content.action;
    let resolver = service.workspace.resolver(call.snapshot.workspace_id)?;
    resolver.verify_root_identity()?;
    let workspace = resolver.resolve_existing(&WorkspacePath::parse(WorkspacePath::ROOT)?)?;
    let mut claims = std::collections::BTreeMap::new();
    if let Some((key, relative)) =
        package_program::claims::working_directory_claim(&tool.tool, arguments)?
    {
        let resolved = resolver.resolve_existing(&relative)?;
        anyhow::ensure!(
            resolved.info.is_directory,
            "package.directory_required: 参数指定的工作目录需要是工作区里已有的文件夹，程序没有运行。请改成工作区里已有的文件夹再调用。"
        );
        claims.insert(key, resolved.absolute_path);
    }
    let root = super::scratch_root(service)?;
    std::fs::create_dir_all(&root)?;
    let scratch =
        tempfile::Builder::new().prefix("package-run-").rand_bytes(32).tempdir_in(&root)?;
    let staged = package_lifecycle::materialize::staged(&started.content, scratch.path())?;
    let context = ProgramContext {
        identity: ProgramInvocationIdentity {
            tool_call_id: call.validated.call.tool_call_id,
            tool_execution_attempt_id: super::program::attempt_identity_for(request),
        },
        workspace_root: Some(&workspace.absolute_path),
        directory_claims: claims,
        instance_configuration: tool
            .settings
            .as_ref()
            .and_then(|entry| entry.configuration.as_ref()),
        powershell: powershell.as_deref(),
        user_environment: environment,
        scratch_parent: scratch.path(),
        timeout: std::time::Duration::from_millis(started.timeout_ms),
        max_output_bytes: super::program::OUTPUT_BYTES,
    };
    let result =
        run_case(&staged, &tool.tool.tool_key, arguments, &context, cancelled, &mut |_| {});
    drop(staged);
    let cleaned = scratch.close().is_ok();
    let result = result?;
    let reason = if cleaned { result.reason } else { Reason::CleanupUnconfirmed };
    // From here on the program has run: an output that cannot be read is its failure, recorded
    // as a run, never as a program that did not start.
    let read = if reason == Reason::Completed {
        read_output(
            &call.validated.definition.result_contract,
            &result.body,
            call.validated.definition.execution_semantics.may_create_persistent_effects,
        )
    } else {
        Err(super::program::failure_text(reason, started.timeout_ms))
    };
    let artifacts = result
        .artifacts
        .iter()
        .map(|artifact| {
            json!({
                "slot": artifact.declaration.slot,
                "media_type": artifact.declaration.media_type,
                "byte_length": artifact.declaration.byte_length,
                "content_hash": artifact.declaration.content_hash,
            })
        })
        .collect::<Vec<_>>();
    Ok(Observed {
        output: read.as_ref().map_or(Value::Null, Clone::clone),
        error: read.map(drop),
        started: result.started,
        stop_confirmed: result.stop_confirmed && cleaned,
        evidence: json!({
            "runtime_evidence_hash": result.runtime_evidence_hash,
            "reason": reason,
            "artifacts": artifacts,
            "package_fingerprint": tool.version.package_fingerprint,
        }),
    })
}

/// A completed program's output as its result contract reads it; one that cannot be read says
/// the program ran, and what that may have changed.
fn read_output(
    contract: &ToolResultContractV1,
    body: &[u8],
    writes: bool,
) -> Result<Value, String> {
    let read = match contract {
        ToolResultContractV1::Unit => Ok(Value::Null),
        ToolResultContractV1::Utf8Text => {
            String::from_utf8(body.to_vec()).map(Value::String).map_err(anyhow::Error::from)
        }
        ToolResultContractV1::StructuredJson { .. } => {
            aura_domain_common::jcs::parse_json_strict(body).map_err(anyhow::Error::from)
        }
    };
    read.map_err(|cause| {
        let effect = if writes {
            "它声明会留下改动，可能已经改了一部分；先核对结果，再决定要不要重做。"
        } else {
            "它不会留下改动；可以重新调用，反复出现时告诉用户。"
        };
        format!(
            "package.output_unreadable: 程序运行完了，但它的输出读不懂（{}）。{effect}",
            bounded(&cause.to_string())
        )
    })
}

/// What a call's record says it changed, and how it ended. A started program whose stop is not
/// confirmed is Unknown and never replayed; a started program that declares persistent effects
/// leaves its side effects unverified, whether it succeeded or failed after it ran.
fn settled(
    observed: &Observed,
    writes: bool,
) -> (SideEffectStateV1, crate::broker::CompletionVerdict) {
    let side_effect = if !observed.started {
        SideEffectStateV1::NoEffect
    } else if !observed.stop_confirmed || writes {
        SideEffectStateV1::Unknown
    } else {
        SideEffectStateV1::NoEffect
    };
    let verdict = if observed.started && !observed.stop_confirmed {
        crate::broker::CompletionVerdict::Unknown
    } else if observed.error.is_ok() {
        crate::broker::CompletionVerdict::Succeeded
    } else {
        crate::broker::CompletionVerdict::Failed
    };
    (side_effect, verdict)
}

/// One terminal per attempt, as [`settled`] reads it.
fn finish(
    service: &Arc<AgentExecution>,
    request: &DispatchRequest,
    writes: bool,
    observed: Observed,
) -> anyhow::Result<()> {
    let (side_effect, verdict) = settled(&observed, writes);
    // A call that started keeps running through a lifecycle change; its result says so.
    let changed = observed.started
        && service.journal.lock().is_ok_and(|store| {
            super::changed_during_call(store.database.connection(), &request.operation.content)
        });
    let evidence = BrokerEvidence {
        action_kind: request.operation.content.action_kind.clone(),
        request_fingerprint: request.operation.fingerprint.0.clone(),
        before: request.plan.prepared.preparation["domain"].clone(),
        after: if observed.error.is_ok() {
            json!({"result": canonical_sha256(&observed.output)?})
        } else {
            Value::Null
        },
        domain_journal: json!({
            "schema": "agent-package-tool-v1",
            "output": if observed.error.is_ok() { observed.output.clone() } else { Value::Null },
            "error": observed.error.as_ref().err(),
            "execution": observed.evidence,
            "started": observed.started,
        }),
        side_effect,
    };
    let notes: &[&str] = if changed { &[super::CHANGED_DURING_CALL] } else { &[] };
    service.journal.complete_noting(request, evidence, verdict, notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The runner's bare code, a known refusal and an unknown cause each become one line that
    /// says whether the program ran.
    #[test]
    fn a_program_that_did_not_run_says_why() {
        let runner =
            anyhow::anyhow!("package.arguments_invalid").context("package.preparation_failed");
        assert!(model_safe(&runner).starts_with("package.arguments_invalid: "));
        let started = anyhow::anyhow!("os error 5").context("package.execution_report_unconfirmed");
        assert!(model_safe(&started).contains("程序已经启动"));
        let known = model_safe(&anyhow::anyhow!("agent.registry_changed"));
        assert!(known.starts_with("agent.registry_changed: This call was not started"), "{known}");
        let raw = anyhow::Error::msg("path not found: src/{x}\nmore")
            .context("package.preparation_failed");
        let line = model_safe(&raw);
        assert!(
            line.starts_with(
                "package.preparation_failed: 程序启动前的准备没有完成（path not found"
            )
        );
        assert!(!line.contains(['\n', '{', '}']) && !line.contains("more"), "{line}");
    }

    /// A program that ran and gave an output its contract cannot read failed after it ran: the
    /// record keeps that it ran, and one that declares changes leaves them unverified.
    #[test]
    fn an_unreadable_output_is_a_failure_after_the_program_ran() {
        let contract = ToolResultContractV1::Utf8Text;
        let writing = read_output(&contract, &[0xff, 0xfe], true).expect_err("unreadable");
        assert!(writing.starts_with("package.output_unreadable: 程序运行完了"), "{writing}");
        assert!(writing.contains("先核对结果"), "{writing}");
        let reading = read_output(&contract, &[0xff, 0xfe], false).expect_err("unreadable");
        assert!(reading.contains("不会留下改动"), "{reading}");
        assert_eq!(read_output(&contract, b"done", true), Ok(json!("done")));
        let ran = |started: bool| Observed {
            output: Value::Null,
            error: Err(writing.clone()),
            started,
            stop_confirmed: true,
            evidence: Value::Null,
        };
        let failed = crate::broker::CompletionVerdict::Failed;
        assert_eq!(settled(&ran(true), true), (SideEffectStateV1::Unknown, failed));
        assert_eq!(settled(&ran(true), false), (SideEffectStateV1::NoEffect, failed));
        assert_eq!(settled(&ran(false), true), (SideEffectStateV1::NoEffect, failed));
    }
}
