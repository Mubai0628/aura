#![forbid(unsafe_code)]
use std::collections::BTreeMap;

use aura_domain_common::ModelContentBlockId;
use aura_model_domain::{
    LogicalConversationItem, ModelApiFormat, ModelExecutionSnapshot, ModelIncompleteReason,
    ModelOutputFormat, ModelRuntimeCapabilityFacts, ModelStreamEvent, NormalizedModelFailure,
    NormalizedModelOutput, NormalizedStopReason, PartialModelUsage, ProviderProtocolReference,
    ProviderRequestHeaderClass, ProviderToolCallId,
};
use aura_model_transport::{
    MODEL_GENERATION_PROBE_MAX_OUTPUT_TOKENS, MODEL_GENERATION_PROBE_PROMPT, ModelAdapterError,
    ModelAvailabilityProbeCompilationInput, ModelGenerationProbeProtocol, ModelProtocolCompiler,
    ModelProtocolDecoder, ModelRequestCompilationInput, ModelStreamDecoder,
    NonSecretProviderHeader, NormalizedOutputBuilder, ProviderContinuationArtifact,
    ProviderContinuationArtifactKind, ProviderErrorFields, ProviderErrorResponse,
    ProviderRequestPlan, ProviderStreamFrame, build_generation_probe_request_plan,
    build_provider_request_plan, historical_tool_call_wire_bindings, provider_http_failure,
    provider_stream_failure, validate_generation_probe_input, validate_model_compilation_input,
};
use serde_json::{Map, Value, json};
mod history;

pub struct OpenAiResponsesAdapter;

mod decoder;
mod errors;
mod probe;
mod request;
use request::common_openai_headers;
pub use request::replayed_task_states;
mod wire;
use wire::*;
