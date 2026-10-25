use aura_context_domain::{HistoryAtomicGroupRefV1, ProviderContinuationBindingV1};
use aura_context_runtime::ToolContinuationFactsV1;
use aura_domain_common::{EventId, ModelStepId};
use aura_domain_common::{ToolBatchId, ToolCallId};
use aura_model_domain::LogicalConversationItem;
use aura_persistence::{PersistenceCoordinator, PersistenceError};

/// Durable authority handed to P08 after a ToolBatch reaches its unique
/// terminal Result set. It intentionally does not contain a
/// `ContextPreparationTriggerV1`: only P08 may create that transition and the
/// next ModelStep identity.
pub(crate) struct P08ToolContinuationAuthorityV1 {
    pub originating_user_event_id: EventId,
    pub previous_model_step_id: ModelStepId,
    pub assistant_response_event_id: EventId,
    pub tool_batch_id: ToolBatchId,
    pub history_atomic_group_ref: HistoryAtomicGroupRefV1,
    pub provider_continuation: Option<ProviderContinuationBindingV1>,
    pub recent_history_items: Vec<LogicalConversationItem>,
    pub durable_result_tool_call_ids: Vec<ToolCallId>,
    pub facts: ToolContinuationFactsV1,
}

pub(crate) fn load_p08_tool_continuation_authority_v1(
    persistence: &PersistenceCoordinator,
    tool_batch_id: ToolBatchId,
    originating_user_event_id: EventId,
) -> Result<Option<P08ToolContinuationAuthorityV1>, PersistenceError> {
    let Some(verified) = persistence
        .load_verified_tool_continuation_authority_v1(tool_batch_id, originating_user_event_id)?
    else {
        return Ok(None);
    };
    let history_atomic_group_ref = verified
        .history_atomic_group
        .group_ref()
        .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    let previous_model_step_id = verified.history_atomic_group.model_step_id;
    let assistant_response_event_id = verified.history_atomic_group.assistant_response_event_id;
    let provider_continuation = verified.provider_continuation;
    let durable_result_tool_call_ids = verified.durable_result_tool_call_ids;
    let facts = ToolContinuationFactsV1 {
        history_atomic_group: verified.history_atomic_group,
        previous_model_step_legally_terminal: true,
        assistant_response_durable: true,
        tool_batch_terminal: true,
        durable_result_tool_call_ids: durable_result_tool_call_ids.clone(),
        verified_provider_continuation: provider_continuation.clone(),
        accepted_additions: vec![],
    };
    Ok(Some(P08ToolContinuationAuthorityV1 {
        originating_user_event_id: verified.originating_user_event_id,
        previous_model_step_id,
        assistant_response_event_id,
        tool_batch_id,
        history_atomic_group_ref,
        provider_continuation,
        recent_history_items: verified.recent_history_items,
        durable_result_tool_call_ids,
        facts,
    }))
}
