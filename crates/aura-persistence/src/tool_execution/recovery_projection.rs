use super::*;

pub(super) fn recovery_call_summary(
    call: &RecoveryCallRow,
) -> Result<ToolCallActivitySummaryView, PersistenceError> {
    let result_summary = call.result.as_ref().map(|result| {
        (
            outcome_summary(&result.outcome),
            error_class(&result.outcome),
            result.side_effect_state,
            result.completed_at,
        )
    });
    Ok(ToolCallActivitySummaryView {
        tool_call_id: call.call.tool_call_id,
        tool_batch_id: call.tool_batch_id,
        ordinal: call.call.ordinal,
        tool_id: Some(resolved_tool(&call.call)?.tool_id),
        tool_version: Some(resolved_tool(&call.call)?.tool_version.clone()),
        display_name: call.display_name.clone(),
        status: call.status,
        outcome: result_summary.as_ref().map(|value| value.0),
        error_class: result_summary.as_ref().and_then(|value| value.1),
        error_code: None,
        side_effect_state: result_summary.as_ref().map(|value| value.2),
        started_at: call.started_at,
        completed_at: result_summary.map(|value| value.3),
        can_cancel: !matches!(
            call.status,
            ToolCallStatusV1::Finalized | ToolCallStatusV1::RecoveryRequired
        ),
    })
}

pub(super) fn recovery_call_projection(
    call: &RecoveryCallRow,
) -> Result<ToolCallProjection, PersistenceError> {
    let (purpose, invalid_input_issue) = match &call.call.input {
        NormalizedToolCallInputStateV1::Ready { purpose, .. } => (Some(purpose.to_string()), None),
        NormalizedToolCallInputStateV1::InvalidInput { issue, .. } => (None, Some(*issue)),
    };
    let result_summary = call.result.as_ref().map(|result| {
        (
            outcome_summary(&result.outcome),
            error_class(&result.outcome),
            result.side_effect_state,
            result.completed_at,
        )
    });
    let safe_error = call.result.as_ref().and_then(|result| match &result.outcome {
        ToolResultOutcomeV1::Failed { error }
        | ToolResultOutcomeV1::Indeterminate { error, .. } => Some(projection_error(error)),
        _ => None,
    });
    Ok(ToolCallProjection {
        tool_call_id: call.call.tool_call_id,
        tool_batch_id: call.tool_batch_id,
        ordinal: call.call.ordinal,
        definition_ref: call.call.definition_ref.clone(),
        purpose,
        invalid_input_issue,
        status: call.status,
        outcome: result_summary.as_ref().map(|value| value.0),
        side_effect_state: result_summary.as_ref().map(|value| value.2),
        attempt_ids: call
            .attempts
            .iter()
            .map(|attempt| attempt.record.tool_execution_attempt_id)
            .collect(),
        started_at: call.started_at,
        completed_at: result_summary.map(|value| value.3),
        safe_error,
        model_visible_result: None,
        user_audit_result: None,
        can_cancel: !matches!(
            call.status,
            ToolCallStatusV1::Finalized | ToolCallStatusV1::RecoveryRequired
        ),
    })
}
