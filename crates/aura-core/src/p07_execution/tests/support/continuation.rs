use super::*;

pub(super) fn assert_and_persist_p06_tool_continuation(
    persistence: &PersistenceCoordinator,
    initial: &P06PreparedFixture,
    driver: &P08ContractTestDriverV1,
    tool_batch_id: ToolBatchId,
    result: &ToolResultV1,
) {
    let continuation = load_p08_tool_continuation_authority_v1(
        persistence,
        tool_batch_id,
        initial.current_user_event_id,
    )
    .expect("load durable P06 continuation authority")
    .expect("terminal ToolBatch has a continuation authority");
    assert_eq!(continuation.durable_result_tool_call_ids, vec![result.tool_call_id]);
    assert!(continuation.provider_continuation.is_none());
    let transition = driver
        .begin_tool_continuation(&continuation)
        .expect("only the P08 Contract Driver creates ToolContinuation");
    let followup = p06_prepared_tool_continuation_fixture(
        initial,
        transition.next_model_step_id,
        transition.trigger,
        continuation.facts,
        continuation.recent_history_items,
    )
    .expect("P06 rebuilds the next request from immutable durable history");
    commit_p06_projection_fixture(persistence, &followup)
        .expect("persist the next P06 Context projection");
    assert_ne!(followup.execution_snapshot.model_step_id, initial.execution_snapshot.model_step_id,);
    assert_eq!(
        followup
            .prepared
            .context
            .conversation_items
            .iter()
            .filter(|item| matches!(item, LogicalConversationItem::User { .. }))
            .count(),
        1,
        "ToolContinuation must not create, copy, or forge a second User event",
    );
    assert!(matches!(
        followup.prepared.context.conversation_items.as_slice(),
        [
            LogicalConversationItem::User { source_event_id, .. },
            LogicalConversationItem::Assistant { tool_calls, .. },
            LogicalConversationItem::ToolResult { tool_call_id, .. },
        ] if *source_event_id == initial.current_user_event_id
            && tool_calls.len() == 1
            && *tool_call_id == result.tool_call_id
    ));
    assert!(followup.prepared.context.provider_continuation.is_none());
}

pub(super) fn strict_sandbox_no_effect_evidence(
    persistence: &PersistenceCoordinator,
    attempt_id: ToolExecutionAttemptId,
    effect_scope_hash: aura_domain_common::jcs::CanonicalSha256HashV1,
) -> VerifiedAttemptSideEffectEvidenceV1 {
    let receipt = persistence
        .write_artifact(ArtifactWriteRequest {
            scope: ArtifactScope::ToolExecution,
            media_type: "application/vnd.aura.strict-sandbox-no-effect-receipt.v1+json"
                .to_owned(),
            access: ArtifactAccess::NONE,
            owner: ArtifactOwnerRef::Tool {
                owner: ToolArtifactOwnerRef::ExecutionAttempt {
                    tool_execution_attempt_id: attempt_id,
                },
            },
            bytes: br#"{"policy_version":1,"mutable_channels_authorized":false,"process_tree_terminated":true}"#
                .to_vec(),
        })
        .expect("persist strict Sandbox no-effect receipt");
    VerifiedAttemptSideEffectEvidenceV1::from_core_evidence(
        attempt_id,
        effect_scope_hash,
        SideEffectEvidenceProducerV1::StrictSandbox,
        AttemptSideEffectProofV1::NoEffect { evidence_ref: receipt.artifact_id },
    )
    .expect("build exact typed no-effect evidence")
}

pub(super) fn strict_sandbox_unknown_evidence(
    persistence: &PersistenceCoordinator,
    attempt_id: ToolExecutionAttemptId,
    effect_scope_hash: aura_domain_common::jcs::CanonicalSha256HashV1,
) -> VerifiedAttemptSideEffectEvidenceV1 {
    let receipt = persistence
        .write_artifact(ArtifactWriteRequest {
            scope: ArtifactScope::ToolExecution,
            media_type: "application/vnd.aura.strict-sandbox-unknown-effect.v1+json".to_owned(),
            access: ArtifactAccess::NONE,
            owner: ArtifactOwnerRef::Tool {
                owner: ToolArtifactOwnerRef::ExecutionAttempt {
                    tool_execution_attempt_id: attempt_id,
                },
            },
            bytes: br#"{"policy_version":1,"process_tree_terminated":true,"external_effect_verified":false}"#
                .to_vec(),
        })
        .expect("persist strict Sandbox unknown-effect evidence");
    VerifiedAttemptSideEffectEvidenceV1::from_core_evidence(
        attempt_id,
        effect_scope_hash,
        SideEffectEvidenceProducerV1::StrictSandbox,
        AttemptSideEffectProofV1::Unknown { evidence_ref: receipt.artifact_id },
    )
    .expect("build exact typed unknown-effect evidence")
}

pub(super) fn trusted_network_no_effect_evidence(
    persistence: &PersistenceCoordinator,
    attempt_id: ToolExecutionAttemptId,
    effect_scope_hash: aura_domain_common::jcs::CanonicalSha256HashV1,
) -> VerifiedAttemptSideEffectEvidenceV1 {
    let receipt = persistence
        .write_artifact(ArtifactWriteRequest {
            scope: ArtifactScope::ToolExecution,
            media_type: "application/vnd.aura.trusted-network-no-effect-receipt.v1+json"
                .to_owned(),
            access: ArtifactAccess::NONE,
            owner: ArtifactOwnerRef::Tool {
                owner: ToolArtifactOwnerRef::ExecutionAttempt {
                    tool_execution_attempt_id: attempt_id,
                },
            },
            bytes: br#"{"policy_version":1,"method":"GET","request_terminal":true,"remote_mutation_authorized":false}"#
                .to_vec(),
        })
        .expect("persist trusted Network no-effect receipt");
    VerifiedAttemptSideEffectEvidenceV1::from_core_evidence(
        attempt_id,
        effect_scope_hash,
        SideEffectEvidenceProducerV1::TrustedNetworkAdapter,
        AttemptSideEffectProofV1::NoEffect { evidence_ref: receipt.artifact_id },
    )
    .expect("build exact typed Network no-effect evidence")
}
