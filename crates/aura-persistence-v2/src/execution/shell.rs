//! Append-only Shell observations reuse the operation journal and Artifact
//! store. Query authority comes from the original batch's session, not a path.
use aura_domain_common::*;
use aura_execution_contract::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact, event};
use aura_windows_user_contract::*;
use rusqlite::{Connection, Transaction};
use serde_json::{Value, json};

pub fn append(
    tx: &Transaction<'_>,
    request: &DispatchRequest,
    kind: &str,
    payload: &Value,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let attempt =
        super::load(tx, request.operation.operation_id, request.binding.attempt.ordinal.get())?;
    if attempt.binding != request.binding
        || !matches!(
            request.operation.content.action_kind.as_str(),
            "shell.powershell" | "shell.cmd"
        )
    {
        return Err(Error::Conflict);
    }
    event::append(
        tx,
        &event::EventDraft {
            id: EventId::new(),
            owner: &request.operation.operation_id.to_string(),
            kind,
            version: 2,
            payload,
            at,
        },
        "execution",
    )?;
    Ok(())
}

pub fn operation_for_call(
    db: &Connection,
    session: SessionId,
    call: ToolCallId,
) -> StoreResult<Option<SecuredOperation>> {
    let (batch, operation): (String, Option<String>) = db.query_row(
        "SELECT batch_id,operation_id FROM tool_calls WHERE id=?1",
        [call.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let batch = crate::tool::execution::batch(db, batch.parse().map_err(|_| Error::Conflict)?)?;
    if batch.batch.invocation_identity.session_id != session {
        return Err(Error::Conflict);
    }
    let Some(operation) = operation else {
        return Ok(None);
    };
    let bytes: String =
        db.query_row("SELECT payload FROM operations WHERE id=?1", [operation], |r| r.get(0))?;
    let operation: SecuredOperation = serde_json::from_str(&bytes)?;
    if !matches!(operation.content.action_kind.as_str(), "shell.powershell" | "shell.cmd") {
        return Err(Error::Conflict);
    }
    Ok(Some(operation))
}

/// What shell.query and shell.stop say about an id that names no Shell call of the session.
pub const NOT_A_SHELL_CALL: &str = "shell.tool_call_id: no shell.powershell or shell.cmd call in this session has this tool_call_id. Pass the tool_call_id field of that call's result (tcl_...), not the provider's call id, a tool name or an artifact id";

/// A missing call, another session's call and another tool's call are all the same wrong id.
fn not_a_shell_call(error: Error) -> Error {
    match error {
        Error::Sqlite(rusqlite::Error::QueryReturnedNoRows) | Error::Conflict => {
            Error::Invalid(NOT_A_SHELL_CALL)
        }
        other => other,
    }
}

pub fn query(
    db: &Connection,
    session: SessionId,
    input: &ShellQueryInput,
) -> StoreResult<ShellExecutionView> {
    if input.max_bytes == 0 || input.max_bytes > 65536 {
        return Err(Error::Invalid("shell.query.max_bytes: max_bytes is 1 to 65536"));
    }
    let operation =
        operation_for_call(db, session, input.tool_call_id).map_err(not_a_shell_call)?;
    let call = crate::tool::execution::call(db, input.tool_call_id)?;
    let reference = call.call.definition_ref.as_ref().ok_or(Error::Conflict)?;
    let aura_tool_contract::NormalizedToolCallInputStateV1::Ready { arguments, .. } =
        &call.call.input
    else {
        return Err(Error::Invalid("invalid original shell input"));
    };
    let original_input: ShellExecuteInput = serde_json::from_value(arguments.clone())?;
    let command: ManagedShellCommand = if let Some(operation) = &operation {
        serde_json::from_value(operation.content.action.clone())?
    } else {
        let registry =
            crate::tool::registry(db, reference.registry_snapshot_ref.tool_registry_snapshot_id)?
                .ok_or(Error::Conflict)?;
        let entry = registry
            .catalog_entries
            .iter()
            .find(|v| v.tool_id == reference.tool_id && v.tool_version == reference.tool_version)
            .ok_or(Error::Conflict)?;
        let shell = match entry.canonical_name.as_str() {
            "shell.powershell" => ManagedShell::PowerShell,
            "shell.cmd" => ManagedShell::Cmd,
            _ => return Err(Error::Invalid(NOT_A_SHELL_CALL)),
        };
        original_input.clone().command(shell).map_err(|_| Error::Invalid("original shell input"))?
    };
    let mut view = ShellExecutionView {
        tool_call_id: input.tool_call_id,
        original_input,
        command,
        submitted_script: None,
        environment_version: SHELL_ENVIRONMENT_VERSION.into(),
        shell_image: None,
        state: ShellCallState::Waiting,
        process_id: None,
        started_at: None,
        finished_at: None,
        exit_code: None,
        stop_reason: None,
        diagnostic: None,
        filtered_environment_names: vec![],
        background: None,
        stdout: vec![],
        stderr: vec![],
        stdout_total: 0,
        stderr_total: 0,
        stdout_next: None,
        stderr_next: None,
        output_truncated: false,
    };
    if let Some(result) = &call.result {
        use aura_tool_contract::ToolResultOutcomeV1 as O;
        view.state = match &result.outcome {
            O::Cancelled { .. }
            | O::NotExecuted {
                reason: aura_tool_contract::ToolNotExecutedReasonV1::CancelledBeforeDispatch,
                ..
            } => ShellCallState::Interrupted,
            O::Indeterminate { .. } => ShellCallState::Indeterminate,
            O::Failed { .. } => ShellCallState::Failed,
            _ => ShellCallState::Completed,
        };
        view.finished_at = Some(result.completed_at);
        if let O::NotExecuted { reason, error } = &result.outcome {
            view.diagnostic = Some(
                error
                    .as_ref()
                    .map(|e| e.message.clone())
                    .unwrap_or_else(|| format!("not executed: {reason:?}")),
            );
        }
    }
    let Some(operation) = operation else {
        return Ok(view);
    };
    let mut has_direct_snapshot = false;
    if call.attempt_id.is_some() {
        let attempt = super::load(db, operation.operation_id, 1)?;
        if let AttemptState::Terminal { result } = attempt.state {
            let receipt: Value = serde_json::from_slice(&artifact::read(
                db,
                result.receipt.evidence_ref,
                &operation.operation_id.to_string(),
            )?)?;
            view.state = match result.outcome {
                ExecutionOutcome::Succeeded => ShellCallState::Completed,
                ExecutionOutcome::Failed => ShellCallState::Failed,
                ExecutionOutcome::Cancelled | ExecutionOutcome::TimedOut => {
                    ShellCallState::Interrupted
                }
                ExecutionOutcome::Unknown => ShellCallState::Indeterminate,
            };
            view.finished_at = Some(result.completed_at);
            if !receipt["not_started"].is_null() {
                view.diagnostic =
                    Some(format!("execution not started: {}", receipt["not_started"]));
            }
            if !receipt["observation"].is_null() {
                has_direct_snapshot = true;
                let observed: RunnerObservation =
                    serde_json::from_value(receipt["observation"].clone())?;
                view.process_id = Some(observed.process_id);
                view.started_at = Some(observed.started_at);
                view.finished_at = Some(observed.finished_at);
                view.exit_code = observed.exit_code;
                view.stop_reason = Some(observed.stop_reason);
                view.shell_image = Some(observed.shell_image);
                view.submitted_script =
                    (!observed.submitted_script.is_empty()).then_some(observed.submitted_script);
                view.background =
                    (!observed.process_tree_stopped).then_some(BackgroundState::Running);
                view.stdout = observed.stdout;
                view.stderr = observed.stderr;
                view.output_truncated = observed.output_truncated;
            }
        }
    }
    let mut query = db.prepare(
        "SELECT kind,payload FROM events WHERE owner=?1 AND kind LIKE 'shell.%' ORDER BY rowid",
    )?;
    for row in query.query_map([operation.operation_id.to_string()], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })? {
        let (kind, text) = row?;
        let payload: Value = serde_json::from_str(&text)?;
        match kind.as_str() {
            "shell.progress" => {
                let update: RunnerProgress = serde_json::from_value(payload)?;
                if view.submitted_script.is_none() {
                    view.submitted_script = update.submitted_script;
                }
                // Terminal snapshots contain these bytes. On transport loss,
                // retain only received deltas and do not invent an exit status.
                if !has_direct_snapshot {
                    view.stdout.extend(update.stdout);
                    view.stderr.extend(update.stderr);
                    view.output_truncated |= update.output_truncated;
                }
            }
            "shell.diagnostic" => {
                view.diagnostic = payload["reason"].as_str().map(str::to_owned);
            }
            "shell.environment_filtered" => {
                view.filtered_environment_names = serde_json::from_value(payload["names"].clone())?;
            }
            "shell.started" => {
                if view.state == ShellCallState::Waiting {
                    view.state = ShellCallState::Running;
                }
                if view.process_id.is_none() {
                    view.process_id =
                        payload["process_id"].as_u64().and_then(|v| u32::try_from(v).ok());
                }
                if view.started_at.is_none() {
                    view.started_at = Some(serde_json::from_value(payload["at"].clone())?);
                }
            }
            "shell.background" => {
                let update: RunnerBackgroundUpdate = serde_json::from_value(payload)?;
                view.background = Some(update.state);
                view.stdout.extend(update.stdout);
                view.stderr.extend(update.stderr);
                view.output_truncated |= update.output_truncated;
            }
            "shell.late_observation" => {
                // Preserve the original indeterminate terminal record; expose recovered facts separately.
                if view.exit_code.is_none() && !payload["observation"].is_null() {
                    let observed: RunnerObservation =
                        serde_json::from_value(payload["observation"].clone())?;
                    view.process_id = Some(observed.process_id);
                    view.started_at = Some(observed.started_at);
                    view.exit_code = observed.exit_code;
                    view.stop_reason = Some(observed.stop_reason);
                    view.shell_image = Some(observed.shell_image);
                    view.submitted_script = Some(observed.submitted_script);
                    view.stdout = observed.stdout;
                    view.stderr = observed.stderr;
                    view.output_truncated |= observed.output_truncated;
                    view.diagnostic = Some("Late execution facts recovered; original indeterminate result is retained and was not replayed.".into());
                }
            }
            "shell.runtime_stopped" if view.background == Some(BackgroundState::Running) => {
                view.background = Some(if payload["stopped"] == true {
                    BackgroundState::Interrupted
                } else {
                    BackgroundState::Unknown
                });
            }
            "shell.runtime_lost" if view.background == Some(BackgroundState::Running) => {
                view.background = Some(BackgroundState::Unknown);
            }
            _ => {}
        }
    }
    view.stdout_total = view.stdout.len() as u64;
    view.stderr_total = view.stderr.len() as u64;
    (view.stdout, view.stdout_next) = page(view.stdout, input.stdout_offset, input.max_bytes)?;
    (view.stderr, view.stderr_next) = page(view.stderr, input.stderr_offset, input.max_bytes)?;
    Ok(view)
}

pub fn request_stop(
    tx: &Transaction<'_>,
    session: SessionId,
    call: ToolCallId,
    at: UtcTimestamp,
) -> StoreResult<Option<SecurityOperationId>> {
    // Query validates the exact original Shell and session, including queued calls.
    query(
        tx,
        session,
        &ShellQueryInput { tool_call_id: call, stdout_offset: 0, stderr_offset: 0, max_bytes: 1 },
    )?;
    let operation = operation_for_call(tx, session, call)?;
    if let Some(operation) = operation {
        super::request_cancel(
            tx,
            &AttemptRef {
                operation_id: operation.operation_id,
                ordinal: std::num::NonZeroU32::MIN,
            },
            at,
        )?;
        Ok(Some(operation.operation_id))
    } else {
        let batch: String =
            tx.query_row("SELECT batch_id FROM tool_calls WHERE id=?1", [call.to_string()], |r| {
                r.get(0)
            })?;
        crate::tool::result::not_executed(
            tx,
            batch.parse().map_err(|_| Error::Conflict)?,
            call,
            aura_tool_contract::ToolNotExecutedReasonV1::CancelledBeforeDispatch,
            at,
        )?;
        Ok(None)
    }
}

/// A new Core cannot assert that old background children are still running.
/// The original direct result stays immutable; actual Job reconciliation is separate.
pub fn runtime_restarted(tx: &Transaction<'_>, at: UtcTimestamp) -> StoreResult<()> {
    let mut q = tx.prepare("SELECT id FROM operations WHERE json_extract(payload,'$.content.action_kind') IN ('shell.powershell','shell.cmd')")?;
    let ids = q.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    for id in ids {
        let running: bool = tx.query_row("SELECT COALESCE((SELECT CASE WHEN kind='shell.background' THEN json_extract(payload,'$.state')='running' WHEN kind='shell.runtime_stopped' OR kind='shell.runtime_lost' THEN 0 ELSE 1 END FROM events WHERE owner=?1 AND kind IN ('shell.started','shell.background','shell.runtime_stopped','shell.runtime_lost') ORDER BY rowid DESC LIMIT 1),0)", [&id], |r|r.get(0))?;
        if running {
            event::append(
                tx,
                &event::EventDraft {
                    id: EventId::new(),
                    owner: &id,
                    kind: "shell.runtime_lost",
                    version: 2,
                    payload: &json!({"reason":"core_restarted","replayed":false}),
                    at,
                },
                "execution",
            )?;
        }
    }
    Ok(())
}

fn page(bytes: Vec<u8>, offset: u64, limit: u32) -> StoreResult<(Vec<u8>, Option<u64>)> {
    let start = usize::try_from(offset).map_err(|_| Error::Invalid("shell offset"))?;
    if start > bytes.len() {
        return Err(Error::Invalid("shell offset"));
    }
    let end = start.saturating_add(limit as usize).min(bytes.len());
    Ok((bytes[start..end].to_vec(), (end < bytes.len()).then_some(end as u64)))
}

/// A lost actual execution observation becomes a terminal indeterminate call,
/// never a hidden retry or an indefinitely running history entry.
pub fn lost(
    tx: &Transaction<'_>,
    operation: SecurityOperationId,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let attempt = super::load(tx, operation, 1)?;
    if matches!(attempt.state, AttemptState::Terminal { .. }) {
        return Ok(());
    }
    let not_dispatched = attempt.dispatched_at.is_none();
    let id = ArtifactId::new();
    artifact::put(
        tx,
        id,
        &operation.to_string(),
        "application/vnd.aura.command-evidence+json",
        &serde_json::to_vec(&json!({
            "schema":"windows-user-execution-evidence-v2","observation":null,"missing":["direct_exit_status"],"replayed":false
        }))?,
        at,
    )?;
    super::complete(
        tx,
        operation,
        1,
        &ExecutionResult {
            outcome: if not_dispatched {
                ExecutionOutcome::Failed
            } else {
                ExecutionOutcome::Unknown
            },
            side_effect: if not_dispatched {
                SideEffectStateV1::NoEffect
            } else {
                SideEffectStateV1::Unknown
            },
            receipt: ExecutionReceipt {
                attempt: attempt.binding.attempt,
                evidence_ref: id,
                postcondition_fingerprint: None,
                rollback: if not_dispatched {
                    RollbackEvidence::NotApplicable
                } else {
                    RollbackEvidence::Unavailable
                },
            },
            termination: if not_dispatched {
                TerminationEvidence::Confirmed { evidence_ref: id }
            } else {
                TerminationEvidence::Unknown
            },
            completed_at: at,
            warnings: if not_dispatched { vec!["execution.not_dispatched".into()] } else { vec![] },
        },
    )
}
