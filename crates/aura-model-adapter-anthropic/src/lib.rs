#![forbid(unsafe_code)]
mod decoder;
mod history;

use std::collections::{BTreeMap, BTreeSet};

use aura_domain_common::ModelContentBlockId;
use aura_model_domain::{
    LogicalConversationItem, ModelApiFormat, ModelExecutionSnapshot, ModelFailureCategory,
    ModelOutputFormat, ModelRetryClass, ModelRuntimeCapabilityFacts, ModelStreamEvent,
    NormalizedModelFailure, NormalizedModelOutput, NormalizedStopReason, PartialModelUsage,
    ProviderProtocolReference, ProviderRequestHeaderClass, ProviderToolCallId,
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

pub struct AnthropicMessagesAdapter;

impl ModelProtocolCompiler for AnthropicMessagesAdapter {
    fn compile_request(
        &self,
        input: ModelRequestCompilationInput<'_>,
    ) -> Result<ProviderRequestPlan, ModelAdapterError> {
        validate_model_compilation_input(input, ModelApiFormat::AnthropicMessages)?;
        let tool_name_bindings = provider_tool_name_bindings(input.request)?;
        let layout = aura_model_transport::instruction_layout(input.request);
        let compile = |items: &[LogicalConversationItem]| {
            history::compile_anthropic_history(
                input.request,
                items,
                input.snapshot,
                input.resolved_attachments,
            )
        };
        let mut history = compile(&input.request.conversation_items)?;
        // Compatible Messages endpoints may reject Anthropic's cache extensions.
        // Only the native endpoint gets explicit, default-TTL cache breakpoints.
        let native_cache =
            input.snapshot.resolved_request_url.starts_with("https://api.anthropic.com/");
        if native_cache {
            history::mark_cacheable_tail(&mut history);
        }
        // The task state follows the history in the last user turn. A question asked after the
        // step's request - a compaction - is a text block of its own after it, and what followed it
        // when the model called tools comes next, so all before them is the step's request as it
        // was sent. No `tool_choice` is sent: changing it drops Anthropic's cache of the messages.
        let mut turn = vec![json!({"type":"text","text":layout.current})];
        if let Some(question) = &input.request.closing_question {
            turn.push(json!({"type":"text","text":question}));
        }
        if let Some(last) = history.last_mut().filter(|m| m["role"] == "user") {
            last["content"].as_array_mut().expect("compiled content blocks").extend(turn);
        } else {
            history.push(json!({"role":"user","content":turn}));
        }
        history.extend(compile(&input.request.closing_exchange)?);
        let mut body = Map::new();
        body.insert("model".to_owned(), Value::String(input.snapshot.model_name.clone()));
        body.insert(
            "system".to_owned(),
            json!([{
                "type": "text",
                "text": layout.stable,
            }]),
        );
        if native_cache {
            body["system"][0]["cache_control"] = json!({"type":"ephemeral"});
        }
        body.insert("messages".to_owned(), Value::Array(history));
        body.insert(
            "max_tokens".to_owned(),
            Value::from(input.snapshot.request_options.effective_max_output_tokens),
        );
        body.insert("stream".to_owned(), Value::Bool(true));
        match &input.request.output_request.format {
            ModelOutputFormat::Text => {}
            ModelOutputFormat::JsonObject => {
                return Err(ModelAdapterError::compatibility(
                    "model_output.json_object_unsupported",
                    "Anthropic Messages has no schema-free json_object output mode",
                ));
            }
            ModelOutputFormat::JsonSchema { schema, .. } => {
                body.insert(
                    "output_config".to_owned(),
                    json!({"format": {"type": "json_schema", "schema": schema}}),
                );
            }
        }
        // A thinking level is a thinking budget, inside the output limit with room left
        // for the answer. A model whose catalog lists effort levels takes the level as
        // `output_config.effort` instead and thinks as deep as it says. Thinking takes the
        // provider's own sampling, so the profile's is not sent.
        let options = &input.snapshot.request_options;
        let effort = options
            .reasoning_effort
            .filter(|_| options.reasoning_by_effort)
            .and_then(aura_model_domain::ReasoningEffort::anthropic_effort_name);
        let output = options.effective_max_output_tokens;
        let thinking = options
            .reasoning_effort
            .filter(|_| !options.reasoning_by_effort)
            .and_then(aura_model_domain::ReasoningEffort::anthropic_budget_tokens)
            .map(|budget| budget.min(output.saturating_sub(1_024)))
            .filter(|budget| *budget >= 1_024);
        if let Some(effort) = effort {
            let config = body.entry("output_config".to_owned()).or_insert_with(|| json!({}));
            if let Some(config) = config.as_object_mut() {
                config.insert("effort".to_owned(), Value::from(effort));
            }
        } else if let Some(budget) = thinking {
            body.insert("thinking".to_owned(), json!({"type": "enabled", "budget_tokens": budget}));
        } else {
            if let Some(temperature) = input.snapshot.request_options.temperature {
                body.insert("temperature".to_owned(), Value::from(temperature));
            }
            if let Some(top_p) = input.snapshot.request_options.top_p {
                body.insert("top_p".to_owned(), Value::from(top_p));
            }
        }
        if !input.request.tool_schema_set.tools.is_empty() {
            let provider_tools =
                aura_model_transport::ordered_tool_bindings(input.request, &tool_name_bindings)
                    .into_iter()
                    .map(|(tool, binding)| -> Result<Value, ModelAdapterError> {
                        let mut wire = Map::new();
                        wire.insert(
                            "name".to_owned(),
                            Value::String(binding.provider_wire_name.clone()),
                        );
                        wire.insert(
                            "description".to_owned(),
                            Value::String(tool.description.clone()),
                        );
                        let (wrapped_schema, wrapped_hash) =
                            aura_model_domain::provider_arguments_envelope_schema_v3(
                                &tool.input_schema,
                            )
                            .map_err(|error| {
                                ModelAdapterError::invalid_request(
                                    "model_request.tool_schema_envelope_invalid",
                                    error.to_string(),
                                )
                            })?;
                        debug_assert_eq!(wrapped_hash, binding.provider_wrapped_schema_hash);
                        // `strict` only for a schema inside the Providers' strict subset.
                        let strict = tool.strict
                            && aura_model_transport::strict_schema_compatible(&wrapped_schema);
                        wire.insert("input_schema".to_owned(), wrapped_schema);
                        if strict {
                            wire.insert("strict".to_owned(), Value::Bool(true));
                        }
                        Ok(Value::Object(wire))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
            body.insert("tools".to_owned(), Value::Array(provider_tools));
        }
        let body = serde_json::to_vec(&Value::Object(body)).map_err(|_| {
            ModelAdapterError::invalid_request(
                "model_request.serialization_failed",
                "Anthropic Messages request could not be serialized",
            )
        })?;
        let mut headers = vec![
            NonSecretProviderHeader {
                name: "accept".to_owned(),
                value: "text/event-stream".to_owned(),
                class: ProviderRequestHeaderClass::ProtocolFixed,
            },
            NonSecretProviderHeader {
                name: "anthropic-version".to_owned(),
                value: "2023-06-01".to_owned(),
                class: ProviderRequestHeaderClass::ProtocolFixed,
            },
            NonSecretProviderHeader {
                name: "content-type".to_owned(),
                value: "application/json".to_owned(),
                class: ProviderRequestHeaderClass::ContentMetadata,
            },
        ];
        if let Some(workspace_id) = &input.snapshot.anthropic_workspace_id {
            headers.push(NonSecretProviderHeader {
                name: "anthropic-workspace-id".to_owned(),
                value: workspace_id.clone(),
                class: ProviderRequestHeaderClass::SemanticRouting,
            });
        }
        build_provider_request_plan(input, body, headers, tool_name_bindings)
    }
}

impl ModelGenerationProbeProtocol for AnthropicMessagesAdapter {
    fn compile_generation_probe(
        &self,
        input: ModelAvailabilityProbeCompilationInput<'_>,
    ) -> Result<ProviderRequestPlan, ModelAdapterError> {
        validate_generation_probe_input(input, ModelApiFormat::AnthropicMessages)?;
        let body = serde_json::to_vec(&json!({
            "model": input.model_profile.provider_model_name,
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "text",
                    "text": MODEL_GENERATION_PROBE_PROMPT,
                }],
            }],
            "max_tokens": MODEL_GENERATION_PROBE_MAX_OUTPUT_TOKENS,
            "stream": false,
        }))
        .map_err(|_| {
            ModelAdapterError::invalid_request(
                "model_generation_probe.serialization_failed",
                "Anthropic Messages generation probe could not be serialized",
            )
        })?;
        let mut headers = vec![
            NonSecretProviderHeader {
                name: "accept".to_owned(),
                value: "application/json".to_owned(),
                class: ProviderRequestHeaderClass::ProtocolFixed,
            },
            NonSecretProviderHeader {
                name: "anthropic-version".to_owned(),
                value: "2023-06-01".to_owned(),
                class: ProviderRequestHeaderClass::ProtocolFixed,
            },
            NonSecretProviderHeader {
                name: "content-type".to_owned(),
                value: "application/json".to_owned(),
                class: ProviderRequestHeaderClass::ContentMetadata,
            },
        ];
        if let Some(workspace_id) = &input.endpoint.anthropic_workspace_id {
            headers.push(NonSecretProviderHeader {
                name: "anthropic-workspace-id".to_owned(),
                value: workspace_id.clone(),
                class: ProviderRequestHeaderClass::SemanticRouting,
            });
        }
        build_generation_probe_request_plan(input, body, headers)
    }

    fn validate_generation_probe_success_body(&self, body: &[u8]) -> Result<(), ModelAdapterError> {
        let value: Value = serde_json::from_slice(body).map_err(|_| {
            ModelAdapterError::protocol(
                "model_generation_probe.anthropic_invalid_json",
                "Anthropic Messages generation probe body is not valid JSON",
            )
        })?;
        let stop_reason = value.get("stop_reason").and_then(Value::as_str);
        if value.get("type").and_then(Value::as_str) != Some("message")
            || stop_reason.is_none()
            || stop_reason == Some("refusal")
        {
            return Err(ModelAdapterError::protocol(
                "model_generation_probe.anthropic_terminal_invalid",
                "Anthropic Messages generation probe did not reach a valid non-refusal terminal",
            ));
        }
        let has_visible_text =
            value.get("content").and_then(Value::as_array).into_iter().flatten().any(|part| {
                part.get("type").and_then(Value::as_str) == Some("text")
                    && part
                        .get("text")
                        .and_then(Value::as_str)
                        .is_some_and(|text| !text.trim().is_empty())
            });
        if !has_visible_text {
            return Err(ModelAdapterError::protocol(
                "model_generation_probe.visible_text_missing",
                "Anthropic Messages generation probe returned no non-blank visible text",
            ));
        }
        Ok(())
    }
}

fn parse_usage(value: &Value) -> PartialModelUsage {
    let cached_input_tokens = value.get("cache_read_input_tokens").and_then(Value::as_u64);
    let cache_write_tokens = value.get("cache_creation_input_tokens").and_then(Value::as_u64);
    // Anthropic reports uncached input separately. Normalize the whole input like
    // the other protocols, while retaining the cache breakdown for inspection.
    let input_tokens = value.get("input_tokens").and_then(Value::as_u64).map(|uncached| {
        uncached
            .saturating_add(cached_input_tokens.unwrap_or(0))
            .saturating_add(cache_write_tokens.unwrap_or(0))
    });
    PartialModelUsage {
        input_tokens,
        output_tokens: value.get("output_tokens").and_then(Value::as_u64),
        total_tokens: None,
        reasoning_tokens: None,
        cached_input_tokens,
        cache_write_tokens,
    }
}

fn map_stop_reason(reason: &str) -> NormalizedStopReason {
    match reason {
        "end_turn" => NormalizedStopReason::EndTurn,
        "tool_use" => NormalizedStopReason::ToolCalls,
        "max_tokens" => NormalizedStopReason::MaxOutputTokens,
        "stop_sequence" => NormalizedStopReason::StopSequence,
        "refusal" => NormalizedStopReason::Refusal,
        "pause_turn" => NormalizedStopReason::ProviderIncomplete,
        _ => NormalizedStopReason::Unknown,
    }
}

fn optional_nullable_string<'a>(
    value: &'a serde_json::Map<String, Value>,
    field: &str,
) -> Result<Option<&'a str>, ModelAdapterError> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(ModelAdapterError::protocol(
            "anthropic_refusal_details_invalid",
            format!("Anthropic refusal stop_details.{field} is not a string or null"),
        )),
    }
}

fn stream_error(value: &Value) -> ModelAdapterError {
    let error = value.get("error").unwrap_or(value);
    ModelAdapterError::protocol(
        error.get("type").and_then(Value::as_str).unwrap_or("anthropic_stream_error"),
        error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Anthropic Messages stream reported an error"),
    )
}

fn classify_http_error(response: &ProviderErrorResponse) -> NormalizedModelFailure {
    let value = serde_json::from_slice::<Value>(&response.body).ok();
    let error = value.as_ref().and_then(|value| value.get("error")).or(value.as_ref());
    let code = error
        .and_then(|value| value.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("anthropic_http_error")
        .to_owned();
    let message = error
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("Anthropic Messages request failed")
        .to_owned();
    let lower = format!("{} {}", code.to_ascii_lowercase(), message.to_ascii_lowercase());
    let (category, mut retry_class) = classify_status(response.status, &lower, error);
    if let ModelRetryClass::TransparentRetry { retry_after_ms } = &mut retry_class {
        *retry_after_ms = retry_after_milliseconds(&response.headers);
    }
    NormalizedModelFailure {
        category,
        code,
        message,
        retry_class,
        provider_status: Some(response.status),
        provider_request_id: response.provider_request_id.clone(),
        provider_error_ref: response.raw_error_artifact_id,
        partial_output_ref: None,
        transport_attempt_id: None,
    }
}

fn classify_status(
    status: u16,
    lower: &str,
    error: Option<&Value>,
) -> (ModelFailureCategory, ModelRetryClass) {
    if status == 401 {
        return (ModelFailureCategory::Authentication, ModelRetryClass::NotRetryable);
    }
    if status == 403 {
        return (ModelFailureCategory::Authorization, ModelRetryClass::NotRetryable);
    }
    if status == 429 {
        if contains_any(lower, &["quota", "balance", "billing", "credit"]) {
            return (ModelFailureCategory::QuotaOrBalance, ModelRetryClass::NotRetryable);
        }
        return (
            ModelFailureCategory::RateLimit,
            ModelRetryClass::TransparentRetry { retry_after_ms: None },
        );
    }
    if matches!(status, 408 | 500 | 502 | 503 | 504 | 529) {
        return (
            ModelFailureCategory::Transport,
            ModelRetryClass::TransparentRetry { retry_after_ms: None },
        );
    }
    if contains_any(lower, &["context window", "too many tokens", "prompt is too long"]) {
        return (ModelFailureCategory::ContextWindowExceeded, ModelRetryClass::NotRetryable);
    }
    if contains_any(lower, &["not_found_error", "model not found", "unknown model"]) {
        return (ModelFailureCategory::UnsupportedModel, ModelRetryClass::NotRetryable);
    }
    if error.and_then(|value| value.get("param")).and_then(Value::as_str).is_some()
        && contains_any(lower, &["unsupported", "unknown", "not supported"])
    {
        return (ModelFailureCategory::UnsupportedParameter, ModelRetryClass::NotRetryable);
    }
    (ModelFailureCategory::InvalidRequest, ModelRetryClass::NotRetryable)
}

fn has_native_attachments(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(has_native_attachments),
        Value::Object(object) => {
            object
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| matches!(kind, "image" | "document"))
                || object.values().any(has_native_attachments)
        }
        _ => false,
    }
}

fn required_index(value: &Value) -> Result<u32, ModelAdapterError> {
    value
        .get("index")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| {
            ModelAdapterError::protocol(
                "anthropic_content_block_index_missing",
                "Anthropic content event has no valid block index",
            )
        })
}

fn required_delta_string<'a>(value: &'a Value, field: &str) -> Result<&'a str, ModelAdapterError> {
    value.get(field).and_then(Value::as_str).ok_or_else(|| {
        ModelAdapterError::protocol(
            "anthropic_delta_field_missing",
            format!("Anthropic delta has no string {field}"),
        )
    })
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}
