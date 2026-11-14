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
/// The call's Vault and its normalized action. A refusal reads as the model's message for it,
/// with the cause kept underneath for callers that classify it.
pub(super) fn resolve(
    service: &AgentExecution,
    snapshot: &PermissionSnapshot,
    kind: &str,
    input: &Value,
) -> anyhow::Result<(ResourceDescriptor, Value)> {
    resolved(service, snapshot, kind, input).map_err(|error| {
        let message = model_message(&error, kind, input, false);
        error.context(message)
    })
}
fn resolved(
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
    anyhow::ensure!(call.operation.content.resources == [resource.clone()], VAULT_CHANGED);
    Ok(json!({"schema":"memory-vault-facts-v1","vault":resource}))
}
const VAULT_CHANGED: &str = "memory.vault_changed: the memory Vault changed after this call was prepared; nothing was read or written; call again";
pub(super) fn backend(
    service: &Arc<AgentExecution>,
    call: &CallContext,
) -> anyhow::Result<Arc<dyn aura_backend_api::ExecutionBackend>> {
    Ok(Arc::new(
        aura_backend_core::CoreBroker::new(
            descriptor(service.runtime),
            super::broker::Port { service: service.clone(), call: call.clone(), commands: None },
        )
        .map_err(super::dispatch::backend_failure)?,
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
        .ok_or_else(|| {
            anyhow::anyhow!(
                "memory.call_not_active: this memory call ended before it started (the turn stopped or the call was cancelled); nothing was read or written"
            )
        })?;
    tokio::task::spawn_blocking(move || {
        let _lifetime = lifetime;
        let _lease = start_lease;
        let write = call.operation.content.capabilities != [ResolvedCapability::MemoryRead];
        let mut entered_write = false;
        let result = service.persistence.with_session_permission_authority(
            call.snapshot.session_id,
            |mode| -> anyhow::Result<Value> {
                anyhow::ensure!(!*active.cancel.borrow(), "memory.cancelled_before_start: the call was cancelled before it started; nothing was read or written");
                anyhow::ensure!(
                    service.operation_permitted_now(&call.operation, &call.snapshot, mode.mode, mode.revision)?,
                    "memory.permission_withdrawn: the session's permission mode no longer allows this memory call; nothing was read or written; ask the user to change the mode if it is still needed"
                );
                service.verify_source(&call)?;
                aura_persistence_v2::agent::checkpoint::verify_start_operation(
                    service.journal.lock()?.database.connection(), request.operation.operation_id,
                )?;
                anyhow::ensure!(inspect(&service, &call)? == request.plan.prepared.preparation["domain"], VAULT_CHANGED);
                let actor = aura_memory::WriteActor {
                    kind: "model_tool".into(), session_id: Some(call.snapshot.session_id),
                    turn_id: call.operation.content.identity.turn_task_id.map(|id| id.to_string()),
                };
                entered_write = write;
                let value = service.journal.memory().invoke(
                    &call.operation.content.action_kind, &call.operation.content.action["input"],
                    &workspace(&service, &call.snapshot)?, &actor,
                )?;
                anyhow::ensure!(serde_json::to_vec(&value)?.len() <= 2 * 1024 * 1024, "memory.output_limit: the result is larger than 2 MiB, so it was not returned; reduce the search limit or read individual notes");
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
        let (kind, input) = (call.operation.content.action_kind.as_str(), &call.operation.content.action["input"]);
        let evidence = BrokerEvidence {
            action_kind: request.operation.content.action_kind.clone(),
            request_fingerprint: request.operation.fingerprint.0.clone(),
            before: request.plan.prepared.preparation["domain"].clone(),
            after: result.as_ref().ok().map(aura_domain_common::jcs::canonical_sha256)
                .transpose()?.map_or(Value::Null, |hash| json!({"result":hash})),
            // `error` is what the model reads; `detail` keeps the full cause chain for diagnosis.
            domain_journal: json!({"schema":"agent-memory-vault-v1","output":result.as_ref().ok(),"error":result.as_ref().err().map(|error| model_message(error, kind, input, uncertain)),"detail":result.as_ref().err().map(|error| format!("{error:#}")),"operation_rejection":result.as_ref().err().filter(|_| side_effect == SideEffectStateV1::NoEffect).and_then(|error| rejection(error, kind, input))}),
            side_effect,
        };
        let verdict = if uncertain { crate::broker::CompletionVerdict::Unknown }
            else if result.is_ok() { crate::broker::CompletionVerdict::Succeeded }
            else { crate::broker::CompletionVerdict::Failed };
        service.journal.complete(&request, evidence, verdict)
    }).await?
}

/// What the model reads when a memory call fails: the rule the call broke, so it can correct
/// the call, whether anything was written, and never a local path or an I/O detail. `tool` and
/// `input` are the call's tool and arguments; `uncertain` says a write may have been saved.
fn model_message(error: &anyhow::Error, tool: &str, input: &Value, uncertain: bool) -> String {
    use aura_memory::MemoryError as E;
    use aura_memory_domain::MemoryFormatError as F;
    match error.downcast_ref::<E>() {
        Some(E::RecoveryRequired) => "memory.failed: memory writes are stopped: a write could not be confirmed as fully undone, and Aura finishes undoing it the next time it starts; reads still work; ask the user to restart Aura before writing memory again".into(),
        _ if uncertain => "memory.failed: Aura could not confirm whether this memory write was saved; read the notes with memory.read before writing them again".into(),
        Some(E::Format(F::Invalid(field))) => {
            format!("memory.invalid_input: {}; nothing was written", field_rule(field))
        }
        Some(E::Format(F::TooLarge)) => "memory.invalid_input: a note may be at most 1 MiB; nothing was written; split it into smaller notes".into(),
        Some(E::Format(F::Frontmatter(line))) => format!(
            "memory.invalid_input: a stored note's frontmatter is invalid at line {line}, so Aura cannot read it; memory.validate lists the notes that need repair; nothing was written"
        ),
        Some(E::Invalid("memory tool input")) => {
            let problem = input_problem(tool, input).map_or_else(
                || "check them against the tool description".to_owned(),
                |problem| format!("what did not fit: {problem}"),
            );
            format!(
                "memory.invalid_input: the arguments for {tool} do not fit its input ({problem}); nothing was written"
            )
        }
        Some(E::Invalid(reason)) => {
            format!("memory.invalid_input: {}; nothing was written", reason_rule(reason))
        }
        Some(E::Secret { kind }) => format!(
            "memory.secret_refused: the note contains a secret-like value ({kind}); nothing was written; remove it and call again"
        ),
        Some(E::Conflict { current_hash }) => format!(
            "memory.hash_conflict: expected_hash is stale; the current hash is {current_hash}; nothing was written; read the note again with memory.read and apply the change to what it holds now"
        ),
        Some(E::NotFound) => "memory.not_found: no such memory in this Vault; nothing was written; find the note with memory.search or memory.list_recent".into(),
        Some(E::RolledBack { .. }) => "memory.rolled_back: the write failed and every changed file was restored, so nothing of it remains; call again".into(),
        _ => {
            let text = error.to_string();
            let own = text.starts_with("memory.") && !text.contains(['\\', '/']);
            if own {
                text
            } else {
                "memory.failed: the memory call did not complete and changed nothing; call again, and tell the user if it fails the same way".into()
            }
        }
    }
}

/// A call a memory rule refused, which wrote nothing, as an ordinary refusal of its own kind: a
/// stale `expected_hash` is a conflict to read again, not a tool that is unavailable.
fn rejection(error: &anyhow::Error, tool: &str, input: &Value) -> Option<Value> {
    use aura_domain_common::ErrorCategory as Kind;
    use aura_memory::MemoryError as E;
    let (code, kind) = match error.downcast_ref::<E>()? {
        E::Conflict { .. } => ("memory.hash_conflict", Kind::Conflict),
        E::NotFound => ("memory.not_found", Kind::NotFound),
        E::Format(_) | E::Invalid(_) => ("memory.invalid_input", Kind::Validation),
        E::Secret { .. } => ("memory.secret_refused", Kind::Validation),
        _ => return None,
    };
    let message = model_message(error, tool, input, false);
    Some(json!({"code": code, "category": kind, "message": message, "retryable": false}))
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
        "schema_version" => {
            "a stored note's frontmatter has a schema_version other than 1, so Aura cannot read it; memory.validate lists the notes that need repair"
        }
        "schema" => {
            "a stored note's frontmatter lacks a field or has an unknown or mistyped one, so Aura cannot read it; memory.validate lists the notes that need repair"
        }
        "unterminated frontmatter" => {
            "a stored note's frontmatter must end with a line '---', so Aura cannot read this note; memory.validate lists the notes that need repair"
        }
        "scope" => {
            "a stored note's scope field does not match the Vault it is in, which memory.repair does not change; the note needs a fix by hand"
        }
        "existing field requires manual repair" => {
            "a stored note has a field whose value memory.repair cannot fix; the note needs a fix by hand"
        }
        other => other,
    }
}

/// The rule behind a reason the memory service refused a call with.
fn reason_rule(reason: &str) -> &str {
    match reason {
        "memory scope" => "scope must be \"global\" or \"project\"",
        "Vault scope" => {
            "a stored note's own scope does not match the Vault it is in; memory.validate lists the notes that need repair"
        }
        "duplicate delete" => {
            "the same note is listed more than once in one delete; list each note once"
        }
        "duplicate batch entry" | "duplicate batch path" => {
            "two items in one call name the same note or path; give each note once, at its own path"
        }
        "batch size must be 1..100" | "delete batch size must be 1..100" => {
            "one call takes 1 to 100 items"
        }
        "batch including superseded notes exceeds 100" => {
            "the items and the notes they supersede must be 100 at most in one call"
        }
        "Vault path or scope" => {
            "path must name a Markdown file ending in .md inside the Vault, and not README.md"
        }
        "apply requires preview_token" => {
            "phase apply needs the preview_token that a memory.repair preview returned"
        }
        "unknown memory tool" => "this version of Aura has no such memory tool",
        "expected_hash must be a lowercase BLAKE3 hex digest" => {
            "expected_hash must be the lowercase hash that memory.read returned for the note"
        }
        "relation missing or in another Vault" => {
            "every related, superseded or conflicting id must name a note in this Vault"
        }
        "cross-Vault or absolute wiki-link" => {
            "a wiki-link in the body must name a note by a relative path inside this Vault"
        }
        other => other,
    }
}

/// What did not fit in a memory tool's arguments, found by decoding them again into that tool's
/// input type, as one bounded line.
fn input_problem(tool: &str, input: &Value) -> Option<String> {
    use aura_memory_domain::*;
    fn misfit<T: serde::de::DeserializeOwned>(input: &Value) -> Option<String> {
        serde_json::from_value::<T>(input.clone()).err().map(|error| error.to_string())
    }
    let problem = match tool {
        "memory.search" => misfit::<MemorySearchInput>(input),
        "memory.read" => misfit::<MemoryReadInput>(input),
        "memory.list_recent" => misfit::<MemoryRecentInput>(input),
        "memory.create" => misfit::<MemoryCreateInput>(input),
        "memory.update" => misfit::<MemoryUpdateInput>(input),
        "memory.link" => misfit::<MemoryLinkInput>(input),
        "memory.forget" | "memory.archive" => misfit::<MemoryStateInput>(input),
        "memory.delete" => misfit::<MemoryDeleteInput>(input),
        "memory.validate" => misfit::<MemoryScopeInput>(input),
        "memory.repair" => misfit::<MemoryRepairInput>(input),
        _ => None,
    }?;
    Some(super::browser::outside_detail(&problem, 200))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_memory::MemoryError as E;
    use aura_memory_domain::MemoryFormatError as F;

    fn said(error: E, kind: &str, input: &Value, uncertain: bool) -> String {
        model_message(&anyhow::Error::new(error), kind, input, uncertain)
    }

    #[test]
    fn a_refusal_names_the_rule_and_that_nothing_was_written() {
        let input = json!({"scope": "everywhere"});
        assert_eq!(
            said(E::Invalid("memory scope"), "memory.search", &input, false),
            "memory.invalid_input: scope must be \"global\" or \"project\"; nothing was written"
        );
        let deleted = said(E::Invalid("duplicate delete"), "memory.delete", &input, false);
        assert!(deleted.contains("listed more than once in one delete"), "{deleted}");
        let unterminated =
            said(E::Format(F::Invalid("unterminated frontmatter")), "memory.read", &input, false);
        assert!(unterminated.contains("must end with a line '---'"), "{unterminated}");
        let line = said(E::Format(F::Frontmatter(4)), "memory.read", &input, false);
        assert!(line.contains("invalid at line 4") && line.contains("memory.validate"), "{line}");
    }

    #[test]
    fn arguments_that_do_not_fit_name_what_did_not_fit() {
        let input = json!({"scope": "project", "items": [{"path": "rules/a.md"}]});
        let text = said(E::Invalid("memory tool input"), "memory.create", &input, false);
        assert!(
            text.starts_with("memory.invalid_input: the arguments for memory.create do not fit"),
            "{text}"
        );
        assert!(text.contains("missing field"), "{text}");
        assert!(text.ends_with("; nothing was written"), "{text}");
    }

    #[test]
    fn a_write_that_may_have_been_saved_says_so_and_how_to_check() {
        let input = json!({"scope": "project"});
        let error = std::io::Error::other("disk gone");
        let text = said(E::Io(error), "memory.create", &input, true);
        assert!(text.starts_with("memory.failed: Aura could not confirm"), "{text}");
        assert!(text.contains("memory.read"), "{text}");
        let stopped = said(E::RecoveryRequired, "memory.update", &input, true);
        assert!(stopped.contains("restart Aura"), "{stopped}");
    }

    #[test]
    fn a_failure_from_outside_the_memory_rules_never_shows_its_own_text() {
        let error = anyhow::anyhow!("memory I/O failed: C:\\Users\\someone\\.memories\\a.md");
        let text = model_message(&error, "memory.read", &Value::Null, false);
        assert!(text.starts_with("memory.failed: the memory call did not complete"), "{text}");
        assert!(!text.contains('\\') && !text.contains("C:"), "{text}");
        let own = anyhow::anyhow!(VAULT_CHANGED);
        assert_eq!(model_message(&own, "memory.read", &Value::Null, false), VAULT_CHANGED);
    }
}
