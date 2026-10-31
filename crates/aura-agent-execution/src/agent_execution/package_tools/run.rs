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
                "package.permission_withdrawn: 权限已变化，调用没有开始"
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
                    .ok_or_else(|| anyhow::anyhow!("agent.plan_call_missing"))?;
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
            return finish(service, request, writes, Observed::refused(error.to_string()));
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

/// Only fixed `package.*` / `agent.*` reasons reach the model; anything else is generic.
fn model_safe(error: &anyhow::Error) -> String {
    error
        .chain()
        .map(ToString::to_string)
        .find(|text| text.starts_with("package.") || text.starts_with("agent."))
        .unwrap_or_else(|| "package.preparation_failed: 程序没有启动".to_owned())
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
            "package.directory_required: 参数指定的工作目录需要是工作区内已有的目录"
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
    let output = if reason == Reason::Completed {
        match &call.validated.definition.result_contract {
            ToolResultContractV1::Unit => Value::Null,
            ToolResultContractV1::Utf8Text => {
                Value::String(String::from_utf8(result.body.clone())?)
            }
            ToolResultContractV1::StructuredJson { .. } => {
                aura_domain_common::jcs::parse_json_strict(&result.body)?
            }
        }
    } else {
        Value::Null
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
        output,
        error: if reason == Reason::Completed {
            Ok(())
        } else {
            Err(super::program::failure_text(reason))
        },
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

/// One terminal per attempt. A started program whose stop is not confirmed is
/// Unknown and never replayed; a started program that declares persistent effects
/// leaves its side effects unverified.
fn finish(
    service: &Arc<AgentExecution>,
    request: &DispatchRequest,
    writes: bool,
    observed: Observed,
) -> anyhow::Result<()> {
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
