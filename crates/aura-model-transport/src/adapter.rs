use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, SystemTime},
};

use aura_domain_common::ToolCallId;
use aura_domain_common::{ArtifactId, TransportAttemptId};
use aura_model_domain::{
    ChatOutputTokenFieldAlias, EndpointAuthScheme, LogicalConversationItem, LogicalModelRequest,
    ModelAdapterIdentity, ModelApiFormat, ModelEndpointConfig, ModelExecutionSnapshot,
    ModelFailureCategory, ModelProfileConfig, ModelRuntimeCapabilityFacts, ModelStreamEvent,
    NormalizedModelFailure, NormalizedModelOutput, ProviderRequestEvidence,
    ProviderRequestHeaderClass, ProviderRequestHeaderMetadata, ProviderRequestTargetMetadata,
    ProviderToolCallReuseDomain, ProviderToolCallWireBinding, ProviderToolNameBinding,
    ResolvedModelAttachment, TransportOperationContext, validate_provider_tool_call_wire_bindings,
    validate_resolved_attachments,
};
use aura_permission_domain::NormalizedOrigin;
use thiserror::Error;
use url::Url;

use crate::ProviderStreamFrame;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderRequestPlan {
    pub method: String,
    pub request_url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub safe_optional_parameters: BTreeSet<String>,
    pub request_evidence: ProviderRequestEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NonSecretProviderHeader {
    pub name: String,
    pub value: String,
    pub class: ProviderRequestHeaderClass,
}

#[derive(Clone, Copy)]
pub struct ModelRequestCompilationInput<'a> {
    pub request: &'a LogicalModelRequest,
    pub snapshot: &'a ModelExecutionSnapshot,
    pub resolved_attachments: &'a [ResolvedModelAttachment],
    pub operation_context: &'a TransportOperationContext,
    pub compatibility_removals: &'a BTreeSet<String>,
    pub applied_output_token_field_alias: Option<&'a ChatOutputTokenFieldAlias>,
}

/// Frozen P05 model-availability probe prompt.
pub const MODEL_GENERATION_PROBE_PROMPT: &str = "Respond with a brief plain-text acknowledgement.";

/// Generation probes use their own fixed limit and never borrow the normal
/// ModelExecutionSnapshot output limit.
pub const MODEL_GENERATION_PROBE_MAX_OUTPUT_TOKENS: u64 = 256;

/// Frozen response codings advertised by every P05 Provider request.
pub const PROVIDER_ACCEPT_ENCODING: &str = "gzip, br, zstd";

/// Adapter-only input for the model-generation stage of a model availability test.
///
/// This deliberately does not contain a ModelExecutionSnapshot, mex, cps, or
/// pbs. Those identities belong to Agent model execution and must not be
/// synthesized for configuration operations.
#[derive(Clone, Copy)]
pub struct ModelAvailabilityProbeCompilationInput<'a> {
    pub endpoint: &'a ModelEndpointConfig,
    pub model_profile: &'a ModelProfileConfig,
    pub resolved_request_url: &'a str,
    pub operation_context: &'a TransportOperationContext,
    pub applied_output_token_field_alias: Option<&'a ChatOutputTokenFieldAlias>,
}

/// Adapter-independent input for the strict Provider model-list request.
#[derive(Clone, Copy)]
pub struct ModelDiscoveryCompilationInput<'a> {
    pub endpoint: &'a ModelEndpointConfig,
    pub resolved_request_url: &'a str,
    pub operation_context: &'a TransportOperationContext,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderErrorResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub provider_request_id: Option<String>,
    pub raw_error_artifact_id: Option<ArtifactId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderContinuationArtifactKind {
    OpenAiResponses,
    Anthropic,
    Opaque,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderContinuationArtifact {
    pub kind: ProviderContinuationArtifactKind,
    pub provider_kind: Option<String>,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

/// Parses the HTTP `Retry-After` field as either delay-seconds or HTTP-date.
/// Invalid values are ignored and a date in the past becomes an immediate retry.
#[must_use]
pub fn retry_after_milliseconds(headers: &[(String, String)]) -> Option<u64> {
    retry_after_milliseconds_at(headers, SystemTime::now())
}

#[doc(hidden)]
#[must_use]
pub fn retry_after_milliseconds_at(
    headers: &[(String, String)],
    received_at: SystemTime,
) -> Option<u64> {
    let value = headers.iter().find(|(name, _)| name.eq_ignore_ascii_case("retry-after"))?.1.trim();
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        return value.parse::<u64>().ok().map(|seconds| seconds.saturating_mul(1_000));
    }
    let retry_at = httpdate::parse_http_date(value).ok()?;
    let delay = retry_at.duration_since(received_at).unwrap_or(Duration::ZERO);
    Some(u64::try_from(delay.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::retry_after_milliseconds_at;

    #[test]
    fn parses_both_retry_after_forms_and_ignores_invalid_values() {
        let now = UNIX_EPOCH + Duration::from_secs(784_111_777);
        assert_eq!(
            retry_after_milliseconds_at(&[("Retry-After".to_owned(), "120".to_owned())], now),
            Some(120_000)
        );
        assert_eq!(
            retry_after_milliseconds_at(
                &[("retry-after".to_owned(), "Sun, 06 Nov 1994 08:51:17 GMT".to_owned())],
                now,
            ),
            Some(100_000)
        );
        assert_eq!(
            retry_after_milliseconds_at(
                &[("retry-after".to_owned(), "Sun, 06 Nov 1994 08:48:17 GMT".to_owned())],
                now,
            ),
            Some(0)
        );
        assert_eq!(
            retry_after_milliseconds_at(&[("retry-after".to_owned(), "1.5".to_owned())], now),
            None
        );
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("{code}: {message}")]
pub struct ModelAdapterError {
    pub category: ModelFailureCategory,
    pub code: String,
    pub message: String,
}

impl ModelAdapterError {
    #[must_use]
    pub fn protocol(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            category: ModelFailureCategory::Protocol,
            code: code.into(),
            message: message.into(),
        }
    }

    #[must_use]
    pub fn invalid_request(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            category: ModelFailureCategory::InvalidRequest,
            code: code.into(),
            message: message.into(),
        }
    }

    #[must_use]
    pub fn compatibility(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            category: ModelFailureCategory::UnsupportedParameter,
            code: code.into(),
            message: message.into(),
        }
    }

    #[must_use]
    pub fn into_failure(self, attempt_id: Option<TransportAttemptId>) -> NormalizedModelFailure {
        NormalizedModelFailure {
            category: self.category,
            code: self.code,
            message: self.message,
            retry_class: aura_model_domain::ModelRetryClass::NotRetryable,
            provider_status: None,
            provider_request_id: None,
            provider_error_ref: None,
            partial_output_ref: None,
            transport_attempt_id: attempt_id,
        }
    }
}

pub trait ModelProtocolCompiler: Send + Sync {
    fn compile_request(
        &self,
        input: ModelRequestCompilationInput<'_>,
    ) -> Result<ProviderRequestPlan, ModelAdapterError>;
}

/// Compiles and validates the non-streaming, text-only generation probe used
/// by Model Availability Test.
pub trait ModelGenerationProbeProtocol: Send + Sync {
    fn compile_generation_probe(
        &self,
        input: ModelAvailabilityProbeCompilationInput<'_>,
    ) -> Result<ProviderRequestPlan, ModelAdapterError>;

    /// Validates a successful HTTP response as a complete provider protocol
    /// terminal containing at least one non-blank visible text block.
    ///
    /// HTTP status classification remains the operation runner's
    /// responsibility; this method is only called for 2xx responses.
    fn validate_generation_probe_success_body(&self, body: &[u8]) -> Result<(), ModelAdapterError>;
}

pub fn validate_generation_probe_input(
    input: ModelAvailabilityProbeCompilationInput<'_>,
    expected_api_format: ModelApiFormat,
) -> Result<(), ModelAdapterError> {
    if input.endpoint.api_format != expected_api_format {
        return Err(ModelAdapterError::invalid_request(
            "model_generation_probe.api_format_mismatch",
            "generation probe adapter does not match the frozen endpoint API format",
        ));
    }
    if input.model_profile.endpoint_id != input.endpoint.id
        || input.model_profile.provider_model_name.trim().is_empty()
    {
        return Err(ModelAdapterError::invalid_request(
            "model_generation_probe.profile_mismatch",
            "generation probe model profile does not belong to the frozen endpoint",
        ));
    }
    if input.applied_output_token_field_alias.is_some()
        && expected_api_format != ModelApiFormat::OpenAiChatCompletions
    {
        return Err(ModelAdapterError::invalid_request(
            "model_generation_probe.output_token_alias_format_mismatch",
            "output-token field aliases are only valid for Chat Completions probes",
        ));
    }
    match input.operation_context {
        TransportOperationContext::ModelAvailabilityTest { model_profile_id, .. }
            if *model_profile_id == input.model_profile.id =>
        {
            Ok(())
        }
        _ => Err(ModelAdapterError::invalid_request(
            "model_generation_probe.operation_context_mismatch",
            "generation probe requires the matching Model Availability Test context",
        )),
    }
}

pub fn validate_model_compilation_input(
    input: ModelRequestCompilationInput<'_>,
    expected_api_format: ModelApiFormat,
) -> Result<(), ModelAdapterError> {
    input.request.validate_contract_shape().map_err(|error| {
        ModelAdapterError::invalid_request(
            "model_request.contract_invalid",
            format!("logical model request violates its frozen contract: {error}"),
        )
    })?;
    validate_resolved_attachments(input.request, input.resolved_attachments).map_err(|error| {
        let code = match error {
            aura_model_domain::LogicalModelRequestError::AttachmentsNotResolved => {
                "model_request.attachments_not_resolved"
            }
            _ => "model_attachment.snapshot_mismatch",
        };
        ModelAdapterError::invalid_request(code, error.to_string())
    })?;
    if input.snapshot.api_format != expected_api_format {
        return Err(ModelAdapterError::invalid_request(
            "model_request.api_format_mismatch",
            "protocol adapter does not match the frozen model execution snapshot",
        ));
    }
    if input.request.model_step_id != input.snapshot.model_step_id
        || input.request.execution_snapshot_id != input.snapshot.id
        || input.request.context_snapshot_id != input.snapshot.context_snapshot_id
    {
        return Err(ModelAdapterError::invalid_request(
            "model_request.snapshot_identity_mismatch",
            "logical model request identities do not match the execution snapshot",
        ));
    }
    let expected_context = TransportOperationContext::AgentModelAttempt {
        model_step_id: input.snapshot.model_step_id,
        model_execution_snapshot_id: input.snapshot.id,
        context_snapshot_id: input.snapshot.context_snapshot_id,
        prompt_bundle_snapshot_id: input.snapshot.prompt_bundle_snapshot_id,
    };
    if input.operation_context != &expected_context {
        return Err(ModelAdapterError::invalid_request(
            "model_request.operation_context_mismatch",
            "transport operation context does not match the model execution snapshot",
        ));
    }
    if input.snapshot.request_options.effective_max_output_tokens == 0 {
        return Err(ModelAdapterError::invalid_request(
            "model_request.output_limit_missing",
            "normal model execution requires a non-zero effective max output token limit",
        ));
    }
    if input.request.output_request.effective_max_output_tokens
        != input.snapshot.request_options.effective_max_output_tokens
    {
        return Err(ModelAdapterError::invalid_request(
            "model_request.output_limit_snapshot_mismatch",
            "logical output-token reserve does not match the frozen model execution snapshot",
        ));
    }
    if !input.compatibility_removals.is_empty() {
        return Err(ModelAdapterError::invalid_request(
            "model_request.compatibility_removal_not_allowed",
            "Adapter Compilation Contract v1 has an empty compatibility-removal whitelist",
        ));
    }
    if input.request.provider_continuation.is_some() {
        return Err(ModelAdapterError::invalid_request(
            "model_request.continuation_not_resolved",
            "provider continuation bytes and identity mapping were not resolved for compilation",
        ));
    }
    let reuse_domain = ProviderToolCallReuseDomain {
        endpoint_id: input.snapshot.endpoint_id,
        endpoint_security_fingerprint: input.snapshot.endpoint_fingerprint.clone(),
        api_format: input.snapshot.api_format,
    };
    validate_provider_tool_call_wire_bindings(
        &input.snapshot.historical_tool_call_wire_bindings,
        &reuse_domain,
    )
    .map_err(|error| {
        ModelAdapterError::invalid_request(
            "model_request.tool_call_binding_invalid",
            format!("historical tool-call wire binding is invalid: {error}"),
        )
    })?;
    let mut historical_calls = BTreeMap::new();
    for item in input.request.sent_items() {
        if let LogicalConversationItem::Assistant { tool_calls, .. } = item {
            for call in tool_calls {
                if historical_calls.insert(call.tool_call_id, call.tool_id).is_some() {
                    return Err(ModelAdapterError::invalid_request(
                        "model_request.tool_call_identity_duplicate",
                        "historical ToolCallId appears more than once in the logical request",
                    ));
                }
            }
        }
    }
    if input.request.sent_items().any(|item| {
        matches!(item, LogicalConversationItem::ToolResult { tool_call_id, .. }
            if !historical_calls.contains_key(tool_call_id))
    }) {
        return Err(ModelAdapterError::invalid_request(
            "model_request.tool_call_mapping_not_resolved",
            "historical tool result does not reference a logical assistant tool call",
        ));
    }
    let historical_bindings = input
        .snapshot
        .historical_tool_call_wire_bindings
        .iter()
        .map(|binding| (binding.tool_call_id, binding.tool_id))
        .collect::<BTreeMap<_, _>>();
    if historical_calls != historical_bindings {
        return Err(ModelAdapterError::invalid_request(
            "model_request.tool_call_mapping_not_resolved",
            "execution snapshot does not contain the exact historical ToolCallId bindings",
        ));
    }
    Ok(())
}

#[must_use]
pub fn historical_tool_call_wire_bindings(
    snapshot: &ModelExecutionSnapshot,
) -> BTreeMap<ToolCallId, &ProviderToolCallWireBinding> {
    snapshot
        .historical_tool_call_wire_bindings
        .iter()
        .map(|binding| (binding.tool_call_id, binding))
        .collect()
}

pub fn provider_tool_name_bindings(
    request: &LogicalModelRequest,
) -> Result<Vec<ProviderToolNameBinding>, ModelAdapterError> {
    let bindings = request
        .tool_schema_set
        .tools
        .iter()
        .map(|snapshot| {
            let (_, wrapped_hash) =
                aura_model_domain::provider_arguments_envelope_schema_v3(&snapshot.input_schema)
                    .map_err(|error| {
                        ModelAdapterError::invalid_request(
                            "model_request.tool_schema_envelope_invalid",
                            format!("provider tool schema envelope is invalid: {error}"),
                        )
                    })?;
            Ok(ProviderToolNameBinding::from_snapshot(
                &request.tool_schema_set.registry_snapshot_ref,
                snapshot,
                wrapped_hash,
            ))
        })
        .collect::<Result<Vec<_>, ModelAdapterError>>()?;
    for binding in &bindings {
        binding.validate_contract_shape().map_err(|error| {
            ModelAdapterError::invalid_request(
                "model_request.tool_binding_invalid",
                format!("provider tool name binding is invalid: {error}"),
            )
        })?;
    }
    Ok(bindings)
}

pub fn build_provider_request_plan(
    input: ModelRequestCompilationInput<'_>,
    body: Vec<u8>,
    headers: Vec<NonSecretProviderHeader>,
    tool_name_bindings: Vec<ProviderToolNameBinding>,
) -> Result<ProviderRequestPlan, ModelAdapterError> {
    build_provider_request_plan_for_target(ProviderRequestPlanParts {
        method: "POST",
        resolved_request_url: &input.snapshot.resolved_request_url,
        auth_scheme: input.snapshot.auth_scheme,
        api_format: input.snapshot.api_format,
        operation_context: input.operation_context,
        compatibility_removals: input.compatibility_removals.clone(),
        applied_output_token_field_alias: input.applied_output_token_field_alias.cloned(),
        body,
        headers,
        tool_registry_snapshot_ref: Some(
            input.request.tool_schema_set.registry_snapshot_ref.clone(),
        ),
        tool_name_bindings,
        historical_tool_call_wire_bindings: input
            .snapshot
            .historical_tool_call_wire_bindings
            .clone(),
    })
}

pub fn build_generation_probe_request_plan(
    input: ModelAvailabilityProbeCompilationInput<'_>,
    body: Vec<u8>,
    headers: Vec<NonSecretProviderHeader>,
) -> Result<ProviderRequestPlan, ModelAdapterError> {
    build_provider_request_plan_for_target(ProviderRequestPlanParts {
        method: "POST",
        resolved_request_url: input.resolved_request_url,
        auth_scheme: input.endpoint.auth_scheme,
        api_format: input.endpoint.api_format,
        operation_context: input.operation_context,
        compatibility_removals: BTreeSet::new(),
        applied_output_token_field_alias: input.applied_output_token_field_alias.cloned(),
        body,
        headers,
        tool_registry_snapshot_ref: None,
        tool_name_bindings: Vec::new(),
        historical_tool_call_wire_bindings: Vec::new(),
    })
}

pub fn build_model_discovery_request_plan(
    input: ModelDiscoveryCompilationInput<'_>,
) -> Result<ProviderRequestPlan, ModelAdapterError> {
    if !matches!(input.operation_context, TransportOperationContext::ModelDiscovery { .. }) {
        return Err(ModelAdapterError::invalid_request(
            "model_discovery.operation_context_mismatch",
            "model discovery requires a ModelDiscovery operation context",
        ));
    }
    let mut headers = vec![NonSecretProviderHeader {
        name: "accept".to_owned(),
        value: "application/json".to_owned(),
        class: ProviderRequestHeaderClass::ProtocolFixed,
    }];
    match input.endpoint.api_format {
        ModelApiFormat::OpenAiResponses | ModelApiFormat::OpenAiChatCompletions => {
            if let Some(value) = &input.endpoint.openai_organization_id {
                headers.push(NonSecretProviderHeader {
                    name: "openai-organization".to_owned(),
                    value: value.clone(),
                    class: ProviderRequestHeaderClass::SemanticRouting,
                });
            }
            if let Some(value) = &input.endpoint.openai_project_id {
                headers.push(NonSecretProviderHeader {
                    name: "openai-project".to_owned(),
                    value: value.clone(),
                    class: ProviderRequestHeaderClass::SemanticRouting,
                });
            }
        }
        ModelApiFormat::AnthropicMessages => {
            headers.push(NonSecretProviderHeader {
                name: "anthropic-version".to_owned(),
                value: "2023-06-01".to_owned(),
                class: ProviderRequestHeaderClass::ProtocolFixed,
            });
            if let Some(value) = &input.endpoint.anthropic_workspace_id {
                headers.push(NonSecretProviderHeader {
                    name: "anthropic-workspace-id".to_owned(),
                    value: value.clone(),
                    class: ProviderRequestHeaderClass::SemanticRouting,
                });
            }
        }
    }
    build_provider_request_plan_for_target(ProviderRequestPlanParts {
        method: "GET",
        resolved_request_url: input.resolved_request_url,
        auth_scheme: input.endpoint.auth_scheme,
        api_format: input.endpoint.api_format,
        operation_context: input.operation_context,
        compatibility_removals: BTreeSet::new(),
        applied_output_token_field_alias: None,
        body: Vec::new(),
        headers,
        tool_registry_snapshot_ref: None,
        tool_name_bindings: Vec::new(),
        historical_tool_call_wire_bindings: Vec::new(),
    })
}

struct ProviderRequestPlanParts<'a> {
    method: &'a str,
    resolved_request_url: &'a str,
    auth_scheme: EndpointAuthScheme,
    api_format: ModelApiFormat,
    operation_context: &'a TransportOperationContext,
    compatibility_removals: BTreeSet<String>,
    applied_output_token_field_alias: Option<ChatOutputTokenFieldAlias>,
    body: Vec<u8>,
    headers: Vec<NonSecretProviderHeader>,
    tool_registry_snapshot_ref: Option<aura_tool_contract::ToolRegistrySnapshotRef>,
    tool_name_bindings: Vec<ProviderToolNameBinding>,
    historical_tool_call_wire_bindings: Vec<ProviderToolCallWireBinding>,
}

fn build_provider_request_plan_for_target(
    parts: ProviderRequestPlanParts<'_>,
) -> Result<ProviderRequestPlan, ModelAdapterError> {
    let ProviderRequestPlanParts {
        method,
        resolved_request_url,
        auth_scheme,
        api_format,
        operation_context,
        compatibility_removals,
        applied_output_token_field_alias,
        body,
        mut headers,
        tool_registry_snapshot_ref,
        tool_name_bindings,
        historical_tool_call_wire_bindings,
    } = parts;
    if headers.iter().any(|header| header.name.eq_ignore_ascii_case("accept-encoding")) {
        return Err(ModelAdapterError::invalid_request(
            "model_request.accept_encoding_owned_by_transport",
            "protocol adapters must not override the transport-owned Accept-Encoding header",
        ));
    }
    headers.push(NonSecretProviderHeader {
        name: "accept-encoding".to_owned(),
        value: PROVIDER_ACCEPT_ENCODING.to_owned(),
        class: ProviderRequestHeaderClass::ProtocolFixed,
    });
    let request_url = Url::parse(resolved_request_url).map_err(|_| {
        ModelAdapterError::invalid_request(
            "model_request.target_invalid",
            "frozen provider request URL is not a valid absolute URL",
        )
    })?;
    if !matches!(request_url.scheme(), "http" | "https")
        || !request_url.username().is_empty()
        || request_url.password().is_some()
        || request_url.fragment().is_some()
    {
        return Err(ModelAdapterError::invalid_request(
            "model_request.target_invalid",
            "provider request URL violates the HTTP target contract",
        ));
    }
    let host = request_url.host_str().ok_or_else(|| {
        ModelAdapterError::invalid_request(
            "model_request.target_invalid",
            "provider request URL has no host",
        )
    })?;
    let port = request_url.port_or_known_default().ok_or_else(|| {
        ModelAdapterError::invalid_request(
            "model_request.target_invalid",
            "provider request URL has no effective port",
        )
    })?;
    let target = ProviderRequestTargetMetadata {
        method: method.to_owned(),
        origin: NormalizedOrigin {
            scheme: request_url.scheme().to_owned(),
            host: host.to_owned(),
            port,
        },
        path: request_url.path().to_owned(),
        query_parameter_names: request_url
            .query_pairs()
            .map(|(name, _)| name.into_owned())
            .collect(),
    };
    let mut header_metadata = headers
        .iter()
        .map(|header| ProviderRequestHeaderMetadata {
            name: header.name.clone(),
            class: header.class,
            non_secret_value: Some(header.value.clone()),
        })
        .collect::<Vec<_>>();
    let credential_header = match auth_scheme {
        EndpointAuthScheme::None => None,
        EndpointAuthScheme::OpenAiBearerApiKey => Some("authorization"),
        EndpointAuthScheme::AnthropicApiKey => Some("x-api-key"),
    };
    if let Some(name) = credential_header {
        header_metadata.push(ProviderRequestHeaderMetadata {
            name: name.to_owned(),
            class: ProviderRequestHeaderClass::CredentialInjected,
            non_secret_value: None,
        });
    }
    header_metadata.sort_by(|left, right| left.name.cmp(&right.name));
    let body_byte_length = u64::try_from(body.len()).map_err(|_| {
        ModelAdapterError::invalid_request(
            "model_request.body_too_large",
            "provider request body length exceeds the supported range",
        )
    })?;
    let request_evidence = ProviderRequestEvidence {
        body_sha256: aura_domain_common::ContentHash::sha256(&body),
        body_byte_length,
        target,
        headers: header_metadata,
        compatibility_removals,
        applied_output_token_field_alias,
        adapter: if matches!(operation_context, TransportOperationContext::AgentModelAttempt { .. })
        {
            ModelAdapterIdentity::current(api_format)
        } else {
            ModelAdapterIdentity::v1(api_format)
        },
        operation_context: operation_context.clone(),
        tool_registry_snapshot_ref,
        tool_name_bindings,
        historical_tool_call_wire_bindings,
        canonical_body_artifact_id: None,
    };
    request_evidence.validate().map_err(|error| {
        ModelAdapterError::invalid_request(
            "model_request.evidence_invalid",
            format!("compiled provider request evidence is invalid: {error}"),
        )
    })?;
    Ok(ProviderRequestPlan {
        method: method.to_owned(),
        request_url: request_url.to_string(),
        headers: headers.into_iter().map(|header| (header.name, header.value)).collect(),
        body,
        safe_optional_parameters: BTreeSet::new(),
        request_evidence,
    })
}

pub trait ModelStreamDecoder: Send {
    fn push_frame(
        &mut self,
        frame: ProviderStreamFrame,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError>;

    fn finish(&mut self) -> Result<NormalizedModelOutput, ModelAdapterError>;

    fn take_continuation_artifact(
        &mut self,
    ) -> Result<Option<ProviderContinuationArtifact>, ModelAdapterError> {
        Ok(None)
    }
}

pub trait ModelProtocolDecoder: Send + Sync {
    fn api_format(&self) -> ModelApiFormat;

    /// The Provider output item kind this adapter parses a function call from.
    ///
    /// It is bounded, adapter-owned evidence for a refused call: an operator
    /// needs to know whether the illegal name arrived as a Responses
    /// `function_call` item, a Chat Completions `tool_calls` entry or an
    /// Anthropic `tool_use` block. An adapter that does not state it reports
    /// `unknown` rather than guessing.
    fn tool_call_item_type(&self) -> &'static str {
        "unknown"
    }

    fn create_stream_decoder(
        &self,
        snapshot: &ModelExecutionSnapshot,
    ) -> Box<dyn ModelStreamDecoder>;

    fn classify_http_error(&self, response: &ProviderErrorResponse) -> NormalizedModelFailure;

    fn runtime_capability_facts(
        &self,
        request_plan: &ProviderRequestPlan,
    ) -> ModelRuntimeCapabilityFacts;
}
