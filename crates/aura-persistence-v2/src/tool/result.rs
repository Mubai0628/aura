//! Materializes the model result only from the same durable execution receipt.
//! Repeated publication reuses the one terminal record and its artifacts.
use super::execution;
use aura_domain_common::*;
use aura_execution_contract::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact};
use aura_tool_contract::*;
use rusqlite::{Connection, Transaction};
use serde_json::Value;

/// The execution contract's marker for a call that ended before it was started.
const NOT_DISPATCHED: &str = "execution.not_dispatched";
mod model_line;
mod notice;

use model_line::{model_safe_named_error, not_started_message, unreachable_target};

pub(super) fn refusal_artifact(
    tx: &Transaction<'_>,
    batch: &ToolBatchRequestV1,
    call: &NormalizedToolCallV1,
    result: &ToolResultV1,
) -> StoreResult<ToolResultV1> {
    let ToolResultOutcomeV1::NotExecuted { reason, error } = &result.outcome else {
        return Err(Error::Conflict);
    };
    let mut result = result.clone();
    if result.model_visible_result_ref.is_some() {
        return Ok(result);
    }
    let envelope = ModelVisibleToolResultEnvelopeV1 {
        schema_version: ToolWireVersionV1,
        tool_call_id: call.tool_call_id,
        definition_ref: call.definition_ref.clone(),
        outcome: ModelVisibleToolResultOutcomeV1::NotExecuted {
            reason: *reason,
            error: error.as_ref().map(|e| ModelSafeToolErrorV1 {
                code: e.code.clone(),
                category: e.category,
                message: e.message.clone(),
                retryable: e.retryable,
            }),
        },
        side_effect_state: result.side_effect_state,
        warnings: vec![],
        artifacts: vec![],
        completed_at: result.completed_at,
    };
    envelope.validate().map_err(|_| Error::Conflict)?;
    let id = ArtifactId::new();
    artifact::put(
        tx,
        id,
        &call.tool_call_id.to_string(),
        "application/vnd.aura.model-tool-result+json",
        &serde_json::to_vec(&envelope)?,
        result.completed_at,
    )?;
    artifact::grant_access(
        tx,
        id,
        &call.tool_call_id.to_string(),
        &batch.invocation_identity.agent_run_id.to_string(),
    )?;
    result.model_visible_result_ref = Some(id);
    result.user_audit_result_ref = Some(id);
    Ok(result)
}

/// Paired refusal without an execution Attempt. Repeated refusals reuse the
/// existing terminal artifact; they can never overwrite a dispatched outcome.
pub fn not_executed(
    tx: &Transaction<'_>,
    batch_id: ToolBatchId,
    call_id: ToolCallId,
    reason: ToolNotExecutedReasonV1,
    at: UtcTimestamp,
) -> StoreResult<ToolResultV1> {
    not_executed_with_notice(tx, batch_id, call_id, reason, None, at)
}

/// Same refusal as [`not_executed`], with a bounded explanation for the model and the user.
pub fn not_executed_with_notice(
    tx: &Transaction<'_>,
    batch_id: ToolBatchId,
    call_id: ToolCallId,
    reason: ToolNotExecutedReasonV1,
    notice: Option<&str>,
    at: UtcTimestamp,
) -> StoreResult<ToolResultV1> {
    let call = execution::call(tx, call_id)?;
    if let Some(previous) = call.result {
        return Ok(previous);
    }
    if call.attempt_id.is_some() {
        return Err(Error::Conflict);
    }
    let batch = execution::batch(tx, batch_id)?;
    if !batch.batch.calls.contains(&call.call) {
        return Err(Error::Conflict);
    }
    let notice = notice.map(|message| {
        let (code, category) = notice::classification(message);
        let error = ErrorEnvelope::new(
            code.clone(),
            category,
            message,
            false,
            CorrelationId::from_bytes(call_id.to_bytes()),
            None,
        );
        Ok::<_, Error>((
            error.clone(),
            ModelSafeToolErrorV1 {
                code,
                category,
                message: error.message.clone(),
                retryable: false,
            },
        ))
    });
    let notice = notice.transpose()?;
    let envelope = ModelVisibleToolResultEnvelopeV1 {
        schema_version: ToolWireVersionV1,
        tool_call_id: call_id,
        definition_ref: call.call.definition_ref.clone(),
        outcome: ModelVisibleToolResultOutcomeV1::NotExecuted {
            reason,
            error: notice.as_ref().map(|(_, safe)| safe.clone()),
        },
        side_effect_state: SideEffectStateV1::NotStarted,
        warnings: vec![],
        artifacts: vec![],
        completed_at: at,
    };
    envelope.validate().map_err(|_| Error::Conflict)?;
    let id = ArtifactId::new();
    artifact::put(
        tx,
        id,
        &call_id.to_string(),
        "application/vnd.aura.model-tool-result+json",
        &serde_json::to_vec(&envelope)?,
        at,
    )?;
    artifact::grant_access(
        tx,
        id,
        &call_id.to_string(),
        &batch.batch.invocation_identity.agent_run_id.to_string(),
    )?;
    let result = ToolResultV1 {
        tool_call_id: call_id,
        outcome: ToolResultOutcomeV1::NotExecuted { reason, error: notice.map(|(error, _)| error) },
        side_effect_state: SideEffectStateV1::NotStarted,
        attempts: vec![],
        model_visible_result_ref: Some(id),
        user_audit_result_ref: Some(id),
        internal_diagnostic_ref: None,
        aggregate_side_effect_evidence_ref: None,
        started_at: None,
        completed_at: at,
    };
    execution::complete_call(tx, batch_id, &result)?;
    Ok(result)
}

pub fn execution_output(
    db: &Connection,
    call_id: ToolCallId,
) -> StoreResult<(ExecutionAttempt, Value)> {
    let (attempt, output, _) = execution_evidence(db, call_id)?;
    Ok((attempt, output))
}

fn execution_evidence(
    db: &Connection,
    call_id: ToolCallId,
) -> StoreResult<(ExecutionAttempt, Value, Option<String>)> {
    let operation: String = db.query_row(
        "SELECT operation_id FROM tool_calls WHERE id=?1",
        [call_id.to_string()],
        |r| r.get(0),
    )?;
    let operation = operation.parse().map_err(|_| Error::Conflict)?;
    let attempt = crate::execution::load(db, operation, 1)?;
    let AttemptState::Terminal { result } = &attempt.state else {
        return Err(Error::Conflict);
    };
    let evidence: Value = serde_json::from_slice(&artifact::read(
        db,
        result.receipt.evidence_ref,
        &operation.to_string(),
    )?)?;
    let output = if let Some(journal) = evidence.get("domain_journal") {
        journal.get("output").cloned().unwrap_or(Value::Null)
    } else if evidence["schema"] == "controlled-web-result-v1" {
        evidence["output"].clone()
    } else if evidence["schema"] == "windows-user-execution-evidence-v2" {
        let session: String = db.query_row("SELECT json_extract(payload,'$.content.identity.session_id') FROM operations WHERE id=?1", [operation.to_string()], |r| r.get(0))?;
        // The model reads the state, the exit code and the output as text.
        serde_json::to_value(crate::execution::shell_result::model_result(
            db,
            session.parse().map_err(|_| Error::Conflict)?,
            call_id,
        )?)?
    } else if evidence["schema"] == "windows-user-execution-evidence-v1" {
        evidence["observation"].clone()
    } else if result.outcome == ExecutionOutcome::Succeeded {
        return Err(Error::Invalid("unrecognized execution evidence"));
    } else {
        Value::Null
    };
    // This optional summary is written by Core, not supplied by the Tool call.
    // It may explain a Failed result, but can never change its terminal state,
    // side effects or retryability. Older receipts keep the generic message.
    let binding = serde_json::to_value(
        jcs::canonical_sha256(&attempt.binding).map_err(|_| Error::Conflict)?,
    )?;
    let windows_message = (result.outcome == ExecutionOutcome::Failed
        && matches!(
            evidence["schema"].as_str(),
            Some("windows-user-execution-evidence-v1" | "windows-user-execution-evidence-v2")
        )
        && evidence["binding"] == binding)
        .then(|| evidence["model_safe_failure_message"].as_str())
        .flatten()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 8192
                && !s.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        })
        .map(str::to_owned);
    let web_message =
        (matches!(result.outcome, ExecutionOutcome::Failed | ExecutionOutcome::Unknown)
            && evidence["schema"] == "controlled-web-result-v1")
            .then(|| evidence["error"].as_str())
            .flatten()
            .and_then(model_safe_named_error)
            // An unknown outcome is told by Core's own sentence (what may have happened and what to
            // check), never by a bare code in place of the general unknown-outcome text.
            .filter(|line| result.outcome == ExecutionOutcome::Failed || line.contains(": "))
            .map(str::to_owned);
    let broker_message = ((result.outcome == ExecutionOutcome::Failed
        && matches!(
            evidence["domain_journal"]["schema"].as_str(),
            Some(
                "agent-terminal-effect-v1"
                    | "agent-native-git-v1"
                    | "agent-package-tool-v1"
                    | "agent-mcp-tool-v1"
                    // Its `error` is written for the model (the broken rule, no local path).
                    | "agent-memory-vault-v1"
                    // The browser's refusals and failures, in Core's and the host's words.
                    | "agent-browser-v1"
                    // The user's Chrome: what failed, and whether the step may have run.
                    | "agent-chrome-v1"
                    // So do the programs on the user's desktop.
                    | "agent-computer-v1"
                    // A sub-agent call's refusal, written for the model.
                    | "agent-team-v1"
                    // A picture or recording that could not be read, and why.
                    | "agent-media-v1"
            )
        ))
        || result.outcome == ExecutionOutcome::Unknown
            && matches!(
                evidence["domain_journal"]["schema"].as_str(),
                Some(
                    "agent-native-git-v1"
                        | "agent-package-tool-v1"
                        | "agent-mcp-tool-v1"
                        | "agent-chrome-v1"
                        | "agent-computer-v1"
                        // A merge or discard whose Git step's end was not seen says what to check.
                        | "agent-team-v1"
                        // A memory write that may have been saved, and how to check it.
                        | "agent-memory-vault-v1"
                        // A page step that may have run, and how to look at the page.
                        | "agent-browser-v1"
                )
            ))
    .then(|| evidence["domain_journal"]["error"].as_str())
    .flatten()
    .filter(|value| {
        value.len() <= 8192
            && !value.chars().any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    })
    .map(str::to_owned);
    // A file or task-state call's own cause, when it is already a line the model can read: its
    // record keeps it, and the model reads it instead of the general sentence.
    let own_message = (result.outcome == ExecutionOutcome::Failed && own_cause(&evidence))
        .then(|| evidence["domain_journal"]["error"].as_str())
        .flatten()
        .and_then(|error| {
            use crate::tool::refusal_text::{code_in, known, readable_line};
            readable_line(error).map(str::to_owned).or_else(|| {
                let sentence = known(error)?;
                Some(format!("{}: {sentence}", code_in(error).unwrap_or("tool.failed")))
            })
        });
    // A call that never started (refused, or stopped before it began) says so in the
    // model's words; the refusal's own reason stays in the receipt.
    let not_started = (result.outcome == ExecutionOutcome::Failed
        && result.warnings.iter().any(|warning| warning == NOT_DISPATCHED))
    .then(|| not_started_message(&evidence));
    Ok((
        attempt,
        output,
        windows_message.or(web_message).or(broker_message).or(own_message).or(not_started),
    ))
}

/// A file or task-state call, whose journal keeps its own cause.
fn own_cause(receipt: &Value) -> bool {
    matches!(
        receipt["domain_journal"]["schema"].as_str(),
        Some("agent-file-evidence-v2" | "agent-task-effect-v1")
    )
}

fn failed_runtime_code(receipt: &Value, named: Option<&str>) -> ErrorCode {
    if receipt["schema"] != "controlled-web-result-v1" {
        return ToolRuntimeErrorCode::DispatchFailed.error_code();
    }
    let unreachable = named.and_then(|line| unreachable_target(line.split(':').next()?.trim()));
    if let Some((code, _)) = unreachable {
        return code.error_code();
    }
    let blob = named.unwrap_or("").to_ascii_lowercase();
    if blob.starts_with("http.partial_response:") {
        return ToolResultErrorCode::HttpPartialResponse.error_code();
    }
    let code = if blob.contains("control_endpoint_blocked") {
        ToolRuntimeErrorCode::HttpControlEndpointDenied
    } else if blob.contains("credential_reflection") {
        ToolRuntimeErrorCode::HttpCredentialReflectionBlocked
    } else if blob.contains("download_limit")
        || blob.contains("upload_limit")
        || blob.contains("response_too_large")
        || blob.contains("byte_limit")
    {
        ToolRuntimeErrorCode::IoLimitExceeded
    } else if blob.starts_with("http.download_folder_") {
        // A folder on the way that is a link or a file, or could not be made: the call has to
        // name another place.
        return ToolResourceErrorCode::ScopeResolutionFailed.error_code();
    } else if blob.contains("redirect_blocked") || blob.contains("redirect_limit") {
        ToolRuntimeErrorCode::HttpRedirectDenied
    } else if blob.contains("invalid_url")
        || blob.contains("url_credentials_forbidden")
        || blob.contains("scheme_blocked")
        || blob.contains("invalid_method")
    {
        ToolRuntimeErrorCode::HttpRequestInvalid
    } else {
        ToolRuntimeErrorCode::HttpGatewayFailed
    };
    code.error_code()
}
/// Whether a Shell command Aura stopped keeps its output as its result: one stopped at its own
/// output or time limit, or because Core went away, says so in that result. A stop the user or the
/// model asked for stays a cancellation.
pub(super) fn kept_after_stop(outcome: ExecutionOutcome, receipt: &Value, output: &Value) -> bool {
    matches!(outcome, ExecutionOutcome::Cancelled | ExecutionOutcome::TimedOut)
        && receipt["schema"] == "windows-user-execution-evidence-v2"
        && matches!(
            output["stop_reason"].as_str(),
            Some("output_limit" | "timed_out" | "core_disconnected")
        )
}

pub(super) fn output_valid(
    db: &Connection,
    call_id: ToolCallId,
    output: &Value,
) -> StoreResult<bool> {
    let call = execution::call(db, call_id)?;
    let reference = call.call.definition_ref.as_ref().ok_or(Error::Conflict)?;
    let definition = super::definitions::get(db, reference.tool_id, &reference.tool_version)?
        .ok_or(Error::Conflict)?;
    let registry = super::registry(db, reference.registry_snapshot_ref.tool_registry_snapshot_id)?
        .ok_or(Error::Conflict)?;
    if registry.snapshot_ref != reference.registry_snapshot_ref
        || !registry.catalog_entries.iter().any(|e| {
            e.tool_id == definition.tool_id
                && e.tool_version == definition.tool_version
                && e.definition_fingerprint == definition.definition_fingerprint
        })
    {
        return Err(Error::Conflict);
    }
    Ok(match &definition.result_contract {
        ToolResultContractV1::Unit => output.is_null(),
        ToolResultContractV1::Utf8Text => output.is_string(),
        ToolResultContractV1::StructuredJson { result_schema } => {
            CompiledToolSchemaV1::compile(result_schema.canonical_document())
                .map_err(|_| Error::Conflict)?
                .validate(output)
                .valid
        }
    })
}
pub fn materialize(
    tx: &Transaction<'_>,
    batch_id: ToolBatchId,
    call_id: ToolCallId,
) -> StoreResult<ToolResultV1> {
    let call = execution::call(tx, call_id)?;
    if let Some(result) = call.result {
        return Ok(result);
    }
    let stored = execution::batch(tx, batch_id)?;
    if !stored.batch.calls.contains(&call.call) {
        return Err(Error::Conflict);
    }
    let (attempt, output, failure_message) = execution_evidence(tx, call_id)?;
    let AttemptState::Terminal { result: evidence } = &attempt.state else {
        return Err(Error::Conflict);
    };
    let receipt: Value = serde_json::from_slice(&artifact::read(
        tx,
        evidence.receipt.evidence_ref,
        &attempt.binding.attempt.operation_id.to_string(),
    )?)?;
    // A web call's rejection (a page DeepSeek could not fetch) is in the web receipt, and so is
    // the refusal of a call whose domain did not start it.
    let rejected = if receipt["schema"] == "controlled-web-result-v1"
        || receipt["schema"] == crate::execution::ENDED_BEFORE_START
    {
        &receipt["operation_rejection"]
    } else if matches!(
        receipt["domain_journal"]["schema"].as_str(),
        Some(
            "agent-file-evidence-v2"
                | "agent-task-effect-v1"
                | "agent-chrome-v1"
                | "agent-computer-v1"
                | "agent-memory-vault-v1"
                // A sub-agent call refused for its own input, and a secure input whose prompt
                // had gone.
                | "agent-team-v1"
                | "agent-terminal-effect-v1"
        )
    ) {
        &receipt["domain_journal"]["operation_rejection"]
    } else {
        &Value::Null
    };
    let rejection = if evidence.outcome == ExecutionOutcome::Failed
        && evidence.side_effect == SideEffectStateV1::NoEffect
        && !rejected.is_null()
    {
        Some(serde_json::from_value::<ModelSafeToolErrorV1>(rejected.clone())?)
    } else {
        None
    };
    let outcome = if kept_after_stop(evidence.outcome, &receipt, &output) {
        ExecutionOutcome::Succeeded
    } else {
        evidence.outcome
    };
    let invalid_output =
        outcome == ExecutionOutcome::Succeeded && !output_valid(tx, call_id, &output)?;
    let code = if invalid_output {
        ToolResultErrorCode::SchemaValidationFailed.error_code()
    } else if evidence.outcome == ExecutionOutcome::Unknown {
        ToolRuntimeErrorCode::DispatchOutcomeUnknown.error_code()
    } else if evidence.outcome == ExecutionOutcome::Failed {
        failed_runtime_code(&receipt, failure_message.as_deref())
    } else {
        ToolRuntimeErrorCode::DispatchFailed.error_code()
    };
    let entry = tool_error_catalog_v1()
        .entries
        .into_iter()
        .find(|e| e.code == code)
        .ok_or(Error::Conflict)?;
    let message = if invalid_output {
        "Execution ended, but its output does not satisfy the frozen Tool contract. Recorded side effects remain authoritative; do not retry automatically.".to_owned()
    } else {
        let effect = unexplained_failure(
            evidence.side_effect,
            receipt["action_kind"].as_str().unwrap_or_default(),
        );
        match failure_message.as_deref() {
            // A file or task-state call's cause comes with what Core recorded it changed.
            Some(cause)
                if own_cause(&receipt)
                    && !matches!(
                        evidence.side_effect,
                        SideEffectStateV1::NoEffect | SideEffectStateV1::NotStarted
                    ) =>
            {
                format!("{cause} {effect}")
            }
            Some(cause) => cause.to_owned(),
            None => effect,
        }
    };
    // A target that refused or could not be found was never sent the request: trying again,
    // once the cause is fixed, is what the model should do.
    let status = receipt["http_status"].as_u64();
    let read = matches!(receipt["action"].as_str(), Some("web.fetch" | "web.search"));
    let retryable = !invalid_output
        && (failure_message
            .as_deref()
            .and_then(|line| unreachable_target(line.split(':').next()?.trim()))
            .is_some_and(|(_, unsent)| unsent)
            || (read
                && evidence.outcome == ExecutionOutcome::Failed
                && evidence.side_effect == SideEffectStateV1::NoEffect
                && matches!(status, Some(408 | 429 | 502 | 503 | 504))));
    let status_failure = status.filter(|status| (400..=599).contains(status)).and_then(|status| {
        let name = failure_message.as_deref()?.split_once(':')?.0;
        let category = match (name, status) {
            ("web.http_failed", 404 | 410) | ("web.deepseek_fetch_empty", 404) => {
                ErrorCategory::NotFound
            }
            ("web.access_denied" | "web.deepseek_fetch_refused", 403) => ErrorCategory::Permission,
            ("web.http_failed" | "web.deepseek_unavailable" | "web.deepseek_rate_limited", _) => {
                ErrorCategory::Unavailable
            }
            _ => return None,
        };
        Some((ErrorCode::new(name).ok()?, category))
    });
    let (code, category) = status_failure.unwrap_or((entry.code, entry.category));
    let mut error = ErrorEnvelope::new(
        code,
        category,
        message,
        retryable,
        CorrelationId::from_bytes(call_id.to_bytes()),
        None,
    );
    if let Some(status) = status.filter(|status| (400..=599).contains(status)) {
        error = error
            .with_detail("http_status", serde_json::json!(status))
            .with_detail("http_source", receipt["http_source"].clone());
    }
    let safe = ModelSafeToolErrorV1 {
        code: error.code.clone(),
        category: error.category,
        message: error.message.clone(),
        retryable,
    };
    let (outcome, visible) = if let Some(rejection) = rejection {
        (
            ToolResultOutcomeV1::OperationRejected {
                error: ErrorEnvelope::new(
                    rejection.code.clone(),
                    rejection.category,
                    rejection.message.clone(),
                    false,
                    CorrelationId::from_bytes(call_id.to_bytes()),
                    None,
                ),
            },
            ModelVisibleToolResultOutcomeV1::OperationRejected { error: rejection },
        )
    } else {
        match outcome {
            ExecutionOutcome::Succeeded if invalid_output => (
                ToolResultOutcomeV1::Failed { error: error.clone() },
                ModelVisibleToolResultOutcomeV1::Failed { error: safe },
            ),
            ExecutionOutcome::Succeeded => (
                ToolResultOutcomeV1::Succeeded,
                ModelVisibleToolResultOutcomeV1::Succeeded {
                    payload: ModelVisibleToolSuccessPayloadV1::StructuredJson { value: output },
                },
            ),
            ExecutionOutcome::Cancelled => {
                // A call that ended before anything of it was sent was cut by its stopped turn.
                let reason = if receipt["schema"] == crate::execution::ENDED_BEFORE_START {
                    ToolCancellationReasonV1::TurnInterrupted
                } else {
                    ToolCancellationReasonV1::UserRequested
                };
                (
                    ToolResultOutcomeV1::Cancelled { reason },
                    ModelVisibleToolResultOutcomeV1::Cancelled { reason },
                )
            }
            ExecutionOutcome::TimedOut => (
                ToolResultOutcomeV1::Cancelled { reason: ToolCancellationReasonV1::Timeout },
                ModelVisibleToolResultOutcomeV1::Cancelled {
                    reason: ToolCancellationReasonV1::Timeout,
                },
            ),
            ExecutionOutcome::Failed => (
                ToolResultOutcomeV1::Failed { error: error.clone() },
                ModelVisibleToolResultOutcomeV1::Failed { error: safe },
            ),
            ExecutionOutcome::Unknown => (
                ToolResultOutcomeV1::Indeterminate {
                    reason: ToolIndeterminateReasonV1::ReconciliationInconclusive,
                    error,
                },
                ModelVisibleToolResultOutcomeV1::Indeterminate {
                    reason: ToolIndeterminateReasonV1::ReconciliationInconclusive,
                    error: safe,
                },
            ),
        }
    };
    // Publish a separately hashed copy of the safe outcome so later completion
    // tools can cite actual evidence. Never expose the private execution receipt
    // or make an envelope contain a hash of itself.
    let bytes = serde_json::to_vec(&visible)?;
    let outcome_id = ArtifactId::new();
    let media = "application/vnd.aura.model-tool-outcome+json";
    artifact::put(tx, outcome_id, &call_id.to_string(), media, &bytes, evidence.completed_at)?;
    artifact::grant_access(
        tx,
        outcome_id,
        &call_id.to_string(),
        &stored.batch.invocation_identity.agent_run_id.to_string(),
    )?;
    let visible_artifact = ModelVisibleToolArtifactRefV1 {
        artifact_id: outcome_id,
        slot: ToolOutputSlotKeyV1::new("result").map_err(|_| Error::Conflict)?,
        media_type: media.into(),
        byte_length: bytes.len() as u64,
        content_hash: ContentHash::sha256(&bytes),
    };
    let envelope = ModelVisibleToolResultEnvelopeV1 {
        schema_version: ToolWireVersionV1,
        tool_call_id: call_id,
        definition_ref: call.call.definition_ref.clone(),
        outcome: visible,
        side_effect_state: evidence.side_effect,
        warnings: model_visible_warnings(&evidence.warnings),
        artifacts: vec![visible_artifact],
        completed_at: evidence.completed_at,
    };
    envelope.validate().map_err(|_| Error::Invalid("model-visible Tool result"))?;
    let id = ArtifactId::new();
    artifact::put(
        tx,
        id,
        &call_id.to_string(),
        "application/vnd.aura.model-tool-result+json",
        &serde_json::to_vec(&envelope)?,
        evidence.completed_at,
    )?;
    artifact::grant_access(
        tx,
        id,
        &call_id.to_string(),
        &stored.batch.invocation_identity.agent_run_id.to_string(),
    )?;
    let action_kind: String = tx.query_row(
        "SELECT json_extract(payload,'$.content.action_kind') FROM operations WHERE id=?1",
        [attempt.binding.attempt.operation_id.to_string()],
        |r| r.get(0),
    )?;
    let actual_start = if matches!(action_kind.as_str(), "shell.powershell" | "shell.cmd") {
        crate::execution::shell::query(
            tx,
            stored.batch.invocation_identity.session_id,
            &aura_windows_user_contract::ShellQueryInput {
                tool_call_id: call_id,
                stdout_offset: 0,
                stderr_offset: 0,
                max_bytes: 1,
            },
        )?
        .started_at
    } else {
        attempt.dispatched_at
    };
    let result = ToolResultV1 {
        tool_call_id: call_id,
        outcome,
        side_effect_state: evidence.side_effect,
        attempts: vec![ToolExecutionAttemptOutcomeRefV1 {
            tool_execution_attempt_id: call.attempt_id.ok_or(Error::Conflict)?,
            attempt_number: 1,
            side_effect_state: evidence.side_effect,
            side_effect_evidence_ref: Some(evidence.receipt.evidence_ref),
            dispatched: attempt.dispatched_at.is_some(),
        }],
        model_visible_result_ref: Some(id),
        user_audit_result_ref: Some(id),
        internal_diagnostic_ref: None,
        aggregate_side_effect_evidence_ref: Some(evidence.receipt.evidence_ref),
        started_at: actual_start,
        completed_at: evidence.completed_at,
    };
    execution::complete_call(tx, batch_id, &result)?;
    Ok(result)
}

/// The warnings a model may read. The execution's own lifecycle markers (`execution.not_dispatched`)
/// are told by the outcome and its message instead, and a marker that is not a registered issue
/// code is left out: such a marker once made a call's result impossible to record,
/// and its run could then neither record its failure nor be stopped.
fn model_visible_warnings(warnings: &[String]) -> Vec<NormalizedIssueCode> {
    warnings.iter().filter_map(|warning| warning.parse().ok()).collect()
}

/// The message for a failure that no recognized cause explains.
/// A failure without a sentence of its own, told by what Core recorded of its effect: never "an
/// unknown effect" when Core knows what changed. A change that may have been made names the tools
/// that show the target as it is now.
fn unexplained_failure(side_effect: SideEffectStateV1, action: &str) -> String {
    let said = effect_sentence(side_effect);
    if matches!(side_effect, SideEffectStateV1::NoEffect | SideEffectStateV1::NotStarted) {
        return said.to_owned();
    }
    let family = action.split('.').next().unwrap_or_default();
    let look = match family {
        "fs" => " fs.read, fs.list or git.diff show how the files are now.",
        "git" => " git.status and git.log show how the repository is now.",
        "shell" => {
            " shell.query shows what the command printed; read what it writes to see what it did."
        }
        "terminal" => " terminal.read shows what the terminal shows now.",
        "memory" => " memory.read shows the memory as it is now.",
        _ => "",
    };
    format!("{said}{look}")
}

fn effect_sentence(side_effect: SideEffectStateV1) -> &'static str {
    match side_effect {
        SideEffectStateV1::NoEffect | SideEffectStateV1::NotStarted => {
            "The execution did not succeed and changed nothing. Check the arguments and the target before calling again."
        }
        SideEffectStateV1::PartiallyApplied => {
            "This call did not succeed, and it changed part of its target. Check what it changed before trying it again."
        }
        SideEffectStateV1::FullyApplied => {
            "This call did not succeed, although its change was made. Check the target before trying it again."
        }
        SideEffectStateV1::Compensated => {
            "This call did not succeed; the change it began was undone. Check the target before trying it again."
        }
        _ => {
            "Aura cannot tell whether this call took effect: it may have run fully, partly or not at all. Check its effect before trying it again."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_partial_download_reports_the_http_response_and_that_no_file_was_published() {
        let receipt = serde_json::json!({ "schema": "controlled-web-result-v1" });
        let line = "http.partial_response: 服务器回应 HTTP 206，只提供了分段内容，文件没有发布。";
        assert_eq!(model_safe_named_error(line), Some(line));
        assert_eq!(
            failed_runtime_code(&receipt, Some(line)),
            ToolResultErrorCode::HttpPartialResponse.error_code()
        );
    }

    #[test]
    fn a_target_core_could_not_reach_is_named_by_kind_with_what_to_do_next() {
        let receipt = serde_json::json!({ "schema": "controlled-web-result-v1" });
        let line = "network.connection_refused: 127.0.0.1:5188 没有程序在监听（连接被拒绝）。如果它是本机的开发服务器或其他服务，先启动它（例如重新运行启动命令），再重试这个请求。";
        assert_eq!(model_safe_named_error(line), Some(line), "Core's sentence reaches the model");
        assert_eq!(
            failed_runtime_code(&receipt, Some(line)),
            ToolNetworkErrorCode::ConnectionRefused.error_code()
        );
        assert_eq!(
            unreachable_target("network.connection_refused").map(|(_, unsent)| unsent),
            Some(true)
        );
        assert_eq!(
            unreachable_target("network.connection_reset").map(|(_, unsent)| unsent),
            Some(false)
        );
        let entry = tool_error_catalog_v1()
            .entries
            .into_iter()
            .find(|entry| entry.code == ToolNetworkErrorCode::HostNotFound.error_code())
            .expect("cataloged");
        assert_eq!(entry.category, ErrorCategory::Transport, "not 'unavailable'");
        // Anything else stays the gateway's failure, its diagnostics behind the code.
        assert_eq!(
            failed_runtime_code(&receipt, Some("network.transport_failed")),
            ToolRuntimeErrorCode::HttpGatewayFailed.error_code()
        );
    }

    #[test]
    fn a_gateway_timeout_is_recorded_as_a_timeout_in_core_words_not_as_unavailable() {
        let receipt = serde_json::json!({ "schema": "controlled-web-result-v1" });
        let line = "network.timeout: 下载超时：github.com:443 在 300 秒内没有传完（已收到 41.2 MB），文件没有写入。可以稍后重试或换一个下载地址；GitHub 这类网站慢时，可以在 设置 → 模型 → 网络代理 里用代理。";
        assert_eq!(model_safe_named_error(line), Some(line), "the model reads Core's sentence");
        let code = failed_runtime_code(&receipt, Some(line));
        assert_eq!(code, ToolNetworkErrorCode::TimedOut.error_code());
        let entry = tool_error_catalog_v1()
            .entries
            .into_iter()
            .find(|entry| entry.code == code)
            .expect("cataloged");
        assert_ne!(
            entry.category,
            ErrorCategory::Unavailable,
            "a timeout is not an unavailable tool"
        );
        assert_eq!(
            unreachable_target("network.timeout").map(|(_, unsent)| unsent),
            Some(false),
            "the request was sent"
        );
        // The bare code, as a proxied route reports it, is a timeout as well.
        assert_eq!(
            failed_runtime_code(&receipt, Some("network.timeout")),
            ToolNetworkErrorCode::TimedOut.error_code()
        );
        // A timeout Core reports before the call ran is not an unavailable tool either.
        let (_, category) = super::notice::classification("network.timeout: DNS 查询超时");
        assert_ne!(category, ErrorCategory::Unavailable);
    }

    #[test]
    fn a_transport_diagnostic_stays_behind_its_code_and_a_proxy_sentence_is_kept() {
        let hidden = model_safe_named_error("network.transport_failed: response body read failed")
            .expect("named code");
        assert_eq!(hidden, "network.transport_failed");
        let sent = model_safe_named_error(
            "network.user_proxy_unavailable: 无法连接代理 127.0.0.1:10808，尚未向目标发出 HTTP 请求。已改走直连，直连失败：network.transport_failed。",
        )
        .expect("proxy sentence");
        assert!(sent.contains("尚未向目标发出 HTTP 请求"), "{sent}");
        let uncertain = model_safe_named_error(
            "network.transport_failed: 本机代理 127.0.0.1:10808 的转发失败。不能确认目标请求是否已经发出，没有自动重放。",
        )
        .expect("uncertain sentence");
        assert!(uncertain.contains("不能确认目标请求是否已经发出"), "{uncertain}");
        let oversized =
            format!("network.transport_failed: {}尚未向目标发出 HTTP 请求", "x".repeat(512));
        assert_eq!(
            model_safe_named_error(&oversized).expect("bounded"),
            "network.transport_failed"
        );
        // A page that is not there (the web runtime's line): the model is told the status and
        // what it means.
        let missing = "web.http_failed: the server answered HTTP 404; there is no page at this \
                       address; check the address, or search for the page";
        assert_eq!(model_safe_named_error(missing), Some(missing));
    }

    #[test]
    fn an_unexplained_failure_that_changed_nothing_does_not_claim_an_unknown_effect() {
        let message = unexplained_failure(SideEffectStateV1::NoEffect, "fs.write");
        assert!(message.contains("changed nothing"), "{message}");
        assert!(!message.contains("fs.read"), "{message}");
        assert!(!message.contains("cannot tell"), "{message}");
        // Each recorded effect is told as it is, and every one says to check before trying again.
        for (state, said) in [
            (SideEffectStateV1::Unknown, "cannot tell whether this call took effect"),
            (SideEffectStateV1::PartiallyApplied, "it changed part of its target"),
            (SideEffectStateV1::FullyApplied, "although its change was made"),
            (SideEffectStateV1::Compensated, "the change it began was undone"),
        ] {
            let message = unexplained_failure(state, "fs.patch");
            assert!(message.contains(said), "{state:?}: {message}");
            assert!(message.contains("before trying it again"), "{state:?}: {message}");
            assert!(!message.contains("changed nothing"), "{state:?}: {message}");
            assert!(message.ends_with("fs.read, fs.list or git.diff show how the files are now."));
        }
        let shell = unexplained_failure(SideEffectStateV1::Unknown, "shell.powershell");
        assert!(shell.contains("shell.query"), "{shell}");
        let other = unexplained_failure(SideEffectStateV1::Unknown, "http.request");
        assert!(other.ends_with("before trying it again."), "{other}");
    }

    /// A file call that failed with a cause it already worded for the model reads that cause, with
    /// what Core recorded it changed when that is not nothing.
    #[test]
    fn a_file_calls_own_cause_reaches_the_model() {
        let receipt = serde_json::json!({
            "action_kind": "fs.patch",
            "domain_journal": {"schema": "agent-file-evidence-v2",
                "error": "fs.patch_conflict: hunk 2 no longer matches the file"},
        });
        assert!(own_cause(&receipt));
        assert!(!own_cause(
            &serde_json::json!({"domain_journal": {"schema": "agent-mcp-tool-v1"}})
        ));
        let line = crate::tool::refusal_text::readable_line(
            receipt["domain_journal"]["error"].as_str().expect("an error"),
        );
        assert_eq!(line, Some("fs.patch_conflict: hunk 2 no longer matches the file"));
    }
}
