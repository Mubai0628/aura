//! Memory has no process/network/credential access. Its roots are process-owned;
//! the file layer checks final paths, identities, links and expected hashes.
use super::*;
use aura_memory::{MemoryScope, MemoryWorkspace};
use serde_json::{Value, json};
pub(super) const BACKEND_ID: &str = "memory-vault-broker-v1";

pub(crate) fn descriptor(runtime: RuntimeInstanceId) -> BackendDescriptor {
    let mut d = super::broker::descriptor(runtime);
    d.id = BackendId(BACKEND_ID.into());
    d.protocol_fingerprint =
        aura_domain_common::jcs::canonical_sha256(&BACKEND_ID).expect("literal");
    d.capabilities = [
        ResolvedCapability::MemoryRead,
        ResolvedCapability::MemoryWrite,
        ResolvedCapability::MemoryDelete,
    ]
    .into();
    // AuthorizedReadsOnly / AuthorizedWritesOnly: constructor-owned Vault roots
    // plus fs-tools' held final-path, reparse/hardlink and hash checks. Internal
    // metadata is confined to the registered Core data directory.
    // CredentialIsolation: this adapter has no credential, network or process
    // API. BoundedOutput: 1 MiB per note, <=100 entries and an output byte cap.
    d.guarantees = [
        EnforcementGuarantee::AuthorizedReadsOnly,
        EnforcementGuarantee::AuthorizedWritesOnly,
        EnforcementGuarantee::CredentialIsolation,
        EnforcementGuarantee::BoundedOutput,
    ]
    .into();
    d
}
fn workspace(
    service: &AgentExecution,
    snapshot: &PermissionSnapshot,
) -> anyhow::Result<MemoryWorkspace> {
    let resolver = service.workspace.resolver(snapshot.workspace_id)?;
    resolver.verify_root_identity()?;
    Ok(MemoryWorkspace {
        workspace_id: snapshot.workspace_id,
        root: resolver.canonical_root().to_path_buf(),
    })
}
pub(super) fn resolve(
    service: &AgentExecution,
    snapshot: &PermissionSnapshot,
    kind: &str,
    input: &Value,
) -> anyhow::Result<(ResourceDescriptor, Value)> {
    let workspace = workspace(service, snapshot)?;
    let memory = service.journal.memory();
    let scope = memory.tool_scope(kind, input, &workspace)?;
    let vault = memory.vault(scope, &workspace)?;
    let resource = ResourceDescriptor::MemoryVault {
        scope: match scope {
            MemoryScope::Global => MemoryVaultScope::Global,
            MemoryScope::Project => {
                MemoryVaultScope::Project { workspace_id: snapshot.workspace_id }
            }
        },
        vault_identity: ContentHash::new(ContentHashAlgorithm::Blake3, vault.identity())?,
    };
    let titles = titles(&vault, kind, input);
    Ok((resource, json!({"scope":scope,"input":input,"titles":titles})))
}
fn titles(vault: &aura_memory::Vault, kind: &str, input: &Value) -> Vec<String> {
    if kind == "memory.create" {
        return input["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|i| i["body"].as_str())
            .map(|s| {
                s.lines()
                    .find_map(|l| l.trim().strip_prefix("# "))
                    .unwrap_or("未命名记忆")
                    .chars()
                    .take(200)
                    .collect()
            })
            .filter(|title: &String| {
                aura_network_control::SecretScanner.scan_text("memory.title", title).is_ok()
            })
            .collect();
    }
    let mut targets = vec![];
    if kind == "memory.read" {
        targets.push(input["target"].clone());
    }
    for item in input["items"].as_array().into_iter().flatten() {
        targets.push(item["target"].clone());
    }
    for item in input["targets"].as_array().into_iter().flatten() {
        targets.push(item.clone());
    }
    let mut result = vec![];
    for target in targets {
        let read = if let Some(id) = target["id"].as_str().and_then(|id| id.parse().ok()) {
            vault.read_id(id)
        } else if let Some(path) =
            target["path"].as_str().and_then(|p| WorkspacePath::parse(p).ok())
        {
            vault.read_path(&path)
        } else {
            continue;
        };
        if let Ok(read) = read {
            let title = read.note.title();
            if aura_network_control::SecretScanner.scan_text("memory.title", &title).is_ok() {
                result.push(title);
            }
        }
    }
    result
}
pub(super) fn inspect(service: &AgentExecution, call: &CallContext) -> anyhow::Result<Value> {
    let (resource, _) = resolve(
        service,
        &call.snapshot,
        &call.operation.content.action_kind,
        &call.operation.content.action["input"],
    )?;
    anyhow::ensure!(call.operation.content.resources == [resource.clone()], "memory.vault_changed");
    Ok(json!({"schema":"memory-vault-facts-v1","vault":resource}))
}
pub(super) fn backend(
    service: &Arc<AgentExecution>,
    call: &CallContext,
) -> anyhow::Result<Arc<dyn aura_backend_api::ExecutionBackend>> {
    Ok(Arc::new(
        aura_backend_core::CoreBroker::new(
            descriptor(service.runtime),
            super::broker::Port { service: service.clone(), call: call.clone(), commands: None },
        )
        .map_err(|e| anyhow::anyhow!("{e:?}"))?,
    ))
}
pub(super) async fn invoke(
    service: Arc<AgentExecution>,
    call: CallContext,
    request: DispatchRequest,
    start_lease: tokio::sync::OwnedRwLockReadGuard<()>,
) -> anyhow::Result<()> {
    let lifetime = service.active_calls.retain_worker(call.validated.call.tool_call_id)?;
    let active = service
        .active_calls
        .session(call.snapshot.session_id)?
        .into_iter()
        .find(|a| a.call.validated.call.tool_call_id == call.validated.call.tool_call_id)
        .ok_or_else(|| anyhow::anyhow!("memory.call_not_active"))?;
    tokio::task::spawn_blocking(move || {
        let _lifetime = lifetime;
        let _lease = start_lease;
        let write = call.operation.content.capabilities != [ResolvedCapability::MemoryRead];
        let mut entered_write = false;
        let result = service.persistence.with_session_permission_authority(
            call.snapshot.session_id,
            |mode| -> anyhow::Result<Value> {
                anyhow::ensure!(!*active.cancel.borrow(), "memory.cancelled_before_start");
                anyhow::ensure!(
                    service.operation_permitted_now(&call.operation, &call.snapshot, mode.mode, mode.revision)?,
                    "memory.permission_withdrawn"
                );
                service.verify_source(&call)?;
                aura_persistence_v2::agent::checkpoint::verify_start_operation(
                    service.journal.lock()?.database.connection(), request.operation.operation_id,
                )?;
                anyhow::ensure!(inspect(&service, &call)? == request.plan.prepared.preparation["domain"], "memory.vault_changed");
                let actor = aura_memory::WriteActor {
                    kind: "model_tool".into(), session_id: Some(call.snapshot.session_id),
                    turn_id: call.operation.content.identity.turn_task_id.map(|id| id.to_string()),
                };
                entered_write = write;
                let value = service.journal.memory().invoke(
                    &call.operation.content.action_kind, &call.operation.content.action["input"],
                    &workspace(&service, &call.snapshot)?, &actor,
                )?;
                anyhow::ensure!(serde_json::to_vec(&value)?.len() <= 2 * 1024 * 1024, "memory.output_limit: reduce the search limit or read individual notes");
                Ok(value)
            },
        ).map_err(anyhow::Error::from).and_then(|r| r);
        let uncertain = entered_write && result.as_ref().err().is_some_and(|error| {
            !matches!(error.downcast_ref::<aura_memory::MemoryError>(), Some(
                aura_memory::MemoryError::Format(_) | aura_memory::MemoryError::Secret { .. }
                | aura_memory::MemoryError::Conflict { .. } | aura_memory::MemoryError::Invalid(_)
                | aura_memory::MemoryError::NotFound | aura_memory::MemoryError::RolledBack { .. }
            ))
        });
        let side_effect = if uncertain { SideEffectStateV1::Unknown }
            else if write && result.is_ok() { SideEffectStateV1::FullyApplied }
            else { SideEffectStateV1::NoEffect };
        let evidence = BrokerEvidence {
            action_kind: request.operation.content.action_kind.clone(),
            request_fingerprint: request.operation.fingerprint.0.clone(),
            before: request.plan.prepared.preparation["domain"].clone(),
            after: result.as_ref().ok().map(aura_domain_common::jcs::canonical_sha256)
                .transpose()?.map_or(Value::Null, |hash| json!({"result":hash})),
            // `error` is what the model reads; `detail` keeps the full cause for diagnosis.
            domain_journal: json!({"schema":"agent-memory-vault-v1","output":result.as_ref().ok(),"error":result.as_ref().err().map(model_message),"detail":result.as_ref().err().map(ToString::to_string),"operation_rejection":result.as_ref().err().filter(|_| side_effect == SideEffectStateV1::NoEffect).and_then(rejection)}),
            side_effect,
        };
        let verdict = if uncertain { crate::broker::CompletionVerdict::Unknown }
            else if result.is_ok() { crate::broker::CompletionVerdict::Succeeded }
            else { crate::broker::CompletionVerdict::Failed };
        service.journal.complete(&request, evidence, verdict)
    }).await?
}

/// What the model reads when a memory call fails: the rule the call broke, so it can correct
/// the call, and never a local path or an I/O detail.
fn model_message(error: &anyhow::Error) -> String {
    use aura_memory::MemoryError as E;
    use aura_memory_domain::MemoryFormatError as F;
    match error.downcast_ref::<E>() {
        Some(E::Format(F::Invalid(field))) => {
            format!("memory.invalid_input: {}; nothing was written", field_rule(field))
        }
        Some(E::Format(F::TooLarge)) => {
            "memory.invalid_input: a note may be at most 1 MiB; nothing was written".into()
        }
        Some(E::Format(F::Frontmatter(line))) => {
            format!("memory.invalid_input: the frontmatter is invalid at line {line}")
        }
        Some(E::Invalid(reason)) => format!("memory.invalid_input: {reason}; nothing was written"),
        Some(E::Secret { kind }) => format!(
            "memory.secret_refused: the note contains a secret-like value ({kind}); nothing was written"
        ),
        Some(E::Conflict { current_hash }) => format!(
            "memory.hash_conflict: expected_hash is stale; the current hash is {current_hash}"
        ),
        Some(E::NotFound) => "memory.not_found: no such memory in this Vault".into(),
        Some(E::RolledBack { .. }) => {
            "memory.rolled_back: the write failed and every changed file was restored".into()
        }
        _ => {
            let text = error.to_string();
            let own = text.starts_with("memory.") && !text.contains(['\\', '/']);
            if own { text } else { "memory.failed: the memory call did not complete".into() }
        }
    }
}

/// A call a memory rule refused, which wrote nothing, as an ordinary refusal of its own kind: a
/// stale `expected_hash` is a conflict to read again, not a tool that is unavailable.
fn rejection(error: &anyhow::Error) -> Option<Value> {
    use aura_domain_common::ErrorCategory as Kind;
    use aura_memory::MemoryError as E;
    let (code, kind) = match error.downcast_ref::<E>()? {
        E::Conflict { .. } => ("memory.hash_conflict", Kind::Conflict),
        E::NotFound => ("memory.not_found", Kind::NotFound),
        E::Format(_) | E::Invalid(_) => ("memory.invalid_input", Kind::Validation),
        E::Secret { .. } => ("memory.secret_refused", Kind::Validation),
        _ => return None,
    };
    Some(
        json!({"code": code, "category": kind, "message": model_message(error), "retryable": false}),
    )
}

fn field_rule(field: &str) -> &str {
    match field {
        "importance" => "importance must be an integer from 1 to 5 (3 is ordinary, 5 is critical)",
        "recall weights" => {
            "confidence must be between 0 and 1, and recall_weight must not be negative"
        }
        "category" => {
            "category must be 1 to 64 lowercase ASCII letters, digits or '-' (for example rules)"
        }
        "self relation" => "a note cannot be related to, supersede or conflict with itself",
        other => other,
    }
}
