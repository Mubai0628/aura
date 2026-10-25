use aura_domain_common::{
    ArtifactId, ContentHash, ContextProjectionSnapshotId, CredentialId, CredentialRevision,
    EndpointId, ModelExecutionSnapshotId, ModelFailureId, ModelProfileId, ModelStepId,
    PromptBundleSnapshotId, ProviderId, TransportAttemptId, UtcTimestamp,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    ChatInstructionRole, ChatOutputTokenField, ModelApiFormat, NormalizedModelFailure,
    ProviderRequestEvidence, ProviderToolCallWireBinding,
};
use crate::{CredentialBindingFingerprint, EndpointAuthScheme};

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedModelRequestOptions {
    pub effective_max_output_tokens: u64,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    /// The thinking level sent with the request, when the profile names one the model
    /// offers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<crate::ReasoningEffort>,
    /// The model takes its level as an effort (its catalog lists effort levels), not as
    /// a thinking budget; how the Anthropic format sends the level.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reasoning_by_effort: bool,
    /// The model is sent images, such as the browser's screenshots.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub image_input: bool,
}

impl ResolvedModelRequestOptions {
    /// Options that only cap the output; every other choice is left to the provider.
    pub const fn output_only(effective_max_output_tokens: u64) -> Self {
        Self {
            effective_max_output_tokens,
            temperature: None,
            top_p: None,
            reasoning_effort: None,
            reasoning_by_effort: false,
            image_input: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelExecutionSnapshot {
    pub id: ModelExecutionSnapshotId,
    pub model_step_id: ModelStepId,
    pub provider_id: ProviderId,
    pub endpoint_id: EndpointId,
    pub model_profile_id: ModelProfileId,
    pub provider_display_name: String,
    pub endpoint_display_name: String,
    pub model_name: String,
    pub readable_ref: String,
    pub api_format: ModelApiFormat,
    pub auth_scheme: EndpointAuthScheme,
    pub base_url: String,
    pub resolved_request_url: String,
    pub openai_organization_id: Option<String>,
    pub openai_project_id: Option<String>,
    pub anthropic_workspace_id: Option<String>,
    pub chat_output_token_field: Option<ChatOutputTokenField>,
    pub chat_instruction_role: Option<ChatInstructionRole>,
    pub endpoint_config_revision: u64,
    pub model_config_revision: u64,
    pub runtime_effective_window_tokens: u64,
    pub historical_snapshot_max_tokens: u64,
    pub request_options: ResolvedModelRequestOptions,
    pub historical_tool_call_wire_bindings: Vec<ProviderToolCallWireBinding>,
    pub credential_id: Option<CredentialId>,
    pub credential_revision: Option<CredentialRevision>,
    pub credential_binding_fingerprint: Option<CredentialBindingFingerprint>,
    pub context_snapshot_id: ContextProjectionSnapshotId,
    pub prompt_bundle_snapshot_id: PromptBundleSnapshotId,
    pub endpoint_fingerprint: ContentHash,
    pub created_at: UtcTimestamp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportAttemptKind {
    Initial,
    TransparentRetry,
    CompatibilityAdjustedRetry,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportAttemptState {
    QueuedForEndpoint,
    AcquiringPermit,
    PreparingRequest,
    AuthorizingNetwork,
    Authorized,
    DispatchPrepared,
    Connecting,
    ResponseStarted,
    ResponseCompleted,
    AwaitingFirstEvent,
    Streaming,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRequestPriority {
    BlockingRecovery,
    InteractiveParent,
    ChildAgent,
    Maintenance,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TransportAttemptRecord {
    pub id: TransportAttemptId,
    pub attempt_number: u32,
    pub attempt_kind: TransportAttemptKind,
    pub client_request_id: String,
    pub provider_request_id: Option<String>,
    pub endpoint_id: EndpointId,
    pub endpoint_fingerprint: ContentHash,
    pub request_evidence: ProviderRequestEvidence,
    pub state: TransportAttemptState,
    pub started_at: UtcTimestamp,
    pub first_byte_at: Option<UtcTimestamp>,
    pub first_model_event_at: Option<UtcTimestamp>,
    pub finished_at: Option<UtcTimestamp>,
    pub raw_stream_artifact_id: Option<ArtifactId>,
    pub failure_id: Option<ModelFailureId>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PersistedModelFailure {
    pub id: ModelFailureId,
    pub fingerprint: FailureFingerprint,
    pub failure: NormalizedModelFailure,
    pub created_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(transparent)]
pub struct FailureFingerprint(pub ContentHash);
