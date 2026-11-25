#![forbid(unsafe_code)]

mod adapter;
mod attempt;
mod cancellation;
mod diagnostic;
mod engine;
mod error;
mod failure;
mod gate;
mod history_archive;
mod images;
mod native_prefix;
mod network;
mod output;
mod prompt_parts;
mod provider_error;
mod provider_routing;
mod reasoning_loop;
mod request_layout;
mod retry;
mod speed_mode;
mod sse;
mod state_log;
mod strict_schema;

pub use adapter::{
    MODEL_GENERATION_PROBE_MAX_OUTPUT_TOKENS, MODEL_GENERATION_PROBE_PROMPT, ModelAdapterError,
    ModelAvailabilityProbeCompilationInput, ModelDiscoveryCompilationInput,
    ModelGenerationProbeProtocol, ModelProtocolCompiler, ModelProtocolDecoder,
    ModelRequestCompilationInput, ModelStreamDecoder, NonSecretProviderHeader,
    PROVIDER_ACCEPT_ENCODING, ProviderContinuationArtifact, ProviderContinuationArtifactKind,
    ProviderErrorResponse, ProviderRequestPlan, build_generation_probe_request_plan,
    build_model_discovery_request_plan, build_provider_request_plan,
    historical_tool_call_wire_bindings, provider_tool_name_bindings, retry_after_milliseconds,
    validate_generation_probe_input, validate_model_compilation_input,
};
pub use cancellation::ModelCancellationToken;
pub use diagnostic::{PROVIDER_TOOL_CALL_BINDING_INVALID, ProviderBindingViolationDiagnostic};
pub use engine::{
    ModelTransport, ModelTransportExecution, ProviderRequestFactory, RawStreamWriter,
    TransportEventChannel, TransportRunTerminal, TransportStore,
};
pub use error::{ModelTransportError, TransportTimeoutKind};
pub use gate::{EndpointGate, EndpointGateMetrics, EndpointGateRegistry, EndpointPermit};
pub use history_archive::{
    archived_deepseek_group, deepseek_history_requires_reasoning, uses_deepseek_history_archive,
};
pub use images::{ToolResultImage, tool_result_image_label, tool_result_images};
pub use native_prefix::{NativeHistoryProjection, NativePrefix};
pub use network::{
    CORE_REQUEST_GRANT_TTL_SECONDS, CoreGatewayModelNetwork, ModelNetworkExecutor,
    ModelRequestAuthorizer, TransportHttpBody, TransportHttpResponse,
};
pub use output::NormalizedOutputBuilder;
pub use prompt_parts::{
    ProviderTools, provider_history_replay, provider_reasoning_content, provider_tool_call,
};
pub use provider_error::{
    ProviderErrorFields, provider_http_failure, provider_reply_account_setting,
    provider_stream_failure,
};
pub use reasoning_loop::REASONING_LOOP_REPEATS;
pub use request_layout::{
    RoutingKeyField, cache_breakpoints, encrypted_reasoning, instruction_envelope_layout,
    instruction_layout, routing_key_field, state_before_conversation, store_off,
    supports_task_state_replay,
};
pub use retry::{MAX_TRANSPORT_ATTEMPTS, RetryDecision, TransportRetryPolicy};
pub use speed_mode::speed_mode_fields;
pub use sse::{ProviderStreamFrame, SseDecoder, SseLimits};
pub use state_log::{StatePlacement, place_state, preview_state};
pub use strict_schema::strict_schema_compatible;
