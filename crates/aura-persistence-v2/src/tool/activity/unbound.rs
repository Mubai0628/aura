//! A refused name has a real call/result, but no executable Tool identity.
use super::*;

pub(super) fn project(
    db: &Connection,
    call: &super::super::execution::CallExecution,
    batch: &super::super::execution::BatchExecution,
) -> StoreResult<AgentToolActivityEntry> {
    let result = call.result.as_ref().ok_or(Error::Conflict)?;
    let ToolResultOutcomeV1::NotExecuted { reason, error } = &result.outcome else {
        return Err(Error::Conflict);
    };
    if call.attempt_id.is_some()
        || result.side_effect_state != aura_execution_contract::SideEffectStateV1::NotStarted
    {
        return Err(Error::Conflict);
    }
    let name = super::super::call_name::unbound(db, batch, &call.call)?;
    let mut display = display::project(db, call, batch, &name)?;
    display.title = name.chars().take(256).collect();
    display.internal = false;
    Ok(AgentToolActivityEntry {
        model_step_id: batch.batch.invocation_identity.model_step_id,
        display,
        summary: ToolCallActivitySummaryView {
            tool_call_id: call.call.tool_call_id,
            tool_batch_id: batch.batch.tool_batch_id,
            ordinal: call.call.ordinal,
            tool_id: None,
            tool_version: None,
            display_name: name,
            status: ToolCallStatusV1::Finalized,
            outcome: Some(ToolResultOutcomeSummaryView::NotExecuted { reason: *reason }),
            error_class: error.as_ref().map(|error| classify(error.category)),
            side_effect_state: Some(result.side_effect_state),
            started_at: None,
            completed_at: Some(result.completed_at),
            can_cancel: false,
        },
        shell_query_available: false,
        permission_mode: None,
        permission_revision: None,
    })
}
