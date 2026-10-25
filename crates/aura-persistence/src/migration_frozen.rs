//! Frozen copies for the historical long-operation migration step. The
//! long-operation module is retired; a migration keeps its original encoding.

pub(crate) fn long_operation_reservation_key_v1(
    approval_draft_id: aura_domain_common::ToolDraftId,
    approval_draft_revision: aura_domain_common::Revision,
    approval_evidence_hash: &aura_domain_common::jcs::CanonicalSha256HashV1,
) -> String {
    format!(
        "{}:{}:{}",
        approval_draft_id,
        approval_draft_revision.value(),
        approval_evidence_hash.hex()
    )
}

pub(crate) const fn long_operation_state_name(
    state: aura_tool_domain::ToolLongOperationStateV1,
) -> &'static str {
    use aura_tool_domain::ToolLongOperationStateV1 as State;
    match state {
        State::Queued => "queued",
        State::Running => "running",
        State::Cancelling => "cancelling",
        State::Cancelled => "cancelled",
        State::Succeeded => "succeeded",
        State::Failed => "failed",
        State::RecoveryRequired => "recovery_required",
    }
}
