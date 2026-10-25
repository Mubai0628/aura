#![forbid(unsafe_code)]
use std::collections::BTreeMap;

use aura_domain_common::ModelContentBlockId;
use aura_model_domain::{
    LogicalConversationItem, ModelApiFormat, ModelExecutionSnapshot, ModelFailureCategory,
    ModelIncompleteReason, ModelOutputFormat, ModelRetryClass, ModelRuntimeCapabilityFacts,
    ModelStreamEvent, NormalizedModelFailure, NormalizedModelOutput, NormalizedStopReason,
    PartialModelUsage, ProviderProtocolReference, ProviderRequestHeaderClass, ProviderToolCallId,
};
use aura_model_transport::{
    MODEL_GENERATION_PROBE_MAX_OUTPUT_TOKENS, MODEL_GENERATION_PROBE_PROMPT, ModelAdapterError,
    ModelAvailabilityProbeCompilationInput, ModelGenerationProbeProtocol, ModelProtocolCompiler,
    ModelProtocolDecoder, ModelRequestCompilationInput, ModelStreamDecoder,
    NonSecretProviderHeader, NormalizedOutputBuilder, ProviderContinuationArtifact,
    ProviderContinuationArtifactKind, ProviderErrorResponse, ProviderRequestPlan,
    ProviderStreamFrame, build_generation_probe_request_plan, build_provider_request_plan,
    historical_tool_call_wire_bindings, provider_tool_name_bindings, retry_after_milliseconds,
    validate_generation_probe_input, validate_model_compilation_input,
};
use serde_json::{Map, Value, json};
mod history;

pub struct OpenAiResponsesAdapter;

mod decoder;
mod errors;
mod probe;
mod request;
use errors::classify_http_error;
use request::common_openai_headers;
mod wire;
use wire::*;
