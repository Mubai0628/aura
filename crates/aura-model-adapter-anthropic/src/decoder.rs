//! Streaming events and signed provider continuation.
use super::*;

struct AnthropicDecoder {
    output: NormalizedOutputBuilder,
    blocks: BTreeMap<u32, AnthropicBlock>,
    provider_tool_ids: BTreeSet<ProviderToolCallId>,
    message_started: bool,
    message_stopped: bool,
    pending_stop_reason: Option<NormalizedStopReason>,
}

enum AnthropicBlock {
    Text { block_id: ModelContentBlockId, stopped: bool },
    Thinking { block_id: ModelContentBlockId, thinking: String, signature: String, stopped: bool },
    RedactedThinking { data: String, stopped: bool },
    Tool { provider_call_id: ProviderToolCallId, arguments: String, stopped: bool },
    Unsupported { stopped: bool },
}

impl ModelProtocolDecoder for AnthropicMessagesAdapter {
    fn api_format(&self) -> ModelApiFormat {
        ModelApiFormat::AnthropicMessages
    }

    fn create_stream_decoder(
        &self,
        snapshot: &ModelExecutionSnapshot,
    ) -> Box<dyn ModelStreamDecoder> {
        Box::new(AnthropicDecoder {
            output: NormalizedOutputBuilder::new(snapshot.model_step_id),
            blocks: BTreeMap::new(),
            provider_tool_ids: BTreeSet::new(),
            message_started: false,
            message_stopped: false,
            pending_stop_reason: None,
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
            api_format: ModelApiFormat::AnthropicMessages,
            native_tool_channel_exposed: body
                .as_ref()
                .and_then(|value| value.get("tools"))
                .and_then(Value::as_array)
                .is_some_and(|tools| !tools.is_empty()),
            native_attachment_parts_present: body.as_ref().is_some_and(has_native_attachments),
            reasoning_channel_observed: body
                .as_ref()
                .is_some_and(|value| value.get("thinking").is_some()),
            native_capability_names: Vec::new(),
        }
    }
}

impl ModelStreamDecoder for AnthropicDecoder {
    fn push_frame(
        &mut self,
        frame: ProviderStreamFrame,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        if frame.data == "[DONE]" {
            return Ok(vec![ModelStreamEvent::ProviderWarning {
                code: Some("anthropic_nonstandard_done_marker".to_owned()),
                message: "Anthropic-compatible endpoint emitted a nonstandard [DONE] marker"
                    .to_owned(),
            }]);
        }
        let value: Value = serde_json::from_str(&frame.data).map_err(|_| {
            ModelAdapterError::protocol(
                "anthropic_event_invalid_json",
                "Anthropic Messages stream event is not valid JSON",
            )
        })?;
        let json_type = value.get("type").and_then(Value::as_str);
        let event_type = frame.event.as_deref().filter(|event| !event.is_empty()).or(json_type);
        if frame
            .event
            .as_deref()
            .is_some_and(|event| json_type.is_some_and(|json| !event.is_empty() && event != json))
        {
            return Err(ModelAdapterError::protocol(
                "anthropic_event_type_mismatch",
                "Anthropic SSE event name does not match JSON type",
            ));
        }
        match event_type.ok_or_else(|| {
            ModelAdapterError::protocol(
                "anthropic_event_type_missing",
                "Anthropic Messages event has no type",
            )
        })? {
            "message_start" => self.message_start(&value),
            "content_block_start" => self.content_block_start(&value),
            "content_block_delta" => self.content_block_delta(&value),
            "content_block_stop" => self.content_block_stop(&value),
            "message_delta" => self.message_delta(&value),
            "message_stop" => self.message_stop(),
            "ping" => Ok(Vec::new()),
            "error" => Err(stream_error(&value)),
            unknown => Ok(vec![ModelStreamEvent::ProviderWarning {
                code: Some("anthropic_event_unknown".to_owned()),
                message: format!("unrecognized Anthropic Messages event {unknown}"),
            }]),
        }
    }

    fn finish(&mut self) -> Result<NormalizedModelOutput, ModelAdapterError> {
        if !self.message_stopped {
            return Err(ModelAdapterError::protocol(
                "anthropic_terminal_missing",
                "Anthropic Messages stream ended without message_stop",
            ));
        }
        if self.blocks.values().any(|block| !block.stopped()) {
            return Err(ModelAdapterError::protocol(
                "anthropic_content_block_incomplete",
                "Anthropic Messages stream ended with an open content block",
            ));
        }
        self.output.finish()
    }

    fn take_continuation_artifact(
        &mut self,
    ) -> Result<Option<ProviderContinuationArtifact>, ModelAdapterError> {
        let blocks = self
            .blocks
            .iter()
            .filter_map(|(index, block)| match block {
                AnthropicBlock::Thinking { thinking, signature, .. } => Some(json!({
                    "index": index,
                    "thinking": thinking,
                    "signature": signature,
                })),
                AnthropicBlock::RedactedThinking { data, .. } => {
                    Some(json!({"index":index,"data":data}))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if blocks.is_empty() {
            return Ok(None);
        }
        let bytes = serde_json::to_vec(&json!({
            "format": "anthropic_messages_thinking_v1",
            "blocks": blocks,
        }))
        .map_err(|_| {
            ModelAdapterError::protocol(
                "anthropic_continuation_encode_failed",
                "Anthropic thinking continuation could not be encoded",
            )
        })?;
        Ok(Some(ProviderContinuationArtifact {
            kind: ProviderContinuationArtifactKind::Anthropic,
            provider_kind: None,
            media_type: "application/vnd.aura.anthropic-continuation+json".to_owned(),
            bytes,
        }))
    }
}

impl AnthropicDecoder {
    fn message_start(&mut self, value: &Value) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        if std::mem::replace(&mut self.message_started, true) {
            return Err(ModelAdapterError::protocol(
                "anthropic_message_start_duplicate",
                "Anthropic Messages stream produced multiple message_start events",
            ));
        }
        let message = value.get("message").ok_or_else(|| {
            ModelAdapterError::protocol(
                "anthropic_message_missing",
                "message_start has no message object",
            )
        })?;
        let provider_request_id = message.get("id").and_then(Value::as_str).map(ToOwned::to_owned);
        self.output.set_provider_request_id(provider_request_id.clone());
        let usage = parse_usage(message.get("usage").unwrap_or(&Value::Null));
        self.output.update_usage(usage.clone());
        Ok(vec![
            ModelStreamEvent::RequestAccepted { provider_request_id },
            ModelStreamEvent::ResponseStarted,
            ModelStreamEvent::UsageUpdated { usage },
        ])
    }

    fn content_block_start(
        &mut self,
        value: &Value,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        self.ensure_message_started()?;
        let index = required_index(value)?;
        if self.blocks.contains_key(&index) {
            return Err(ModelAdapterError::protocol(
                "anthropic_content_block_duplicate",
                "Anthropic content block index is duplicated",
            ));
        }
        let content = value.get("content_block").ok_or_else(|| {
            ModelAdapterError::protocol(
                "anthropic_content_block_missing",
                "content_block_start has no content_block object",
            )
        })?;
        let kind = content.get("type").and_then(Value::as_str).ok_or_else(|| {
            ModelAdapterError::protocol(
                "anthropic_content_block_type_missing",
                "Anthropic content block has no type",
            )
        })?;
        match kind {
            "text" => {
                let block_id = self.output.start_assistant_block(Some(ProviderProtocolReference {
                    item_id: None,
                    block_id: None,
                    index: Some(index),
                }));
                self.blocks.insert(index, AnthropicBlock::Text { block_id, stopped: false });
                let mut events = vec![ModelStreamEvent::AssistantTextStarted { block_id }];
                if let Some(text) = content.get("text").and_then(Value::as_str)
                    && !text.is_empty()
                {
                    self.output.append_assistant(block_id, text)?;
                    events.push(ModelStreamEvent::AssistantTextDelta {
                        block_id,
                        text: text.to_owned(),
                    });
                }
                Ok(events)
            }
            "redacted_thinking" => {
                let data = content
                    .get("data")
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| {
                        ModelAdapterError::protocol(
                            "anthropic_redacted_thinking_missing",
                            "Redacted thinking has no opaque data",
                        )
                    })?;
                self.blocks.insert(
                    index,
                    AnthropicBlock::RedactedThinking { data: data.to_owned(), stopped: false },
                );
                Ok(vec![])
            }
            "thinking" => {
                let block_id = self.output.start_reasoning_block(Some(ProviderProtocolReference {
                    item_id: None,
                    block_id: None,
                    index: Some(index),
                }));
                let thinking =
                    content.get("thinking").and_then(Value::as_str).unwrap_or_default().to_owned();
                let signature =
                    content.get("signature").and_then(Value::as_str).unwrap_or_default().to_owned();
                self.blocks.insert(
                    index,
                    AnthropicBlock::Thinking {
                        block_id,
                        thinking: thinking.clone(),
                        signature,
                        stopped: false,
                    },
                );
                let mut events = vec![ModelStreamEvent::ReasoningStarted { block_id }];
                if !thinking.is_empty() {
                    self.output.append_reasoning(block_id, &thinking)?;
                    events.push(ModelStreamEvent::ReasoningDelta { block_id, text: thinking });
                }
                Ok(events)
            }
            "tool_use" => {
                let raw_id = content.get("id").and_then(Value::as_str).ok_or_else(|| {
                    ModelAdapterError::protocol(
                        "provider_tool_call_id_missing",
                        "Anthropic tool_use block has no provider ID",
                    )
                })?;
                let provider_call_id = ProviderToolCallId::new(raw_id).map_err(|_| {
                    ModelAdapterError::protocol(
                        "provider_tool_call_id_invalid",
                        "Anthropic tool_use block has a blank provider ID",
                    )
                })?;
                if !self.provider_tool_ids.insert(provider_call_id.clone()) {
                    return Err(ModelAdapterError::protocol(
                        "provider_tool_call_duplicate",
                        "Anthropic response duplicated a provider tool call ID",
                    ));
                }
                let name = content
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|name| !name.trim().is_empty())
                    .ok_or_else(|| {
                        ModelAdapterError::protocol(
                            "provider_tool_name_missing",
                            "Anthropic tool_use block has no tool name",
                        )
                    })?
                    .to_owned();
                self.output.start_tool_call(provider_call_id.clone(), name.clone(), index)?;
                let mut arguments = String::new();
                if let Some(input) = content.get("input")
                    && input.as_object().is_some_and(|object| !object.is_empty())
                {
                    arguments = serde_json::to_string(input).map_err(|_| {
                        ModelAdapterError::protocol(
                            "anthropic_tool_input_invalid",
                            "Anthropic tool_use input could not be encoded",
                        )
                    })?;
                    self.output.append_tool_arguments(&provider_call_id, &arguments)?;
                }
                self.blocks.insert(
                    index,
                    AnthropicBlock::Tool {
                        provider_call_id: provider_call_id.clone(),
                        arguments: arguments.clone(),
                        stopped: false,
                    },
                );
                let mut events = vec![ModelStreamEvent::ToolCallStarted {
                    provider_call_id: provider_call_id.clone(),
                    tool_name: name,
                    position: index,
                }];
                if !arguments.is_empty() {
                    events.push(ModelStreamEvent::ToolCallArgumentsDelta {
                        provider_call_id,
                        delta: arguments,
                    });
                }
                Ok(events)
            }
            unsupported => {
                self.blocks.insert(index, AnthropicBlock::Unsupported { stopped: false });
                Ok(vec![ModelStreamEvent::ProviderWarning {
                    code: Some("anthropic_content_block_unknown".to_owned()),
                    message: format!("unrecognized Anthropic content block {unsupported}"),
                }])
            }
        }
    }

    fn content_block_delta(
        &mut self,
        value: &Value,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        let index = required_index(value)?;
        let delta = value.get("delta").ok_or_else(|| {
            ModelAdapterError::protocol(
                "anthropic_delta_missing",
                "content_block_delta has no delta object",
            )
        })?;
        let kind = delta.get("type").and_then(Value::as_str).ok_or_else(|| {
            ModelAdapterError::protocol(
                "anthropic_delta_type_missing",
                "Anthropic content delta has no type",
            )
        })?;
        let block = self.blocks.get_mut(&index).ok_or_else(|| {
            ModelAdapterError::protocol(
                "anthropic_content_block_unknown",
                "content delta references an unknown block index",
            )
        })?;
        if block.stopped() {
            return Err(ModelAdapterError::protocol(
                "anthropic_content_block_stopped",
                "content delta arrived after block_stop",
            ));
        }
        match (kind, block) {
            ("text_delta", AnthropicBlock::Text { block_id, .. }) => {
                let text = required_delta_string(delta, "text")?;
                self.output.append_assistant(*block_id, text)?;
                Ok(vec![ModelStreamEvent::AssistantTextDelta {
                    block_id: *block_id,
                    text: text.to_owned(),
                }])
            }
            ("thinking_delta", AnthropicBlock::Thinking { block_id, thinking, .. }) => {
                let delta_text = required_delta_string(delta, "thinking")?;
                thinking.push_str(delta_text);
                self.output.append_reasoning(*block_id, delta_text)?;
                Ok(vec![ModelStreamEvent::ReasoningDelta {
                    block_id: *block_id,
                    text: delta_text.to_owned(),
                }])
            }
            ("input_json_delta", AnthropicBlock::Tool { provider_call_id, arguments, .. }) => {
                let partial_json = required_delta_string(delta, "partial_json")?;
                arguments.push_str(partial_json);
                self.output.append_tool_arguments(provider_call_id, partial_json)?;
                Ok(vec![ModelStreamEvent::ToolCallArgumentsDelta {
                    provider_call_id: provider_call_id.clone(),
                    delta: partial_json.to_owned(),
                }])
            }
            ("signature_delta", AnthropicBlock::Thinking { signature, .. }) => {
                signature.push_str(required_delta_string(delta, "signature")?);
                Ok(Vec::new())
            }
            ("citations_delta", AnthropicBlock::Text { .. }) => {
                Ok(vec![ModelStreamEvent::ProviderWarning {
                    code: Some("anthropic_citation_delta_unmapped".to_owned()),
                    message: "Anthropic citation delta is preserved in the raw stream".to_owned(),
                }])
            }
            (unknown, _) => Ok(vec![ModelStreamEvent::ProviderWarning {
                code: Some("anthropic_delta_unknown".to_owned()),
                message: format!("unrecognized or mismatched Anthropic delta {unknown}"),
            }]),
        }
    }

    fn content_block_stop(
        &mut self,
        value: &Value,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        let index = required_index(value)?;
        let block = self.blocks.get_mut(&index).ok_or_else(|| {
            ModelAdapterError::protocol(
                "anthropic_content_block_unknown",
                "content_block_stop references an unknown block index",
            )
        })?;
        if block.stopped() {
            return Err(ModelAdapterError::protocol(
                "anthropic_content_block_stop_duplicate",
                "Anthropic content block stopped more than once",
            ));
        }
        if let AnthropicBlock::Thinking { signature, .. } = block
            && signature.trim().is_empty()
        {
            return Err(ModelAdapterError::protocol(
                "anthropic_thinking_signature_missing",
                "Anthropic thinking block stopped without its integrity signature",
            ));
        }
        block.set_stopped();
        if let AnthropicBlock::Tool { provider_call_id, arguments, .. } = block {
            let mut events = Vec::new();
            if arguments.is_empty() {
                arguments.push_str("{}");
                self.output.append_tool_arguments(provider_call_id, arguments)?;
                events.push(ModelStreamEvent::ToolCallArgumentsDelta {
                    provider_call_id: provider_call_id.clone(),
                    delta: arguments.clone(),
                });
            }
            self.output.complete_tool_call(provider_call_id)?;
            events.push(ModelStreamEvent::ToolCallCompleted {
                provider_call_id: provider_call_id.clone(),
            });
            return Ok(events);
        }
        Ok(Vec::new())
    }

    fn message_delta(&mut self, value: &Value) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        self.ensure_message_started()?;
        if self.pending_stop_reason.is_some() {
            return Err(ModelAdapterError::protocol(
                "anthropic_message_delta_duplicate",
                "Anthropic Messages stream produced multiple terminal message deltas",
            ));
        }
        let delta = value.get("delta").ok_or_else(|| {
            ModelAdapterError::protocol(
                "anthropic_message_delta_missing",
                "message_delta has no delta object",
            )
        })?;
        let stop_reason = delta
            .get("stop_reason")
            .and_then(Value::as_str)
            .map(map_stop_reason)
            .unwrap_or(NormalizedStopReason::Unknown);
        let mut events = Vec::new();
        if stop_reason == NormalizedStopReason::Refusal {
            let stop_details =
                delta.get("stop_details").and_then(Value::as_object).ok_or_else(|| {
                    ModelAdapterError::protocol(
                        "anthropic_refusal_details_missing",
                        "Anthropic refusal stop reason has no stop_details object",
                    )
                })?;
            if stop_details.get("type").and_then(Value::as_str) != Some("refusal") {
                return Err(ModelAdapterError::protocol(
                    "anthropic_refusal_details_invalid",
                    "Anthropic refusal stop_details does not have type refusal",
                ));
            }
            let provider_code =
                optional_nullable_string(stop_details, "category")?.map(str::to_owned);
            let explanation = optional_nullable_string(stop_details, "explanation")?;
            self.output.mark_refusal(provider_code);
            if let Some(explanation) = explanation
                && !explanation.is_empty()
            {
                self.output.append_refusal(explanation);
                events.push(ModelStreamEvent::RefusalDelta { text: explanation.to_owned() });
            }
        }
        self.pending_stop_reason = Some(stop_reason);
        let usage = parse_usage(value.get("usage").unwrap_or(&Value::Null));
        self.output.update_usage(usage.clone());
        events.push(ModelStreamEvent::UsageUpdated { usage });
        Ok(events)
    }

    fn message_stop(&mut self) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        if std::mem::replace(&mut self.message_stopped, true) {
            return Err(ModelAdapterError::protocol(
                "anthropic_message_stop_duplicate",
                "Anthropic Messages stream produced multiple message_stop events",
            ));
        }
        if self.blocks.values().any(|block| !block.stopped()) {
            return Err(ModelAdapterError::protocol(
                "anthropic_content_block_incomplete",
                "message_stop arrived while a content block was still open",
            ));
        }
        let stop_reason = self.pending_stop_reason.ok_or_else(|| {
            ModelAdapterError::protocol(
                "anthropic_stop_reason_missing",
                "message_stop arrived before a stop_reason",
            )
        })?;
        self.output.complete_response(stop_reason)?;
        Ok(vec![ModelStreamEvent::ResponseCompleted { stop_reason }])
    }

    fn ensure_message_started(&self) -> Result<(), ModelAdapterError> {
        if self.message_started {
            Ok(())
        } else {
            Err(ModelAdapterError::protocol(
                "anthropic_message_start_missing",
                "Anthropic content event arrived before message_start",
            ))
        }
    }
}

impl AnthropicBlock {
    const fn stopped(&self) -> bool {
        match self {
            Self::Text { stopped, .. }
            | Self::Thinking { stopped, .. }
            | Self::RedactedThinking { stopped, .. }
            | Self::Tool { stopped, .. }
            | Self::Unsupported { stopped, .. } => *stopped,
        }
    }

    fn set_stopped(&mut self) {
        match self {
            Self::Text { stopped, .. }
            | Self::Thinking { stopped, .. }
            | Self::RedactedThinking { stopped, .. }
            | Self::Tool { stopped, .. }
            | Self::Unsupported { stopped, .. } => *stopped = true,
        }
    }
}
