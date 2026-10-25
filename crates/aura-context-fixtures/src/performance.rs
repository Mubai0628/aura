use aura_context_domain::{
    ContextCompressibility, ContextPreparationRequest, ContextPreparationTriggerV1,
    ContextPrivacyClass, ContextSectionKind, ContextSourceRef, ModelEgressAuthorizationBasis,
    ModelEgressSourceKind, PromptActivationCondition, PromptBundleSnapshot, PromptModuleSnapshot,
    PromptSectionClass,
};
use aura_context_runtime::{
    ContextFragment, ContextPreparationInput, ContextProviderTargetV1, ContextSectionInput,
    ModelEstimatorKey, PromptInspectorInput,
};
use aura_domain_common::CanonicalSemVer;
use aura_domain_common::{
    AgentRunId, ArtifactId, ContentHash, CorrelationId, EndpointId, EventId, ModelProfileId,
    ModelStepId, PromptBundleSnapshotId, ProviderId, Revision, SessionId, SessionSequence,
    TurnTaskId, UtcTimestamp, WorkspaceId,
};
use aura_model_domain::{ModelApiFormat, ModelOutputFormat, ModelOutputRequest};
use aura_security_contract::{InvocationIdentity, NetworkDataClass};

use crate::ContextFixtureError;

/// Builds a deterministic-shape, model-visible Context input for repeatable P06 performance gates.
///
/// The generated text is synthetic public content and never reaches a Provider. The caller selects
/// both the runtime window and the exact UTF-8 byte count so the benchmark exercises the frozen
/// `Utf8ByteUpperBoundV1` fallback without relying on language-dependent token guesses.
pub fn p06_performance_preparation_input(
    runtime_effective_window_tokens: u64,
    current_user_utf8_bytes: usize,
) -> Result<ContextPreparationInput, ContextFixtureError> {
    if runtime_effective_window_tokens == 0 || current_user_utf8_bytes == 0 {
        return Err(ContextFixtureError::PreparationFailed);
    }

    let workspace_id = WorkspaceId::new();
    let session_id = SessionId::new();
    let turn_task_id = TurnTaskId::new();
    let agent_run_id = AgentRunId::new();
    let model_profile_id = ModelProfileId::new();
    let provider_id = ProviderId::new();
    let model_step_id = ModelStepId::new();
    let current_user_event_id = EventId::new();
    let created_at = UtcTimestamp::now();
    let module_content = "Aura P06 performance fixture core rules.\n";
    let module_hash = ContentHash::sha256(module_content.as_bytes());
    let module_id = "core.performance-fixture".to_owned();
    let prompt_bundle_snapshot = PromptBundleSnapshot {
        id: PromptBundleSnapshotId::new(),
        turn_task_id,
        bundle_version: CanonicalSemVer::new("1.0.0")
            .map_err(|_| ContextFixtureError::PreparationFailed)?,
        serializer_version: 1,
        module_snapshots: vec![PromptModuleSnapshot {
            module_id: module_id.clone(),
            module_version: CanonicalSemVer::new("1.0.0")
                .map_err(|_| ContextFixtureError::PreparationFailed)?,
            section: PromptSectionClass::Core,
            activation: PromptActivationCondition::Always,
            order: 1,
            content_hash: module_hash.clone(),
            content_artifact_id: ArtifactId::new(),
        }],
        custom_addendum_artifact_id: None,
        custom_addendum_hash: None,
        settings_revision: Revision::INITIAL,
        bundle_manifest_hash: ContentHash::sha256(b"p06-performance-fixture-manifest"),
        release_identity: "p06-performance-fixture".to_owned(),
        created_at,
    };
    let mut invocation_identity =
        InvocationIdentity::runtime_user(workspace_id, session_id, CorrelationId::new());
    invocation_identity.turn_task_id = Some(turn_task_id);
    invocation_identity.agent_run_id = Some(agent_run_id);

    Ok(ContextPreparationInput {
        request: ContextPreparationRequest {
            invocation_identity,
            session_id,
            turn_task_id,
            agent_run_id,
            model_step_id,
            model_profile_id,
            runtime_effective_window_tokens,
            output_request: ModelOutputRequest {
                format: ModelOutputFormat::Text,
                effective_max_output_tokens: 8_192,
            },
            history_cutoff: SessionSequence::new(1),
        },
        prompt_bundle_snapshot,
        active_prompt_module_ids: vec![module_id.clone()],
        section_inputs: vec![
            ContextSectionInput {
                kind: ContextSectionKind::CoreSystemPrompt,
                fragments: vec![ContextFragment {
                    source_ref: ContextSourceRef::PromptModule {
                        module_id,
                        content_hash: module_hash,
                    },
                    source_kind: ModelEgressSourceKind::AuraCore,
                    data_class: NetworkDataClass::PublicContent,
                    authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
                    content: module_content.to_owned(),
                }],
                compressibility: ContextCompressibility::NeverCompress,
                privacy_class: ContextPrivacyClass::ModelVisible,
            },
            ContextSectionInput {
                kind: ContextSectionKind::CurrentTurnWorkingSet,
                fragments: vec![ContextFragment {
                    source_ref: ContextSourceRef::Event { event_id: current_user_event_id },
                    source_kind: ModelEgressSourceKind::UserEvent,
                    data_class: NetworkDataClass::PublicQuery,
                    authorization_basis: ModelEgressAuthorizationBasis::UserSubmission,
                    content: "x".repeat(current_user_utf8_bytes),
                }],
                compressibility: ContextCompressibility::NeverCompress,
                privacy_class: ContextPrivacyClass::ModelVisible,
            },
        ],
        recent_history_items: Vec::new(),
        trigger: ContextPreparationTriggerV1::UserSubmission {
            user_event_id: current_user_event_id,
        },
        tool_continuation_facts: None,
        core_continuation_facts: None,
        tool_schema_set: crate::empty_p07_context_tool_schema_set()?,
        effective_tool_bindings: Vec::new(),
        tools: Vec::new(),
        attachments: Vec::new(),
        attachment_egress_sources: Vec::new(),
        model_estimator_key: ModelEstimatorKey {
            provider_id,
            provider_model_name: "p06-performance-fixture-model".to_owned(),
        },
        provider_target: ContextProviderTargetV1 {
            provider_id,
            api_format: ModelApiFormat::OpenAiResponses,
            endpoint_id: EndpointId::new(),
            model_profile_id,
            endpoint_fingerprint: ContentHash::sha256(b"p06-performance-endpoint"),
        },
        inspector: PromptInspectorInput::default(),
        previous_snapshot: None,
        previous_measurement: None,
        created_at,
        compacted_source_proofs: Vec::new(),
        coverage_hops: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use aura_context_runtime::{
        ContextEstimatorRegistry, ContextPreparationCompilationOutcome, ContextPreparationCompiler,
    };

    use super::*;

    #[test]
    fn performance_fixture_exercises_utf8_byte_upper_bound_without_provider_io() {
        let input = p06_performance_preparation_input(262_144, 180_000).expect("fixture");
        let estimators = ContextEstimatorRegistry::default();
        let outcome = ContextPreparationCompiler::new(&estimators).compile(input).expect("compile");
        let ContextPreparationCompilationOutcome::Prepared(prepared) = outcome else {
            panic!("performance fixture must fit its request input budget");
        };
        assert!(prepared.context.snapshot.budget.item_estimates.iter().all(|item| {
            item.estimator.estimator_id == aura_context_domain::UTF8_BYTE_UPPER_BOUND_ESTIMATOR_ID
                && item.estimator.exactness
                    == aura_context_domain::TokenEstimateExactness::UpperBound
        }));
        assert!(prepared.context.snapshot.budget.estimated_input_tokens >= 180_000);
    }
}
