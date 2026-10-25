use super::*;

pub(super) struct ResponsesDecoder {
    output: NormalizedOutputBuilder,
    assistant_blocks: BTreeMap<(u32, u32), ModelContentBlockId>,
    reasoning_blocks: BTreeMap<(u32, u32), ModelContentBlockId>,
    reasoning_text_blocks: BTreeMap<(u32, u32), PendingReasoningTextBlock>,
    refusal_blocks: BTreeMap<(u32, u32), PendingRefusalBlock>,
    continuation_items: BTreeMap<u32, Value>,
    tool_items: BTreeMap<String, PendingToolItem>,
    response_started: bool,
    terminal_seen: bool,
    done_marker_seen: bool,
    has_tool_calls: bool,
}

pub(super) struct PendingReasoningTextBlock {
    block_id: ModelContentBlockId,
    item_id: String,
    text: String,
    completed: bool,
}

pub(super) struct PendingRefusalBlock {
    item_id: String,
    text: String,
    completed: bool,
}

pub(super) struct PendingToolItem {
    provider_call_id: Option<ProviderToolCallId>,
    name: Option<String>,
    position: u32,
    arguments: String,
    started: bool,
    completed: bool,
}

impl ModelProtocolDecoder for OpenAiResponsesAdapter {
    fn api_format(&self) -> ModelApiFormat {
        ModelApiFormat::OpenAiResponses
    }

    /// A Responses function call is a `function_call` output item. The name is
    /// read from that item's `name` field and from nowhere else, so a refusal can
    /// name the exact Provider item kind that carried the illegal name.
    fn tool_call_item_type(&self) -> &'static str {
        "function_call"
    }

    fn create_stream_decoder(
        &self,
        snapshot: &ModelExecutionSnapshot,
    ) -> Box<dyn ModelStreamDecoder> {
        Box::new(ResponsesDecoder {
            output: NormalizedOutputBuilder::new(snapshot.model_step_id),
            assistant_blocks: BTreeMap::new(),
            reasoning_blocks: BTreeMap::new(),
            reasoning_text_blocks: BTreeMap::new(),
            refusal_blocks: BTreeMap::new(),
            continuation_items: BTreeMap::new(),
            tool_items: BTreeMap::new(),
            response_started: false,
            terminal_seen: false,
            done_marker_seen: false,
            has_tool_calls: false,
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
        let native_tool_channel_exposed = body
            .as_ref()
            .and_then(|value| value.get("tools"))
            .and_then(Value::as_array)
            .is_some_and(|tools| !tools.is_empty());
        let native_attachment_parts_present = body.as_ref().is_some_and(has_native_attachments);
        let reasoning_channel_observed =
            body.as_ref().is_some_and(|value| value.get("reasoning").is_some());
        ModelRuntimeCapabilityFacts {
            api_format: ModelApiFormat::OpenAiResponses,
            native_tool_channel_exposed,
            native_attachment_parts_present,
            reasoning_channel_observed,
            native_capability_names: Vec::new(),
        }
    }
}

impl ModelStreamDecoder for ResponsesDecoder {
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
                "openai_responses_event_invalid_json",
                "OpenAI Responses stream event is not valid JSON",
            )
        })?;
        let json_type = string_at(&value, &["type"]);
        let event_type = frame.event.as_deref().filter(|event| !event.is_empty()).or(json_type);
        if frame
            .event
            .as_deref()
            .is_some_and(|event| json_type.is_some_and(|json| !event.is_empty() && event != json))
        {
            return Err(ModelAdapterError::protocol(
                "openai_responses_event_type_mismatch",
                "SSE event name does not match the JSON event type",
            ));
        }
        let event_type = event_type.ok_or_else(|| {
            ModelAdapterError::protocol(
                "openai_responses_event_type_missing",
                "OpenAI Responses stream event has no type",
            )
        })?;
        match event_type {
            "response.created" | "response.in_progress" => self.response_started(&value),
            "response.output_text.delta" => self.output_text_delta(&value),
            "response.refusal.delta" => self.refusal_delta(&value),
            "response.refusal.done" => self.refusal_done(&value),
            "response.reasoning_text.delta" => self.reasoning_text_delta(&value),
            "response.reasoning_text.done" => self.reasoning_text_done(&value),
            "response.reasoning_summary_text.delta" => self.reasoning_summary_delta(&value),
            "response.output_item.added" => self.output_item_added(&value),
            "response.function_call_arguments.delta" => self.tool_arguments_delta(&value),
            "response.output_item.done" => self.output_item_done(&value),
            "response.completed" => self.response_completed(&value),
            "response.incomplete" => self.response_incomplete(&value),
            "error" => Err(provider_stream_error(&value)),
            "response.content_part.added"
            | "response.content_part.done"
            | "response.output_text.done"
            | "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done"
            | "response.reasoning_summary_text.done"
            | "response.function_call_arguments.done"
            | "response.queued" => Ok(Vec::new()),
            unknown => Ok(vec![ModelStreamEvent::ProviderWarning {
                code: Some("openai_responses_event_unknown".to_owned()),
                message: format!("unrecognized OpenAI Responses event {unknown}"),
            }]),
        }
    }

    fn finish(&mut self) -> Result<NormalizedModelOutput, ModelAdapterError> {
        if !self.terminal_seen {
            return Err(ModelAdapterError::protocol(
                "openai_responses_terminal_missing",
                "OpenAI Responses stream ended without response.completed or response.incomplete",
            ));
        }
        let _ = self.done_marker_seen;
        self.output.finish()
    }

    fn take_continuation_artifact(
        &mut self,
    ) -> Result<Option<ProviderContinuationArtifact>, ModelAdapterError> {
        if self.continuation_items.is_empty() {
            return Ok(None);
        }
        let items = self.continuation_items.values().cloned().collect::<Vec<_>>();
        let bytes = serde_json::to_vec(&json!({
            "format": "openai_responses_reasoning_items_v1",
            "items": items,
        }))
        .map_err(|_| {
            ModelAdapterError::protocol(
                "openai_responses_continuation_encode_failed",
                "OpenAI Responses continuation items could not be encoded",
            )
        })?;
        Ok(Some(ProviderContinuationArtifact {
            kind: ProviderContinuationArtifactKind::OpenAiResponses,
            provider_kind: None,
            media_type: "application/vnd.aura.openai-responses-continuation+json".to_owned(),
            bytes,
        }))
    }
}

mod terminal;
mod text;
mod tools;
