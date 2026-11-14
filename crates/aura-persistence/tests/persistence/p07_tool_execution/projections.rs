use super::*;

pub(super) fn normalized_call(
    tool_call_id: ToolCallId,
    ordinal: u32,
    tool_id: ToolId,
    tool_version: ToolVersion,
    registry_snapshot_ref: aura_tool_contract::ToolRegistrySnapshotRef,
    purpose: &str,
    seed: u8,
) -> NormalizedToolCallV1 {
    let arguments = json!({"value": format!("fixture-{seed}")});
    NormalizedToolCallV1 {
        tool_call_id,
        ordinal,
        definition_ref: Some(ToolDefinitionSnapshotRef {
            tool_id,
            tool_version,
            registry_snapshot_ref,
        }),
        input: NormalizedToolCallInputStateV1::Ready {
            purpose: ToolPurposeTextV1::new(purpose).expect("purpose"),
            arguments: arguments.clone(),
            arguments_hash: aura_domain_common::jcs::canonical_sha256(&arguments)
                .expect("arguments hash"),
        },
        provider_arguments_envelope_hash: CanonicalHashV1::sha256(&[seed, 7]),
    }
}

pub(super) fn call_summary(
    batch: &aura_tool_contract::ToolBatchRequestV1,
    call: &NormalizedToolCallV1,
    display_name: &str,
    not_executed: Option<ToolNotExecutedReasonV1>,
) -> ToolCallActivitySummaryView {
    ToolCallActivitySummaryView {
        tool_call_id: call.tool_call_id,
        tool_batch_id: batch.tool_batch_id,
        ordinal: call.ordinal,
        tool_id: call.definition_ref.as_ref().map(|r| r.tool_id),
        tool_version: call.definition_ref.as_ref().map(|r| r.tool_version.clone()),
        display_name: display_name.to_owned(),
        status: not_executed.map_or(ToolCallStatusV1::Queued, |_| ToolCallStatusV1::Finalized),
        outcome: not_executed.map(|reason| ToolResultOutcomeSummaryView::NotExecuted { reason }),
        // A call that never ran has no stored error to classify; the not-executed reason
        // is the fact the row shows.
        error_class: None,
        error_code: None,
        side_effect_state: not_executed.map(|_| SideEffectStateV1::NotStarted),
        started_at: None,
        completed_at: not_executed.map(|_| timestamp()),
        can_cancel: not_executed.is_none(),
    }
}

pub(super) fn call_projection(
    batch: &aura_tool_contract::ToolBatchRequestV1,
    call: &NormalizedToolCallV1,
    not_executed: Option<ToolNotExecutedReasonV1>,
) -> ToolCallProjection {
    let purpose = match &call.input {
        NormalizedToolCallInputStateV1::Ready { purpose, .. } => Some(purpose.to_string()),
        NormalizedToolCallInputStateV1::InvalidInput { .. } => None,
    };
    ToolCallProjection {
        tool_call_id: call.tool_call_id,
        tool_batch_id: batch.tool_batch_id,
        ordinal: call.ordinal,
        definition_ref: call.definition_ref.clone(),
        purpose,
        invalid_input_issue: None,
        status: not_executed.map_or(ToolCallStatusV1::Queued, |_| ToolCallStatusV1::Finalized),
        outcome: not_executed.map(|reason| ToolResultOutcomeSummaryView::NotExecuted { reason }),
        side_effect_state: not_executed.map(|_| SideEffectStateV1::NotStarted),
        attempt_ids: Vec::new(),
        started_at: None,
        completed_at: not_executed.map(|_| timestamp()),
        safe_error: None,
        model_visible_result: None,
        user_audit_result: None,
        can_cancel: not_executed.is_none(),
    }
}
