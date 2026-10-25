use aura_domain_common::ToolCallId;
use aura_domain_common::{
    ArtifactId, ModelContentBlockId, ModelStepId, TransportAttemptId, UtcTimestamp,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ProviderToolCallId;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderProtocolReference {
    pub item_id: Option<String>,
    pub block_id: Option<String>,
    pub index: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantContentBlock {
    pub block_id: ModelContentBlockId,
    pub text: String,
    pub provider_reference: Option<ProviderProtocolReference>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReasoningContentBlock {
    pub block_id: ModelContentBlockId,
    pub text: String,
    pub summary: Option<String>,
    pub provider_reference: Option<ProviderProtocolReference>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedToolCall {
    pub tool_call_id: ToolCallId,
    pub provider_call_id: ProviderToolCallId,
    pub tool_name: String,
    pub arguments_json: String,
    pub position: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRefusal {
    pub message: String,
    pub provider_code: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PartialModelUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    ProviderReported,
    LocalExactTokenizer,
    LocalEstimated,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub reasoning_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub source: UsageSource,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalizedStopReason {
    EndTurn,
    ToolCalls,
    MaxOutputTokens,
    StopSequence,
    Refusal,
    ContentFilter,
    Cancelled,
    ProviderIncomplete,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelIncompleteReason {
    MaxOutputTokens,
    ContentFilter,
    StreamInterrupted,
    ProviderReported { code: String },
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFailureCategory {
    Authentication,
    Authorization,
    RateLimit,
    QuotaOrBalance,
    InvalidRequest,
    UnsupportedParameter,
    UnsupportedModel,
    ContextWindowExceeded,
    Transport,
    StreamInterrupted,
    Protocol,
    ContentFilter,
    Refusal,
    EmptyOutput,
    InvalidToolCall,
    Persistence,
    Cancelled,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelRetryClass {
    NotRetryable,
    TransparentRetry { retry_after_ms: Option<u64> },
    CompatibilityAdjustment { parameter: String },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedModelFailure {
    pub category: ModelFailureCategory,
    pub code: String,
    pub message: String,
    pub retry_class: ModelRetryClass,
    pub provider_status: Option<u16>,
    pub provider_request_id: Option<String>,
    pub provider_error_ref: Option<ArtifactId>,
    pub partial_output_ref: Option<ArtifactId>,
    pub transport_attempt_id: Option<TransportAttemptId>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderContinuationState {
    OpenAiResponses { state_ref: ArtifactId },
    Anthropic { state_ref: ArtifactId },
    Opaque { provider_kind: String, state_ref: ArtifactId },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRuntimeCapabilityFacts {
    pub api_format: crate::ModelApiFormat,
    pub native_tool_channel_exposed: bool,
    pub native_attachment_parts_present: bool,
    pub reasoning_channel_observed: bool,
    pub native_capability_names: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelStreamEvent {
    RequestAccepted { provider_request_id: Option<String> },
    ResponseStarted,
    AssistantTextStarted { block_id: ModelContentBlockId },
    AssistantTextDelta { block_id: ModelContentBlockId, text: String },
    ReasoningStarted { block_id: ModelContentBlockId },
    ReasoningDelta { block_id: ModelContentBlockId, text: String },
    ReasoningSummaryDelta { block_id: ModelContentBlockId, text: String },
    RefusalDelta { text: String },
    ToolCallStarted { provider_call_id: ProviderToolCallId, tool_name: String, position: u32 },
    ToolCallArgumentsDelta { provider_call_id: ProviderToolCallId, delta: String },
    ToolCallCompleted { provider_call_id: ProviderToolCallId },
    NativeCapabilityEvent { capability_name: String, payload_ref: ArtifactId },
    UsageUpdated { usage: PartialModelUsage },
    ProviderWarning { code: Option<String>, message: String },
    ResponseCompleted { stop_reason: NormalizedStopReason },
    ResponseIncomplete { reason: ModelIncompleteReason },
    StreamError { failure: NormalizedModelFailure },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelStreamEventEnvelope {
    pub model_step_id: ModelStepId,
    pub transport_attempt_id: TransportAttemptId,
    pub sequence: u64,
    pub received_at: UtcTimestamp,
    pub event: ModelStreamEvent,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedModelOutput {
    pub model_step_id: ModelStepId,
    pub assistant_blocks: Vec<AssistantContentBlock>,
    pub reasoning_blocks: Vec<ReasoningContentBlock>,
    pub tool_calls: Vec<NormalizedToolCall>,
    pub refusal: Option<ModelRefusal>,
    pub stop_reason: NormalizedStopReason,
    pub usage: ModelUsage,
    pub provider_continuation: Option<ProviderContinuationState>,
    pub provider_request_id: Option<String>,
    pub raw_response_ref: Option<ArtifactId>,
}
