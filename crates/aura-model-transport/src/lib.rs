#![forbid(unsafe_code)]

mod adapter;
mod attempt;
mod cancellation;
mod diagnostic;
mod engine;
mod error;
mod failure;
mod gate;
mod images;
mod network;
mod output;
mod request_layout;
mod retry;
mod sse;
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
pub use images::{ToolResultImage, tool_result_image_label, tool_result_images};
pub use network::{
    CORE_REQUEST_GRANT_TTL_SECONDS, CoreGatewayModelNetwork, ModelNetworkExecutor,
    ModelRequestAuthorizer, TransportHttpBody, TransportHttpResponse,
};
pub use output::NormalizedOutputBuilder;
pub use request_layout::{instruction_layout, openai_prompt_cache_key, ordered_tool_bindings};
pub use retry::{MAX_TRANSPORT_ATTEMPTS, RetryDecision, TransportRetryPolicy};
pub use sse::{ProviderStreamFrame, SseDecoder, SseLimits};
pub use strict_schema::strict_schema_compatible;
