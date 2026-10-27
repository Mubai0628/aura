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
mod notice;

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
    let web_message = (result.outcome == ExecutionOutcome::Failed
        && evidence["schema"] == "controlled-web-result-v1")
        .then(|| evidence["error"].as_str())
        .flatten()
        .and_then(model_safe_named_error)
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
                )
            ))
    .then(|| evidence["domain_journal"]["error"].as_str())
    .flatten()
    .filter(|value| {
        value.len() <= 8192
            && !value.chars().any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    })
    .map(str::to_owned);
    // A call that never started (refused, or stopped before it began) says so in the
    // model's words; the refusal's own reason stays in the receipt.
    let not_started = (result.outcome == ExecutionOutcome::Failed
        && result.warnings.iter().any(|warning| warning == NOT_DISPATCHED))
    .then(|| not_started_message(&evidence).to_owned());
    Ok((attempt, output, windows_message.or(web_message).or(broker_message).or(not_started)))
}

fn model_safe_named_error(raw: &str) -> Option<&str> {
    let first = raw.lines().next()?.trim();
    let code = first.split(':').next()?.trim();
    if code.is_empty() || code.len() > 128 {
        return None;
    }
    ErrorCode::new(code).ok()?;
    // Gateway diagnostics after the code stay hidden. Core's proxy sentences say whether
    // the HTTP request was sent, and those are the lines the model is meant to read.
    let visible = !first.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        && first.len() <= 512
        && (proxy_explanation_for_the_model(first) || core_web_service_line(code));
    Some(if visible { first } else { code })
}

/// The first line of a DeepSeek web service refusal is Core's own sentence.
fn core_web_service_line(code: &str) -> bool {
    code.starts_with("web.deepseek_")
        || code == "web.service_connection_unusable"
        || code == "web.search_query_too_long"
}

fn proxy_explanation_for_the_model(line: &str) -> bool {
    line.contains("尚未向目标发出 HTTP 请求") || line.contains("不能确认目标请求是否已经发出")
}

fn failed_runtime_code(receipt: &Value, named: Option<&str>) -> ToolRuntimeErrorCode {
    if receipt["schema"] != "controlled-web-result-v1" {
        return ToolRuntimeErrorCode::DispatchFailed;
    }
    let blob = named.unwrap_or("").to_ascii_lowercase();
    if blob.contains("control_endpoint_blocked") {
        ToolRuntimeErrorCode::HttpControlEndpointDenied
    } else if blob.contains("credential_reflection") {
        ToolRuntimeErrorCode::HttpCredentialReflectionBlocked
    } else if blob.contains("download_limit")
        || blob.contains("upload_limit")
        || blob.contains("response_too_large")
        || blob.contains("byte_limit")
    {
        ToolRuntimeErrorCode::IoLimitExceeded
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
    }
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
    // A web call's rejection (a page DeepSeek could not fetch) is in the web receipt.
    let rejected = if receipt["schema"] == "controlled-web-result-v1" {
        &receipt["operation_rejection"]
    } else if matches!(
        receipt["domain_journal"]["schema"].as_str(),
        Some("agent-file-evidence-v2" | "agent-task-effect-v1")
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
        failed_runtime_code(&receipt, failure_message.as_deref()).error_code()
    } else {
        ToolRuntimeErrorCode::DispatchFailed.error_code()
    };
    let entry = tool_error_catalog_v1()
        .entries
        .into_iter()
        .find(|e| e.code == code)
        .ok_or(Error::Conflict)?;
    let message = if invalid_output {
        "Execution ended, but its output does not satisfy the frozen Tool contract. Recorded side effects remain authoritative; do not retry automatically."
    } else {
        failure_message.as_deref().unwrap_or(unexplained_failure(evidence.side_effect))
    };
    let error = ErrorEnvelope::new(
        entry.code,
        entry.category,
        message,
        false,
        CorrelationId::from_bytes(call_id.to_bytes()),
        None,
    );
    let safe = ModelSafeToolErrorV1 {
        code: error.code.clone(),
        category: error.category,
        message: error.message.clone(),
        retryable: false,
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
            ExecutionOutcome::Cancelled => (
                ToolResultOutcomeV1::Cancelled { reason: ToolCancellationReasonV1::UserRequested },
                ModelVisibleToolResultOutcomeV1::Cancelled {
                    reason: ToolCancellationReasonV1::UserRequested,
                },
            ),
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

/// Why a call did not start, from its refusal's own reason code. Nothing else of the reason (an
/// operation identity, an internal message) reaches the model.
fn not_started_message(evidence: &Value) -> &'static str {
    let reason = if evidence["schema"] == "execution-prepared-rejection-v1" {
        evidence["reason"].as_str().unwrap_or_default()
    } else {
        ""
    };
    if reason.contains("agent.resource_changed") || reason.contains("prepared domain facts changed")
    {
        concat!(
            "This call was not started: its target changed between being prepared and being ",
            "started (another command may have been changing it at the same moment). Nothing was ",
            "changed. Check the target, then try again."
        )
    } else if reason.contains("agent.mode_changed") {
        concat!(
            "This call was not started: the session's permission mode changed before it started. ",
            "Nothing was changed."
        )
    } else if evidence["schema"] == "windows-user-execution-evidence-v2" {
        "This command was not started: the run stopped before it began. Nothing was changed."
    } else {
        "This call was not started: it was refused before it could run. Nothing was changed."
    }
}

/// The warnings a model may read. The execution's own lifecycle markers (`execution.not_dispatched`)
/// are told by the outcome and its message instead, and a marker that is not a registered issue
/// code is left out: such a marker once made a call's result impossible to record,
/// and its run could then neither record its failure nor be stopped.
fn model_visible_warnings(warnings: &[String]) -> Vec<NormalizedIssueCode> {
    warnings.iter().filter_map(|warning| warning.parse().ok()).collect()
}

/// The message for a failure that no recognized cause explains.
fn unexplained_failure(side_effect: SideEffectStateV1) -> &'static str {
    if side_effect == SideEffectStateV1::NoEffect {
        // Core recorded that nothing changed; "an unknown effect" would contradict that fact
        //.
        "The execution did not succeed and changed nothing. Check the arguments and the target before calling again."
    } else {
        "The execution did not succeed. Inspect the recorded result; do not retry an unknown effect."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transport_diagnostic_stays_behind_its_code_and_a_proxy_sentence_is_kept() {
        let hidden = model_safe_named_error("network.transport_failed: response body read failed")
            .expect("named code");
        assert_eq!(hidden, "network.transport_failed");
        let sent = model_safe_named_error(
            "network.user_proxy_unavailable: 无法连接工具设置中的代理 127.0.0.1:10808，尚未向目标发出 HTTP 请求。已改走直连，直连失败：network.transport_failed。",
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
    }

    #[test]
    fn an_unexplained_failure_that_changed_nothing_does_not_claim_an_unknown_effect() {
        let message = unexplained_failure(SideEffectStateV1::NoEffect);
        assert!(message.contains("changed nothing"), "{message}");
        assert!(!message.contains("unknown effect"), "{message}");
        for state in [
            SideEffectStateV1::Unknown,
            SideEffectStateV1::PartiallyApplied,
            SideEffectStateV1::FullyApplied,
            SideEffectStateV1::Compensated,
        ] {
            assert!(unexplained_failure(state).contains("do not retry an unknown effect"));
        }
    }
}
