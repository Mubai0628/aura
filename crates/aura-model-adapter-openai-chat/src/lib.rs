#![forbid(unsafe_code)]
mod decoder;
mod history;

use aura_model_domain::{
    ChatInstructionRole, ChatOutputTokenField, LogicalConversationItem, ModelApiFormat,
    ModelExecutionSnapshot, ModelOutputFormat, ModelRuntimeCapabilityFacts, NormalizedModelFailure,
    ProviderRequestHeaderClass,
};
use aura_model_transport::{
    MODEL_GENERATION_PROBE_MAX_OUTPUT_TOKENS, MODEL_GENERATION_PROBE_PROMPT, ModelAdapterError,
    ModelAvailabilityProbeCompilationInput, ModelGenerationProbeProtocol, ModelProtocolCompiler,
    ModelProtocolDecoder, ModelRequestCompilationInput, ModelStreamDecoder,
    NonSecretProviderHeader, ProviderErrorFields, ProviderErrorResponse, ProviderRequestPlan,
    build_generation_probe_request_plan, build_provider_request_plan,
    historical_tool_call_wire_bindings, provider_http_failure, provider_tool_name_bindings,
    validate_generation_probe_input, validate_model_compilation_input,
};
use serde_json::{Map, Value, json};

pub struct OpenAiChatCompletionsAdapter;

impl ModelProtocolCompiler for OpenAiChatCompletionsAdapter {
    fn compile_request(
        &self,
        input: ModelRequestCompilationInput<'_>,
    ) -> Result<ProviderRequestPlan, ModelAdapterError> {
        validate_model_compilation_input(input, ModelApiFormat::OpenAiChatCompletions)?;
        let instruction_role = input.snapshot.chat_instruction_role.ok_or_else(|| {
            ModelAdapterError::invalid_request(
                "model_request.chat_instruction_role_missing",
                "Chat execution snapshot did not freeze an instruction role",
            )
        })?;
        let output_token_field = input.snapshot.chat_output_token_field.ok_or_else(|| {
            ModelAdapterError::invalid_request(
                "model_request.chat_output_token_field_missing",
                "Chat execution snapshot did not freeze an output-token field",
            )
        })?;
        let tool_name_bindings = provider_tool_name_bindings(input.request)?;
        let layout = aura_model_transport::instruction_layout(input.request);
        let mut messages = vec![json!({
            "role": chat_instruction_role(instruction_role),
            "content": layout.stable,
        })];
        let compile = |items: &[LogicalConversationItem]| {
            history::compile_chat_history(
                input.request,
                items,
                input.snapshot,
                input.resolved_attachments,
            )
        };
        messages.extend(compile(&input.request.conversation_items)?);
        messages.push(json!({
            "role": chat_instruction_role(instruction_role),
            "content": layout.current,
        }));
        // A question asked after the step's request - a compaction - is a user message of its own
        // after the task state, and what followed it when the model called tools comes next, so all
        // before them is the step's request as it was sent. Chat endpoints are not sent
        // `tool_choice`, which DeepSeek's thinking models refuse in its forced forms.
        if let Some(question) = &input.request.closing_question {
            messages.push(json!({"role": "user", "content": question}));
        }
        messages.extend(compile(&input.request.closing_exchange)?);

        let mut body = Map::new();
        body.insert("model".to_owned(), Value::String(input.snapshot.model_name.clone()));
        if let Some(key) =
            aura_model_transport::routing_key_field(input.snapshot, input.request, &layout)
        {
            body.insert(key.name, Value::String(key.value));
        }
        body.insert("messages".to_owned(), Value::Array(messages));
        body.insert(
            match output_token_field {
                ChatOutputTokenField::MaxCompletionTokens => "max_completion_tokens",
                ChatOutputTokenField::MaxTokens => "max_tokens",
            }
            .to_owned(),
            Value::from(input.snapshot.request_options.effective_max_output_tokens),
        );
        body.insert("stream".to_owned(), Value::Bool(true));
        body.insert("stream_options".to_owned(), json!({"include_usage": true}));
        body.insert(
            "response_format".to_owned(),
            chat_output_format(&input.request.output_request.format),
        );
        if let Some(temperature) = input.snapshot.request_options.temperature {
            body.insert("temperature".to_owned(), Value::from(temperature));
        }
        if let Some(top_p) = input.snapshot.request_options.top_p {
            body.insert("top_p".to_owned(), Value::from(top_p));
        }
        // The thinking level the profile asks for.
        if let Some(level) = input.snapshot.request_options.reasoning_effort {
            body.insert("reasoning_effort".to_owned(), Value::from(level.openai_name()));
        }
        body.extend(aura_model_transport::speed_mode_fields(input.snapshot));
        if !input.request.tool_schema_set.tools.is_empty() {
            let provider_tools =
                aura_model_transport::ordered_tool_bindings(input.request, &tool_name_bindings)
                    .into_iter()
                    .map(|(tool, binding)| -> Result<Value, ModelAdapterError> {
                        let mut function = Map::new();
                        function.insert(
                            "name".to_owned(),
                            Value::String(binding.provider_wire_name.clone()),
                        );
                        function.insert(
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
                        function.insert("parameters".to_owned(), wrapped_schema);
                        if strict {
                            function.insert("strict".to_owned(), Value::Bool(true));
                        }
                        Ok(json!({"type": "function", "function": Value::Object(function)}))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
            body.insert("tools".to_owned(), Value::Array(provider_tools));
        }
        let body = serde_json::to_vec(&Value::Object(body)).map_err(|_| {
            ModelAdapterError::invalid_request(
                "model_request.serialization_failed",
                "OpenAI Chat request could not be serialized",
            )
        })?;
        let mut headers = common_openai_headers("text/event-stream");
        if let Some(organization_id) = &input.snapshot.openai_organization_id {
            headers.push(NonSecretProviderHeader {
                name: "openai-organization".to_owned(),
                value: organization_id.clone(),
                class: ProviderRequestHeaderClass::SemanticRouting,
            });
        }
        if let Some(project_id) = &input.snapshot.openai_project_id {
            headers.push(NonSecretProviderHeader {
                name: "openai-project".to_owned(),
                value: project_id.clone(),
                class: ProviderRequestHeaderClass::SemanticRouting,
            });
        }
        build_provider_request_plan(input, body, headers, tool_name_bindings)
    }
}

impl ModelGenerationProbeProtocol for OpenAiChatCompletionsAdapter {
    fn compile_generation_probe(
        &self,
        input: ModelAvailabilityProbeCompilationInput<'_>,
    ) -> Result<ProviderRequestPlan, ModelAdapterError> {
        validate_generation_probe_input(input, ModelApiFormat::OpenAiChatCompletions)?;
        let configured_output_token_field = input
            .model_profile
            .request_defaults
            .chat_output_token_field
            .or(input.endpoint.chat_output_token_field)
            .ok_or_else(|| {
                ModelAdapterError::invalid_request(
                    "model_generation_probe.chat_output_token_field_missing",
                    "Chat generation probe requires the configured output-token field",
                )
            })?;
        let output_token_field = input
            .applied_output_token_field_alias
            .map_or(configured_output_token_field, |alias| alias.effective_field);
        let output_token_field = match output_token_field {
            ChatOutputTokenField::MaxCompletionTokens => "max_completion_tokens",
            ChatOutputTokenField::MaxTokens => "max_tokens",
        };
        let body = serde_json::to_vec(&json!({
            "model": input.model_profile.provider_model_name,
            "messages": [{
                "role": "user",
                "content": MODEL_GENERATION_PROBE_PROMPT,
            }],
            output_token_field: MODEL_GENERATION_PROBE_MAX_OUTPUT_TOKENS,
            "stream": false,
        }))
        .map_err(|_| {
            ModelAdapterError::invalid_request(
                "model_generation_probe.serialization_failed",
                "OpenAI Chat generation probe could not be serialized",
            )
        })?;
        let mut headers = common_openai_headers("application/json");
        if let Some(organization_id) = &input.endpoint.openai_organization_id {
            headers.push(NonSecretProviderHeader {
                name: "openai-organization".to_owned(),
                value: organization_id.clone(),
                class: ProviderRequestHeaderClass::SemanticRouting,
            });
        }
        if let Some(project_id) = &input.endpoint.openai_project_id {
            headers.push(NonSecretProviderHeader {
                name: "openai-project".to_owned(),
                value: project_id.clone(),
                class: ProviderRequestHeaderClass::SemanticRouting,
            });
        }
        build_generation_probe_request_plan(input, body, headers)
    }

    fn validate_generation_probe_success_body(&self, body: &[u8]) -> Result<(), ModelAdapterError> {
        let value: Value = serde_json::from_slice(body).map_err(|_| {
            ModelAdapterError::protocol(
                "model_generation_probe.chat_invalid_json",
                "OpenAI Chat generation probe body is not valid JSON",
            )
        })?;
        let choices = value.get("choices").and_then(Value::as_array).ok_or_else(|| {
            ModelAdapterError::protocol(
                "model_generation_probe.chat_terminal_invalid",
                "OpenAI Chat generation probe has no choices array",
            )
        })?;
        // The one choice there is may come without its index.
        if choices.len() != 1
            || !choices[0]
                .get("index")
                .is_none_or(|index| index.is_null() || index.as_u64() == Some(0))
            || choices[0].get("finish_reason").and_then(Value::as_str).is_none()
        {
            return Err(ModelAdapterError::protocol(
                "model_generation_probe.chat_terminal_invalid",
                "OpenAI Chat generation probe did not produce one complete terminal choice",
            ));
        }
        let message = choices[0].get("message").ok_or_else(|| {
            ModelAdapterError::protocol(
                "model_generation_probe.chat_terminal_invalid",
                "OpenAI Chat generation probe terminal has no assistant message",
            )
        })?;
        if message
            .get("refusal")
            .and_then(Value::as_str)
            .is_some_and(|refusal| !refusal.trim().is_empty())
            || !message
                .get("content")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.trim().is_empty())
        {
            return Err(ModelAdapterError::protocol(
                "model_generation_probe.visible_text_missing",
                "OpenAI Chat generation probe returned no non-blank visible text",
            ));
        }
        Ok(())
    }
}

const fn chat_instruction_role(role: ChatInstructionRole) -> &'static str {
    match role {
        ChatInstructionRole::System => "system",
        ChatInstructionRole::Developer => "developer",
    }
}

fn chat_output_format(format: &ModelOutputFormat) -> Value {
    match format {
        ModelOutputFormat::Text => json!({"type": "text"}),
        ModelOutputFormat::JsonObject => json!({"type": "json_object"}),
        ModelOutputFormat::JsonSchema { name, schema, strict, .. } => json!({
            "type": "json_schema",
            "json_schema": {
                "name": name,
                "schema": schema,
                "strict": strict,
            },
        }),
    }
}

fn common_openai_headers(accept: &str) -> Vec<NonSecretProviderHeader> {
    vec![
        NonSecretProviderHeader {
            name: "accept".to_owned(),
            value: accept.to_owned(),
            class: ProviderRequestHeaderClass::ProtocolFixed,
        },
        NonSecretProviderHeader {
            name: "content-type".to_owned(),
            value: "application/json".to_owned(),
            class: ProviderRequestHeaderClass::ContentMetadata,
        },
    ]
}

impl ModelProtocolDecoder for OpenAiChatCompletionsAdapter {
    fn api_format(&self) -> ModelApiFormat {
        ModelApiFormat::OpenAiChatCompletions
    }

    fn create_stream_decoder(
        &self,
        snapshot: &ModelExecutionSnapshot,
    ) -> Box<dyn ModelStreamDecoder> {
        Box::new(decoder::ChatDecoder::new(snapshot))
    }

    fn classify_http_error(&self, response: &ProviderErrorResponse) -> NormalizedModelFailure {
        provider_http_failure(response, HTTP_ERROR)
    }

    fn runtime_capability_facts(
        &self,
        request_plan: &ProviderRequestPlan,
    ) -> ModelRuntimeCapabilityFacts {
        let body = serde_json::from_slice::<Value>(&request_plan.body).ok();
        ModelRuntimeCapabilityFacts {
            api_format: ModelApiFormat::OpenAiChatCompletions,
            native_tool_channel_exposed: body
                .as_ref()
                .and_then(|value| value.get("tools"))
                .and_then(Value::as_array)
                .is_some_and(|tools| !tools.is_empty()),
            native_attachment_parts_present: body.as_ref().is_some_and(has_native_attachments),
            reasoning_channel_observed: body.as_ref().is_some_and(|value| {
                value.get("reasoning_effort").is_some() || value.get("reasoning").is_some()
            }),
            native_capability_names: Vec::new(),
        }
    }
}

/// The fields of a failed reply: an error names its code as `code`, or as `type`.
const HTTP_ERROR: ProviderErrorFields = ProviderErrorFields {
    code: &["code", "type"],
    fallback_code: "openai_chat_http_error",
    fallback_message: "OpenAI Chat Completions request failed",
};

fn has_native_attachments(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(has_native_attachments),
        Value::Object(object) => {
            object
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| matches!(kind, "image_url" | "input_audio" | "file"))
                || object.values().any(has_native_attachments)
        }
        _ => false,
    }
}
