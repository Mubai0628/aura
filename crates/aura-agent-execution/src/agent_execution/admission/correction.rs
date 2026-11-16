//! Track the arguments rejected before execution so the turn can stop a repeat loop.
use aura_agent_domain::DeterministicFailure;
use aura_domain_common::UtcTimestamp;
use aura_persistence_core::{StoreResult, Transaction};
use aura_persistence_v2::agent::guard;
use aura_tool_contract::ToolSchemaErrorCode;
use aura_tool_contract::{
    NormalizedToolCallInputStateV1, ToolBatchRequestV1, ToolNotExecutedReasonV1,
};
use aura_tool_scheduler::ToolPreflightRejectionV1;

pub(super) fn record_invalid_arguments(
    tx: &Transaction<'_>,
    batch: &ToolBatchRequestV1,
    rejections: &[ToolPreflightRejectionV1],
    at: UtcTimestamp,
) -> StoreResult<()> {
    let code = ToolSchemaErrorCode::ArgumentsInvalid.error_code();
    for rejection in rejections {
        if rejection.reason != ToolNotExecutedReasonV1::SchemaValidationFailed
            || rejection.error_code.as_ref() != Some(&code)
        {
            continue;
        }
        let Some(call) =
            batch.calls.iter().find(|call| call.tool_call_id == rejection.tool_call_id)
        else {
            continue;
        };
        let Some(definition) = &call.definition_ref else { continue };
        // The purpose can vary between equivalent calls. For a malformed envelope there
        // are no verified business arguments, so only an identical envelope is counted.
        let hash = match &call.input {
            NormalizedToolCallInputStateV1::Ready { arguments_hash, .. } => arguments_hash,
            NormalizedToolCallInputStateV1::InvalidInput { .. } => {
                &call.provider_arguments_envelope_hash
            }
        };
        let failure = DeterministicFailure::schema_constraint(
            definition.tool_id.to_string(),
            format!("valid_arguments:{}", hash.hex()),
            "arguments refused by validation",
            "Correct the arguments described by the tool result.",
        );
        guard::record_failure(tx, batch.invocation_identity.turn_task_id, &failure, at)?;
    }
    Ok(())
}
