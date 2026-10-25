use aura_context_domain::{
    ContextCompressibility, ContextPreparationRequest, ContextPreparationTriggerV1,
    ContextPrivacyClass, ContextSectionKind, ContextSourceRef, ModelEgressAuthorizationBasis,
    ModelEgressSourceKind, PromptActivationCondition, PromptBundleSnapshot, PromptModuleSnapshot,
    PromptSectionClass,
};
use aura_context_runtime::{
    ContextEstimatorRegistry, ContextFragment, ContextPreparationCompilationOutcome,
    ContextPreparationCompiler, ContextPreparationInput, ContextProviderTargetV1,
    ContextSectionInput, ContextToolInput, ModelEstimatorKey, PreparedContextDraft,
    PromptInspectorInput, ToolContinuationFactsV1,
};
use aura_domain_common::CanonicalSemVer;
use aura_domain_common::{
    AgentRunId, ArtifactId, ContentHash, CorrelationId, EndpointId, EventId,
    ModelExecutionSnapshotId, ModelProfileId, ModelStepId, PromptBundleSnapshotId, ProviderId,
    Revision, SessionId, SessionSequence, TurnTaskId, UtcTimestamp, WorkspaceId,
};
use aura_model_domain::{
    EndpointAuthScheme, LogicalConversationItem, ModelApiFormat, ModelExecutionSnapshot,
    ModelOutputFormat, ModelOutputRequest, ResolvedModelRequestOptions,
};
use aura_security_contract::{InvocationIdentity, NetworkDataClass};
use aura_tool_contract::{EffectiveToolBindingSnapshot, ToolSchemaSetSnapshotV1};

use crate::ContextFixtureError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixtureArtifact {
    pub artifact_id: ArtifactId,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct P06PreparedFixture {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub turn_task_id: TurnTaskId,
    pub agent_run_id: AgentRunId,
    pub current_user_event_id: EventId,
    pub current_user_content: String,
    pub prompt_bundle_snapshot: PromptBundleSnapshot,
    pub prompt_bundle_artifacts: Vec<FixtureArtifact>,
    pub prepared: PreparedContextDraft,
    pub execution_snapshot: ModelExecutionSnapshot,
}

#[derive(Clone, Debug)]
pub struct P06ToolContextFixtureInputV1 {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub turn_task_id: TurnTaskId,
    pub agent_run_id: AgentRunId,
    pub model_step_id: ModelStepId,
    pub model_profile_id: ModelProfileId,
    pub provider_id: ProviderId,
    pub endpoint_id: EndpointId,
    pub tool_schema_set: ToolSchemaSetSnapshotV1,
    pub effective_tool_bindings: Vec<EffectiveToolBindingSnapshot>,
}

pub fn p06_prepared_context_fixture() -> Result<P06PreparedFixture, ContextFixtureError> {
    let workspace_id = WorkspaceId::new();
    let session_id = SessionId::new();
    let turn_task_id = TurnTaskId::new();
    let agent_run_id = AgentRunId::new();
    let model_profile_id = ModelProfileId::new();
    let provider_id = ProviderId::new();
    let endpoint_id = EndpointId::new();
    let created_at = UtcTimestamp::now();
    let module_content = "Aura fixture core system rules.\n";
    let module_artifact_id = ArtifactId::new();
    let prompt_bundle_snapshot = PromptBundleSnapshot {
        id: PromptBundleSnapshotId::new(),
        turn_task_id,
        bundle_version: CanonicalSemVer::new("1.0.0")
            .map_err(|_| ContextFixtureError::PreparationFailed)?,
        serializer_version: 1,
        module_snapshots: vec![PromptModuleSnapshot {
            module_id: "core.fixture".to_owned(),
            module_version: CanonicalSemVer::new("1.0.0")
                .map_err(|_| ContextFixtureError::PreparationFailed)?,
            section: PromptSectionClass::Core,
            activation: PromptActivationCondition::Always,
            order: 1,
            content_hash: ContentHash::sha256(module_content.as_bytes()),
            content_artifact_id: module_artifact_id,
        }],
        custom_addendum_artifact_id: None,
        custom_addendum_hash: None,
        settings_revision: Revision::INITIAL,
        bundle_manifest_hash: ContentHash::sha256(b"p06-fixture-manifest"),
        release_identity: "p06-fixture-release".to_owned(),
        created_at,
    };
    build_prepared_fixture(
        P06FixtureScope {
            workspace_id,
            session_id,
            turn_task_id,
            agent_run_id,
            model_step_id: ModelStepId::new(),
            model_profile_id,
            provider_id,
            endpoint_id,
        },
        prompt_bundle_snapshot,
        vec![FixtureArtifact {
            artifact_id: module_artifact_id,
            media_type: "text/markdown; charset=utf-8".to_owned(),
            bytes: module_content.as_bytes().to_vec(),
        }],
        crate::empty_p07_context_tool_schema_set()?,
        Vec::new(),
    )
}

pub fn p06_prepared_tool_context_fixture(
    input: P06ToolContextFixtureInputV1,
) -> Result<P06PreparedFixture, ContextFixtureError> {
    let created_at = UtcTimestamp::now();
    let module_content = "Aura fixture core system rules.\n";
    let module_artifact_id = ArtifactId::new();
    let prompt_bundle_snapshot = PromptBundleSnapshot {
        id: PromptBundleSnapshotId::new(),
        turn_task_id: input.turn_task_id,
        bundle_version: CanonicalSemVer::new("1.0.0")
            .map_err(|_| ContextFixtureError::PreparationFailed)?,
        serializer_version: 1,
        module_snapshots: vec![PromptModuleSnapshot {
            module_id: "core.fixture".to_owned(),
            module_version: CanonicalSemVer::new("1.0.0")
                .map_err(|_| ContextFixtureError::PreparationFailed)?,
            section: PromptSectionClass::Core,
            activation: PromptActivationCondition::Always,
            order: 1,
            content_hash: ContentHash::sha256(module_content.as_bytes()),
            content_artifact_id: module_artifact_id,
        }],
        custom_addendum_artifact_id: None,
        custom_addendum_hash: None,
        settings_revision: Revision::INITIAL,
        bundle_manifest_hash: ContentHash::sha256(b"p06-fixture-manifest"),
        release_identity: "p06-fixture-release".to_owned(),
        created_at,
    };
    build_prepared_fixture(
        P06FixtureScope {
            workspace_id: input.workspace_id,
            session_id: input.session_id,
            turn_task_id: input.turn_task_id,
            agent_run_id: input.agent_run_id,
            model_step_id: input.model_step_id,
            model_profile_id: input.model_profile_id,
            provider_id: input.provider_id,
            endpoint_id: input.endpoint_id,
        },
        prompt_bundle_snapshot,
        vec![FixtureArtifact {
            artifact_id: module_artifact_id,
            media_type: "text/markdown; charset=utf-8".to_owned(),
            bytes: module_content.as_bytes().to_vec(),
        }],
        input.tool_schema_set,
        input.effective_tool_bindings,
    )
}

/// Builds another immutable Context snapshot in the exact same TurnTask scope.
///
/// The helper deliberately reuses the frozen Prompt Bundle Snapshot and model
/// ownership identities. It exists for P06 persistence/retry fixtures where a
/// new ModelStep/CPS/MEX must not accidentally create a second PBS for one
/// TurnTask.
pub fn p06_followup_prepared_context_fixture(
    base: &P06PreparedFixture,
) -> Result<P06PreparedFixture, ContextFixtureError> {
    build_prepared_fixture(
        P06FixtureScope {
            workspace_id: base.workspace_id,
            session_id: base.session_id,
            turn_task_id: base.turn_task_id,
            agent_run_id: base.agent_run_id,
            model_step_id: ModelStepId::new(),
            model_profile_id: base.execution_snapshot.model_profile_id,
            provider_id: base.execution_snapshot.provider_id,
            endpoint_id: base.execution_snapshot.endpoint_id,
        },
        base.prompt_bundle_snapshot.clone(),
        base.prompt_bundle_artifacts.clone(),
        base.prepared.context.tool_schema_set.clone(),
        base.prepared.context.snapshot.effective_tool_bindings.clone(),
    )
}

/// Compiles the next P06 snapshot from the exact durable history authorized by
/// The originating User event is reused as history; no new User event is
/// created or synthesized by this helper.
pub fn p06_prepared_tool_continuation_fixture(
    base: &P06PreparedFixture,
    next_model_step_id: ModelStepId,
    trigger: ContextPreparationTriggerV1,
    facts: ToolContinuationFactsV1,
    recent_history_items: Vec<LogicalConversationItem>,
) -> Result<P06PreparedFixture, ContextFixtureError> {
    let ContextPreparationTriggerV1::ToolContinuation { originating_user_event_id, .. } = &trigger
    else {
        return Err(ContextFixtureError::PreparationFailed);
    };
    let current_user_event_id = *originating_user_event_id;
    let current_user_content = recent_history_items
        .iter()
        .find_map(|item| match item {
            LogicalConversationItem::User { source_event_id, content, .. }
                if *source_event_id == current_user_event_id =>
            {
                Some(content.clone())
            }
            _ => None,
        })
        .ok_or(ContextFixtureError::PreparationFailed)?;
    let module = base
        .prompt_bundle_snapshot
        .module_snapshots
        .first()
        .ok_or(ContextFixtureError::PreparationFailed)?;
    let module_content = base
        .prompt_bundle_artifacts
        .iter()
        .find(|artifact| artifact.artifact_id == module.content_artifact_id)
        .and_then(|artifact| std::str::from_utf8(&artifact.bytes).ok())
        .ok_or(ContextFixtureError::PreparationFailed)?;
    let mut invocation_identity =
        InvocationIdentity::runtime_user(base.workspace_id, base.session_id, CorrelationId::new());
    invocation_identity.turn_task_id = Some(base.turn_task_id);
    invocation_identity.agent_run_id = Some(base.agent_run_id);
    let section_inputs = vec![
        ContextSectionInput {
            kind: ContextSectionKind::CoreSystemPrompt,
            fragments: vec![ContextFragment {
                source_ref: ContextSourceRef::PromptModule {
                    module_id: module.module_id.clone(),
                    content_hash: module.content_hash.clone(),
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
            kind: ContextSectionKind::RecentHistory,
            fragments: recent_history_items
                .iter()
                .map(history_fragment)
                .collect::<Result<Vec<_>, _>>()?,
            compressibility: ContextCompressibility::CompressibleHistory,
            privacy_class: ContextPrivacyClass::ModelVisible,
        },
    ];
    let tool_schema_set = base.prepared.context.tool_schema_set.clone();
    let effective_tool_bindings = base.prepared.context.snapshot.effective_tool_bindings.clone();
    let output_request = base.prepared.context.output_request.clone();
    let created_at = UtcTimestamp::now();
    let preparation_input = ContextPreparationInput {
        request: ContextPreparationRequest {
            invocation_identity,
            session_id: base.session_id,
            turn_task_id: base.turn_task_id,
            agent_run_id: base.agent_run_id,
            model_step_id: next_model_step_id,
            model_profile_id: base.execution_snapshot.model_profile_id,
            runtime_effective_window_tokens: base
                .execution_snapshot
                .runtime_effective_window_tokens,
            output_request: output_request.clone(),
            history_cutoff: SessionSequence::new(2),
        },
        prompt_bundle_snapshot: base.prompt_bundle_snapshot.clone(),
        active_prompt_module_ids: vec!["core.fixture".to_owned()],
        section_inputs,
        recent_history_items,
        trigger,
        tool_continuation_facts: Some(facts),
        core_continuation_facts: None,
        tools: tool_schema_set
            .tools
            .iter()
            .map(|tool| ContextToolInput {
                tool_id: tool.tool_id,
                compressibility: ContextCompressibility::NeverCompress,
            })
            .collect(),
        tool_schema_set,
        effective_tool_bindings,
        attachments: Vec::new(),
        attachment_egress_sources: Vec::new(),
        model_estimator_key: ModelEstimatorKey {
            provider_id: base.execution_snapshot.provider_id,
            provider_model_name: base.execution_snapshot.model_name.clone(),
        },
        provider_target: ContextProviderTargetV1 {
            provider_id: base.execution_snapshot.provider_id,
            api_format: base.execution_snapshot.api_format,
            endpoint_id: base.execution_snapshot.endpoint_id,
            model_profile_id: base.execution_snapshot.model_profile_id,
            endpoint_fingerprint: base.execution_snapshot.endpoint_fingerprint.clone(),
        },
        inspector: PromptInspectorInput::default(),
        previous_snapshot: Some(base.prepared.context.snapshot.clone()),
        previous_measurement: None,
        created_at,
        compacted_source_proofs: Vec::new(),
        coverage_hops: Vec::new(),
    };
    let estimators = ContextEstimatorRegistry::default();
    let compiler = ContextPreparationCompiler::new(&estimators);
    let ContextPreparationCompilationOutcome::Prepared(prepared) =
        compiler.compile(preparation_input).map_err(|_| ContextFixtureError::PreparationFailed)?
    else {
        return Err(ContextFixtureError::PreparationDidNotProduceSnapshot);
    };
    let mut execution_snapshot = base.execution_snapshot.clone();
    execution_snapshot.id = ModelExecutionSnapshotId::new();
    execution_snapshot.model_step_id = next_model_step_id;
    execution_snapshot.context_snapshot_id = prepared.context.snapshot.id;
    execution_snapshot.created_at = created_at;
    Ok(P06PreparedFixture {
        workspace_id: base.workspace_id,
        session_id: base.session_id,
        turn_task_id: base.turn_task_id,
        agent_run_id: base.agent_run_id,
        current_user_event_id,
        current_user_content,
        prompt_bundle_snapshot: base.prompt_bundle_snapshot.clone(),
        prompt_bundle_artifacts: base.prompt_bundle_artifacts.clone(),
        prepared: *prepared,
        execution_snapshot,
    })
}

fn history_fragment(
    item: &LogicalConversationItem,
) -> Result<ContextFragment, ContextFixtureError> {
    let (event_id, source_kind, data_class, authorization_basis, content) = match item {
        LogicalConversationItem::User { source_event_id, content, .. } => (
            *source_event_id,
            ModelEgressSourceKind::UserEvent,
            NetworkDataClass::PublicQuery,
            ModelEgressAuthorizationBasis::UserSubmission,
            content.clone(),
        ),
        LogicalConversationItem::Assistant { source_event_id, content, tool_calls, .. } => (
            *source_event_id,
            ModelEgressSourceKind::AssistantEvent,
            NetworkDataClass::PublicContent,
            ModelEgressAuthorizationBasis::ModelChannelPolicy,
            content.clone().unwrap_or_else(|| format!("{tool_calls:?}")),
        ),
        LogicalConversationItem::ToolResult { source_event_id, content, .. } => (
            *source_event_id,
            ModelEgressSourceKind::ToolResult,
            NetworkDataClass::PublicContent,
            ModelEgressAuthorizationBasis::ModelChannelPolicy,
            content.clone(),
        ),
    };
    Ok(ContextFragment {
        source_ref: ContextSourceRef::Event { event_id },
        source_kind,
        data_class,
        authorization_basis,
        content,
    })
}

#[derive(Clone, Copy)]
struct P06FixtureScope {
    workspace_id: WorkspaceId,
    session_id: SessionId,
    turn_task_id: TurnTaskId,
    agent_run_id: AgentRunId,
    model_step_id: ModelStepId,
    model_profile_id: ModelProfileId,
    provider_id: ProviderId,
    endpoint_id: EndpointId,
}

fn build_prepared_fixture(
    scope: P06FixtureScope,
    prompt_bundle_snapshot: PromptBundleSnapshot,
    prompt_bundle_artifacts: Vec<FixtureArtifact>,
    tool_schema_set: ToolSchemaSetSnapshotV1,
    effective_tool_bindings: Vec<EffectiveToolBindingSnapshot>,
) -> Result<P06PreparedFixture, ContextFixtureError> {
    let model_step_id = scope.model_step_id;
    let current_user_event_id = EventId::new();
    let created_at = UtcTimestamp::now();
    let module = prompt_bundle_snapshot
        .module_snapshots
        .first()
        .ok_or(ContextFixtureError::PreparationFailed)?;
    let module_content = prompt_bundle_artifacts
        .iter()
        .find(|artifact| artifact.artifact_id == module.content_artifact_id)
        .and_then(|artifact| std::str::from_utf8(&artifact.bytes).ok())
        .ok_or(ContextFixtureError::PreparationFailed)?;
    let mut invocation_identity = InvocationIdentity::runtime_user(
        scope.workspace_id,
        scope.session_id,
        CorrelationId::new(),
    );
    invocation_identity.turn_task_id = Some(scope.turn_task_id);
    invocation_identity.agent_run_id = Some(scope.agent_run_id);
    let current_user_content = "Implement the frozen P06 fixture.".to_owned();
    let output_request =
        ModelOutputRequest { format: ModelOutputFormat::Text, effective_max_output_tokens: 1_024 };
    let preparation_input = ContextPreparationInput {
        request: ContextPreparationRequest {
            invocation_identity,
            session_id: scope.session_id,
            turn_task_id: scope.turn_task_id,
            agent_run_id: scope.agent_run_id,
            model_step_id,
            model_profile_id: scope.model_profile_id,
            runtime_effective_window_tokens: 32_768,
            output_request: output_request.clone(),
            history_cutoff: SessionSequence::new(1),
        },
        prompt_bundle_snapshot: prompt_bundle_snapshot.clone(),
        active_prompt_module_ids: vec!["core.fixture".to_owned()],
        section_inputs: vec![
            ContextSectionInput {
                kind: ContextSectionKind::CoreSystemPrompt,
                fragments: vec![ContextFragment {
                    source_ref: ContextSourceRef::PromptModule {
                        module_id: module.module_id.clone(),
                        content_hash: module.content_hash.clone(),
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
                    content: current_user_content.clone(),
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
        tools: tool_schema_set
            .tools
            .iter()
            .map(|tool| ContextToolInput {
                tool_id: tool.tool_id,
                compressibility: ContextCompressibility::NeverCompress,
            })
            .collect(),
        tool_schema_set,
        effective_tool_bindings,
        attachments: Vec::new(),
        attachment_egress_sources: Vec::new(),
        model_estimator_key: ModelEstimatorKey {
            provider_id: scope.provider_id,
            provider_model_name: "p06-fixture-model".to_owned(),
        },
        provider_target: ContextProviderTargetV1 {
            provider_id: scope.provider_id,
            api_format: ModelApiFormat::OpenAiResponses,
            endpoint_id: scope.endpoint_id,
            model_profile_id: scope.model_profile_id,
            endpoint_fingerprint: ContentHash::blake3(b"p06-fixture-endpoint"),
        },
        inspector: PromptInspectorInput::default(),
        previous_snapshot: None,
        previous_measurement: None,
        created_at,
        compacted_source_proofs: Vec::new(),
        coverage_hops: Vec::new(),
    };
    let estimators = ContextEstimatorRegistry::default();
    let compiler = ContextPreparationCompiler::new(&estimators);
    let ContextPreparationCompilationOutcome::Prepared(prepared) =
        compiler.compile(preparation_input).map_err(|_| ContextFixtureError::PreparationFailed)?
    else {
        return Err(ContextFixtureError::PreparationDidNotProduceSnapshot);
    };
    let execution_snapshot = ModelExecutionSnapshot {
        id: ModelExecutionSnapshotId::new(),
        model_step_id,
        provider_id: scope.provider_id,
        endpoint_id: scope.endpoint_id,
        model_profile_id: scope.model_profile_id,
        provider_display_name: "P06 Fixture Provider".to_owned(),
        endpoint_display_name: "P06 Fixture Endpoint".to_owned(),
        model_name: "p06-fixture-model".to_owned(),
        readable_ref: "P06 Fixture Provider / P06 Fixture Endpoint / p06-fixture-model".to_owned(),
        api_format: ModelApiFormat::OpenAiResponses,
        auth_scheme: EndpointAuthScheme::None,
        base_url: "https://api.example.test/v1".to_owned(),
        resolved_request_url: "https://api.example.test/v1/responses".to_owned(),
        openai_organization_id: None,
        openai_project_id: None,
        anthropic_workspace_id: None,
        chat_output_token_field: None,
        chat_instruction_role: None,
        endpoint_config_revision: 1,
        model_config_revision: 1,
        runtime_effective_window_tokens: 32_768,
        historical_snapshot_max_tokens: 1_024,
        request_options: ResolvedModelRequestOptions {
            effective_max_output_tokens: output_request.effective_max_output_tokens,
            temperature: None,
            top_p: None,
            reasoning_effort: None,
            reasoning_by_effort: false,
            image_input: false,
        },
        historical_tool_call_wire_bindings: Vec::new(),
        credential_id: None,
        credential_revision: None,
        credential_binding_fingerprint: None,
        context_snapshot_id: prepared.context.snapshot.id,
        prompt_bundle_snapshot_id: prompt_bundle_snapshot.id,
        endpoint_fingerprint: ContentHash::blake3(b"p06-fixture-endpoint"),
        created_at,
    };
    Ok(P06PreparedFixture {
        workspace_id: scope.workspace_id,
        session_id: scope.session_id,
        turn_task_id: scope.turn_task_id,
        agent_run_id: scope.agent_run_id,
        current_user_event_id,
        current_user_content,
        prompt_bundle_snapshot,
        prompt_bundle_artifacts,
        prepared: *prepared,
        execution_snapshot,
    })
}

#[cfg(test)]
mod tests {
    use aura_context_runtime::{
        ContextWindowRebuildError, ContextWindowResolutionDirective,
        resolve_context_window_resolution,
    };
    use aura_domain_common::{ArtifactId, ModelExecutionSnapshotId};
    use aura_model_domain::ContextWindowResolution;

    use super::p06_prepared_context_fixture;

    #[test]
    fn p05_context_window_resolution_is_bound_to_failed_mex_before_rebuild() {
        let fixture = p06_prepared_context_fixture().expect("P06 fixture");
        let provider_error_ref = ArtifactId::new();
        let resolution = ContextWindowResolution::P06RebuildRequired {
            failed_execution_snapshot_id: fixture.execution_snapshot.id,
            model_profile_id: fixture.execution_snapshot.model_profile_id,
            provider_error_ref,
            local_estimated_input_tokens: 31_000,
            current_effective_window_tokens: 32_768,
        };
        let ContextWindowResolutionDirective::Rebuild(directive) =
            resolve_context_window_resolution(&fixture.execution_snapshot, &resolution)
                .expect("resolve P05 context-window result")
        else {
            panic!("P06 rebuild resolution must produce a rebuild directive");
        };
        assert_eq!(directive.failed_execution_snapshot_id, fixture.execution_snapshot.id);
        assert_eq!(directive.failed_context_snapshot_id, fixture.prepared.context.snapshot.id);
        assert_eq!(directive.failed_model_step_id, fixture.execution_snapshot.model_step_id);
        assert_eq!(directive.runtime_effective_window_tokens, 32_768);
        assert_eq!(directive.provider_error_ref, provider_error_ref);

        let forged = ContextWindowResolution::P06RebuildRequired {
            failed_execution_snapshot_id: ModelExecutionSnapshotId::new(),
            model_profile_id: fixture.execution_snapshot.model_profile_id,
            provider_error_ref,
            local_estimated_input_tokens: 31_000,
            current_effective_window_tokens: 32_768,
        };
        assert_eq!(
            resolve_context_window_resolution(&fixture.execution_snapshot, &forged)
                .expect_err("forged failed MEX must fail closed"),
            ContextWindowRebuildError::FailedExecutionMismatch
        );
    }
}
