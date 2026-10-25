#![forbid(unsafe_code)]
mod history;

use std::collections::{BTreeMap, BTreeSet};

use aura_domain_common::ModelContentBlockId;
use aura_model_domain::{
    ChatInstructionRole, ChatOutputTokenField, LogicalConversationItem, ModelApiFormat,
    ModelExecutionSnapshot, ModelFailureCategory, ModelOutputFormat, ModelRetryClass,
    ModelRuntimeCapabilityFacts, ModelStreamEvent, NormalizedModelFailure, NormalizedModelOutput,
    NormalizedStopReason, PartialModelUsage, ProviderProtocolReference, ProviderRequestHeaderClass,
    ProviderToolCallId,
};
use aura_model_transport::{
    MODEL_GENERATION_PROBE_MAX_OUTPUT_TOKENS, MODEL_GENERATION_PROBE_PROMPT, ModelAdapterError,
    ModelAvailabilityProbeCompilationInput, ModelGenerationProbeProtocol, ModelProtocolCompiler,
    ModelProtocolDecoder, ModelRequestCompilationInput, ModelStreamDecoder,
    NonSecretProviderHeader, NormalizedOutputBuilder, ProviderErrorResponse, ProviderRequestPlan,
    ProviderStreamFrame, build_generation_probe_request_plan, build_provider_request_plan,
    historical_tool_call_wire_bindings, provider_tool_name_bindings, retry_after_milliseconds,
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
        messages.extend(history::compile_chat_history(
            input.request,
            input.snapshot,
            input.resolved_attachments,
        )?);
        messages
            .push(json!({"role":chat_instruction_role(instruction_role),"content":layout.current}));

        let mut body = Map::new();
        body.insert("model".to_owned(), Value::String(input.snapshot.model_name.clone()));
        if let Some(key) = aura_model_transport::openai_prompt_cache_key(input.snapshot, &layout) {
            body.insert("prompt_cache_key".to_owned(), Value::String(key));
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
        if choices.len() != 1
            || choices[0].get("index").and_then(Value::as_u64) != Some(0)
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

struct ChatDecoder {
    output: NormalizedOutputBuilder,
    assistant_block: Option<ModelContentBlockId>,
    reasoning_block: Option<ModelContentBlockId>,
    tool_calls: BTreeMap<u32, PendingChatToolCall>,
    provider_tool_ids: BTreeSet<ProviderToolCallId>,
    refusal_seen: bool,
    response_started: bool,
    terminal_seen: bool,
    done_marker_seen: bool,
}

struct PendingChatToolCall {
    provider_call_id: Option<ProviderToolCallId>,
    name: Option<String>,
    buffered_arguments: String,
    started: bool,
    completed: bool,
    thought_signature: Option<String>,
}

impl ModelProtocolDecoder for OpenAiChatCompletionsAdapter {
    fn api_format(&self) -> ModelApiFormat {
        ModelApiFormat::OpenAiChatCompletions
    }

    fn create_stream_decoder(
        &self,
        snapshot: &ModelExecutionSnapshot,
    ) -> Box<dyn ModelStreamDecoder> {
        Box::new(ChatDecoder {
            output: NormalizedOutputBuilder::new(snapshot.model_step_id),
            assistant_block: None,
            reasoning_block: None,
            tool_calls: BTreeMap::new(),
            provider_tool_ids: BTreeSet::new(),
            refusal_seen: false,
            response_started: false,
            terminal_seen: false,
            done_marker_seen: false,
        })
    }

    fn classify_http_error(&self, response: &ProviderErrorResponse) -> NormalizedModelFailure {
        classify_http_error(response)
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

impl ModelStreamDecoder for ChatDecoder {
    fn push_frame(
        &mut self,
        frame: ProviderStreamFrame,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        if frame.data == "[DONE]" {
            self.done_marker_seen = true;
            return Ok(Vec::new());
        }
        let value: Value = serde_json::from_str(&frame.data).map_err(|_| {
            ModelAdapterError::protocol(
                "openai_chat_chunk_invalid_json",
                "OpenAI Chat Completions stream chunk is not valid JSON",
            )
        })?;
        if value.get("error").is_some() {
            return Err(stream_error(&value));
        }
        let mut events = self.start_response_if_needed(&value);
        if let Some(usage) = value.get("usage") {
            let partial = parse_usage(usage);
            self.output.update_usage(partial.clone());
            events.push(ModelStreamEvent::UsageUpdated { usage: partial });
        }
        let choices = value.get("choices").and_then(Value::as_array).ok_or_else(|| {
            ModelAdapterError::protocol(
                "openai_chat_choices_missing",
                "OpenAI Chat Completions chunk has no choices array",
            )
        })?;
        if choices.is_empty() {
            return Ok(events);
        }
        if choices.len() != 1 {
            return Err(ModelAdapterError::protocol(
                "openai_chat_multiple_choices_unsupported",
                "OpenAI Chat Completions returned multiple choices; Aura will not merge them",
            ));
        }
        let choice = &choices[0];
        if choice.get("index").and_then(Value::as_u64) != Some(0) {
            return Err(ModelAdapterError::protocol(
                "openai_chat_choice_index_unsupported",
                "OpenAI Chat Completions only supports choice index 0 in P05",
            ));
        }
        if let Some(delta) = choice.get("delta") {
            self.apply_delta(delta, &mut events)?;
        }
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.complete_tool_calls(&mut events)?;
            let stop_reason = if reason == "stop" && self.refusal_seen {
                NormalizedStopReason::Refusal
            } else {
                map_stop_reason(reason)
            };
            if std::mem::replace(&mut self.terminal_seen, true) {
                return Err(ModelAdapterError::protocol(
                    "openai_chat_terminal_duplicate",
                    "OpenAI Chat Completions stream produced multiple finish reasons",
                ));
            }
            self.output.complete_response(stop_reason)?;
            events.push(ModelStreamEvent::ResponseCompleted { stop_reason });
        }
        Ok(events)
    }

    fn finish(&mut self) -> Result<NormalizedModelOutput, ModelAdapterError> {
        if !self.terminal_seen {
            return Err(ModelAdapterError::protocol(
                "openai_chat_terminal_missing",
                "OpenAI Chat Completions stream ended without finish_reason",
            ));
        }
        if !self.done_marker_seen {
            return Err(ModelAdapterError::protocol(
                "openai_chat_done_missing",
                "OpenAI Chat Completions stream ended without [DONE]",
            ));
        }
        self.output.finish()
    }

    fn take_continuation_artifact(
        &mut self,
    ) -> Result<Option<aura_model_transport::ProviderContinuationArtifact>, ModelAdapterError> {
        let signatures = self
            .tool_calls
            .values()
            .filter_map(|c| {
                c.provider_call_id
                    .as_ref()
                    .zip(c.thought_signature.as_ref())
                    .map(|(id, signature)| (id.as_str().to_owned(), signature.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        if signatures.is_empty() {
            return Ok(None);
        }
        let bytes = serde_json::to_vec(
            &json!({"format":"openai_chat_continuation_v1","thought_signatures":signatures}),
        )
        .map_err(|_| {
            ModelAdapterError::protocol(
                "openai_chat_continuation_encode_failed",
                "Unable to retain thought signatures",
            )
        })?;
        Ok(Some(aura_model_transport::ProviderContinuationArtifact {
            kind: aura_model_transport::ProviderContinuationArtifactKind::Opaque,
            provider_kind: Some("openai_chat".into()),
            media_type: "application/vnd.aura.openai-chat-continuation+json".into(),
            bytes,
        }))
    }
}

impl ChatDecoder {
    fn start_response_if_needed(&mut self, value: &Value) -> Vec<ModelStreamEvent> {
        if self.response_started {
            return Vec::new();
        }
        self.response_started = true;
        let provider_request_id = value.get("id").and_then(Value::as_str).map(ToOwned::to_owned);
        self.output.set_provider_request_id(provider_request_id.clone());
        vec![
            ModelStreamEvent::RequestAccepted { provider_request_id },
            ModelStreamEvent::ResponseStarted,
        ]
    }

    fn apply_delta(
        &mut self,
        delta: &Value,
        events: &mut Vec<ModelStreamEvent>,
    ) -> Result<(), ModelAdapterError> {
        if let Some(text) = delta.get("content").and_then(Value::as_str) {
            let block_id = if let Some(block_id) = self.assistant_block {
                block_id
            } else {
                let block_id = self.output.start_assistant_block(Some(ProviderProtocolReference {
                    item_id: None,
                    block_id: None,
                    index: Some(0),
                }));
                self.assistant_block = Some(block_id);
                events.push(ModelStreamEvent::AssistantTextStarted { block_id });
                block_id
            };
            self.output.append_assistant(block_id, text)?;
            events.push(ModelStreamEvent::AssistantTextDelta { block_id, text: text.to_owned() });
        }
        if let Some(reasoning) = delta.get("reasoning_content").and_then(Value::as_str) {
            let block_id = if let Some(block_id) = self.reasoning_block {
                block_id
            } else {
                let block_id = self.output.start_reasoning_block(Some(ProviderProtocolReference {
                    item_id: None,
                    block_id: None,
                    index: Some(0),
                }));
                self.reasoning_block = Some(block_id);
                events.push(ModelStreamEvent::ReasoningStarted { block_id });
                block_id
            };
            self.output.append_reasoning(block_id, reasoning)?;
            events.push(ModelStreamEvent::ReasoningDelta { block_id, text: reasoning.to_owned() });
        }
        if let Some(refusal) = delta.get("refusal").and_then(Value::as_str) {
            self.refusal_seen = true;
            self.output.append_refusal(refusal);
            events.push(ModelStreamEvent::RefusalDelta { text: refusal.to_owned() });
        }
        if let Some(tool_calls) = delta.get("tool_calls") {
            let tool_calls = tool_calls.as_array().ok_or_else(|| {
                ModelAdapterError::protocol(
                    "openai_chat_tool_calls_invalid",
                    "OpenAI Chat Completions tool_calls delta is not an array",
                )
            })?;
            for call in tool_calls {
                self.apply_tool_delta(call, events)?;
            }
        }
        for key in ["reasoning", "thinking"] {
            if delta.get(key).is_some() {
                events.push(ModelStreamEvent::ProviderWarning {
                    code: Some("openai_chat_reasoning_extension_unmapped".to_owned()),
                    message: format!("unmapped compatible-endpoint field delta.{key}"),
                });
            }
        }
        Ok(())
    }

    fn apply_tool_delta(
        &mut self,
        value: &Value,
        events: &mut Vec<ModelStreamEvent>,
    ) -> Result<(), ModelAdapterError> {
        let index = value
            .get("index")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| {
                ModelAdapterError::protocol(
                    "openai_chat_tool_index_missing",
                    "OpenAI Chat Completions tool delta has no valid index",
                )
            })?;
        let pending = self.tool_calls.entry(index).or_insert(PendingChatToolCall {
            provider_call_id: None,
            name: None,
            buffered_arguments: String::new(),
            started: false,
            completed: false,
            thought_signature: None,
        });
        if pending.completed {
            return Err(ModelAdapterError::protocol(
                "openai_chat_tool_completed",
                "tool delta arrived after the call completed",
            ));
        }
        if let Some(signature) =
            value.pointer("/extra_content/google/thought_signature").and_then(Value::as_str)
        {
            if pending.thought_signature.as_deref().is_some_and(|old| old != signature) {
                return Err(ModelAdapterError::protocol(
                    "openai_chat_thought_signature_changed",
                    "Thought signature changed within one call",
                ));
            }
            pending.thought_signature = Some(signature.to_owned());
        }
        if let Some(raw_id) = value.get("id").and_then(Value::as_str) {
            let id = ProviderToolCallId::new(raw_id).map_err(|_| {
                ModelAdapterError::protocol(
                    "provider_tool_call_id_invalid",
                    "OpenAI Chat Completions tool call ID is blank",
                )
            })?;
            if pending.provider_call_id.as_ref().is_some_and(|current| current != &id) {
                return Err(ModelAdapterError::protocol(
                    "openai_chat_tool_identity_changed",
                    "tool call ID changed for one choice/index",
                ));
            }
            if pending.provider_call_id.is_none() && !self.provider_tool_ids.insert(id.clone()) {
                return Err(ModelAdapterError::protocol(
                    "provider_tool_call_duplicate",
                    "OpenAI Chat Completions duplicated a provider tool call ID",
                ));
            }
            pending.provider_call_id = Some(id);
        }
        if let Some(function) = value.get("function") {
            if let Some(name) = function.get("name").and_then(Value::as_str) {
                if name.trim().is_empty() {
                    return Err(ModelAdapterError::protocol(
                        "provider_tool_name_missing",
                        "OpenAI Chat Completions tool name is blank",
                    ));
                }
                if pending.name.as_ref().is_some_and(|current| current != name) {
                    return Err(ModelAdapterError::protocol(
                        "openai_chat_tool_name_changed",
                        "tool name changed for one choice/index",
                    ));
                }
                pending.name = Some(name.to_owned());
            }
            if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                pending.buffered_arguments.push_str(arguments);
            }
        }
        if !pending.started
            && let (Some(provider_call_id), Some(name)) =
                (pending.provider_call_id.clone(), pending.name.clone())
        {
            self.output.start_tool_call(provider_call_id.clone(), name.clone(), index)?;
            pending.started = true;
            events.push(ModelStreamEvent::ToolCallStarted {
                provider_call_id: provider_call_id.clone(),
                tool_name: name,
                position: index,
            });
            if !pending.buffered_arguments.is_empty() {
                self.output
                    .append_tool_arguments(&provider_call_id, &pending.buffered_arguments)?;
                events.push(ModelStreamEvent::ToolCallArgumentsDelta {
                    provider_call_id,
                    delta: pending.buffered_arguments.clone(),
                });
            }
        } else if pending.started
            && let Some(arguments) = value
                .get("function")
                .and_then(|function| function.get("arguments"))
                .and_then(Value::as_str)
            && !arguments.is_empty()
        {
            let provider_call_id = pending.provider_call_id.as_ref().ok_or_else(|| {
                ModelAdapterError::protocol(
                    "provider_tool_call_id_missing",
                    "started tool call lost its provider ID",
                )
            })?;
            self.output.append_tool_arguments(provider_call_id, arguments)?;
            events.push(ModelStreamEvent::ToolCallArgumentsDelta {
                provider_call_id: provider_call_id.clone(),
                delta: arguments.to_owned(),
            });
        }
        Ok(())
    }

    fn complete_tool_calls(
        &mut self,
        events: &mut Vec<ModelStreamEvent>,
    ) -> Result<(), ModelAdapterError> {
        for call in self.tool_calls.values_mut() {
            if !call.started {
                return Err(ModelAdapterError::protocol(
                    "provider_tool_call_id_missing",
                    "OpenAI Chat Completions response ended before a tool call had ID and name",
                ));
            }
            let provider_call_id = call.provider_call_id.as_ref().ok_or_else(|| {
                ModelAdapterError::protocol(
                    "provider_tool_call_id_missing",
                    "OpenAI Chat Completions tool call has no provider ID",
                )
            })?;
            self.output.complete_tool_call(provider_call_id)?;
            call.completed = true;
            events.push(ModelStreamEvent::ToolCallCompleted {
                provider_call_id: provider_call_id.clone(),
            });
        }
        Ok(())
    }
}

fn parse_usage(value: &Value) -> PartialModelUsage {
    PartialModelUsage {
        input_tokens: value.get("prompt_tokens").and_then(Value::as_u64),
        output_tokens: value.get("completion_tokens").and_then(Value::as_u64),
        total_tokens: value.get("total_tokens").and_then(Value::as_u64),
        reasoning_tokens: value
            .pointer("/completion_tokens_details/reasoning_tokens")
            .and_then(Value::as_u64),
        cached_input_tokens: value
            .pointer("/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_u64)
            .or_else(|| value.get("prompt_cache_hit_tokens").and_then(Value::as_u64)),
        cache_write_tokens: None,
    }
}

fn map_stop_reason(reason: &str) -> NormalizedStopReason {
    match reason {
        "stop" => NormalizedStopReason::EndTurn,
        "tool_calls" | "function_call" => NormalizedStopReason::ToolCalls,
        "length" => NormalizedStopReason::MaxOutputTokens,
        "content_filter" => NormalizedStopReason::ContentFilter,
        _ => NormalizedStopReason::Unknown,
    }
}

fn stream_error(value: &Value) -> ModelAdapterError {
    let error = value.get("error").unwrap_or(value);
    ModelAdapterError::protocol(
        error
            .get("code")
            .or_else(|| error.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("openai_chat_stream_error"),
        error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("OpenAI Chat Completions stream reported an error"),
    )
}

fn classify_http_error(response: &ProviderErrorResponse) -> NormalizedModelFailure {
    let value = serde_json::from_slice::<Value>(&response.body).ok();
    let error = value.as_ref().and_then(|value| value.get("error")).or(value.as_ref());
    let code = error
        .and_then(|value| value.get("code").or_else(|| value.get("type")).and_then(Value::as_str))
        .unwrap_or("openai_chat_http_error")
        .to_owned();
    let message = error
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("OpenAI Chat Completions request failed")
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
    if matches!(status, 408 | 500 | 502 | 503 | 504) {
        return (
            ModelFailureCategory::Transport,
            ModelRetryClass::TransparentRetry { retry_after_ms: None },
        );
    }
    if contains_any(lower, &["context_length", "context window", "too many tokens"]) {
        return (ModelFailureCategory::ContextWindowExceeded, ModelRetryClass::NotRetryable);
    }
    if contains_any(lower, &["model_not_found", "unknown model"]) {
        return (ModelFailureCategory::UnsupportedModel, ModelRetryClass::NotRetryable);
    }
    if error.and_then(|value| value.get("param")).and_then(Value::as_str).is_some()
        && contains_any(lower, &["unsupported", "unknown parameter", "not supported"])
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
                .is_some_and(|kind| matches!(kind, "image_url" | "input_audio" | "file"))
                || object.values().any(has_native_attachments)
        }
        _ => false,
    }
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}
