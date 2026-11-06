//! What the decoder has seen, and the chunk loop: which chunk starts the response, which one ends
//! it, and what may still follow the end.
use std::collections::{BTreeMap, BTreeSet};

use aura_domain_common::ModelContentBlockId;
use aura_model_domain::{
    ModelExecutionSnapshot, ModelStreamEvent, NormalizedModelOutput, NormalizedStopReason,
    ProviderToolCallId,
};
use aura_model_transport::{
    ModelAdapterError, ModelStreamDecoder, NormalizedOutputBuilder, ProviderErrorFields,
    ProviderStreamFrame, provider_stream_failure,
};
use serde_json::{Value, json};

use super::{delta, usage};

pub(crate) struct ChatDecoder {
    pub(super) output: NormalizedOutputBuilder,
    pub(super) assistant_block: Option<ModelContentBlockId>,
    pub(super) reasoning_block: Option<ModelContentBlockId>,
    pub(super) tool_calls: BTreeMap<u32, PendingChatToolCall>,
    pub(super) provider_tool_ids: BTreeSet<ProviderToolCallId>,
    pub(super) refusal_seen: bool,
    response_started: bool,
    /// The first finish reason. Some services name one again in a later chunk, usually the one
    /// with the usage; it does not end the response a second time.
    finish_reason: Option<String>,
}

pub(super) struct PendingChatToolCall {
    pub(super) provider_call_id: Option<ProviderToolCallId>,
    pub(super) name: Option<String>,
    pub(super) buffered_arguments: String,
    pub(super) started: bool,
    pub(super) completed: bool,
    pub(super) thought_signature: Option<String>,
}

impl ChatDecoder {
    pub(crate) fn new(snapshot: &ModelExecutionSnapshot) -> Self {
        Self {
            output: NormalizedOutputBuilder::new(snapshot.model_step_id),
            assistant_block: None,
            reasoning_block: None,
            tool_calls: BTreeMap::new(),
            provider_tool_ids: BTreeSet::new(),
            refusal_seen: false,
            response_started: false,
            finish_reason: None,
        }
    }

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

    fn record_usage(&mut self, usage: &Value, events: &mut Vec<ModelStreamEvent>) {
        let partial = usage::parse_usage(usage);
        self.output.update_usage(partial.clone());
        events.push(ModelStreamEvent::UsageUpdated { usage: partial });
    }

    fn finish_response(
        &mut self,
        reason: &str,
        events: &mut Vec<ModelStreamEvent>,
    ) -> Result<(), ModelAdapterError> {
        if let Some(first) = &self.finish_reason {
            if first != reason {
                events.push(ModelStreamEvent::ProviderWarning {
                    code: Some("openai_chat_finish_reason_changed".to_owned()),
                    message: format!("a later chunk named finish reason {reason} after {first}"),
                });
            }
            return Ok(());
        }
        self.complete_tool_calls(events)?;
        let stop_reason = if reason == "stop" && self.refusal_seen {
            NormalizedStopReason::Refusal
        } else {
            map_stop_reason(reason)
        };
        self.finish_reason = Some(reason.to_owned());
        self.output.complete_response(stop_reason)?;
        events.push(ModelStreamEvent::ResponseCompleted { stop_reason });
        Ok(())
    }
}

impl ModelStreamDecoder for ChatDecoder {
    fn push_frame(
        &mut self,
        frame: ProviderStreamFrame,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        if frame.data == "[DONE]" {
            return Ok(Vec::new());
        }
        let value: Value = serde_json::from_str(&frame.data).map_err(|_| {
            ModelAdapterError::protocol(
                "openai_chat_chunk_invalid_json",
                "OpenAI Chat Completions stream chunk is not valid JSON",
            )
        })?;
        // Some services send `"error": null` with every chunk.
        if value.get("error").is_some_and(|error| !error.is_null()) {
            return Err(provider_stream_failure(&value, STREAM_ERROR));
        }
        let mut events = self.start_response_if_needed(&value);
        if let Some(usage) = value.get("usage").filter(|usage| !usage.is_null()) {
            self.record_usage(usage, &mut events);
        }
        // A chunk of usage alone may come without any choices.
        let choices = match value.get("choices") {
            None | Some(Value::Null) => return Ok(events),
            Some(choices) => choices.as_array().ok_or_else(|| {
                ModelAdapterError::protocol(
                    "openai_chat_choices_invalid",
                    "OpenAI Chat Completions chunk choices is not an array",
                )
            })?,
        };
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
        // The one choice there is may come without its index.
        if !choice.get("index").is_none_or(|index| index.is_null() || index.as_u64() == Some(0)) {
            return Err(ModelAdapterError::protocol(
                "openai_chat_choice_index_unsupported",
                "OpenAI Chat Completions only supports choice index 0 in P05",
            ));
        }
        if let Some(usage) = choice.get("usage").filter(|usage| !usage.is_null()) {
            self.record_usage(usage, &mut events);
        }
        if let Some(delta) = choice.get("delta") {
            if self.finish_reason.is_none() {
                self.apply_delta(delta, &mut events)?;
            } else if delta::carries_content(delta) {
                // Taken after the end, it would be missing from what was already completed.
                return Err(ModelAdapterError::protocol(
                    "openai_chat_content_after_finish",
                    "OpenAI Chat Completions stream sent content after its finish reason",
                ));
            }
        }
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish_response(reason, &mut events)?;
        }
        Ok(events)
    }

    fn finish(&mut self) -> Result<NormalizedModelOutput, ModelAdapterError> {
        // `[DONE]` is not required: some services close the stream after the finish reason.
        if self.finish_reason.is_none() {
            return Err(ModelAdapterError::protocol(
                "openai_chat_terminal_missing",
                "OpenAI Chat Completions stream ended without finish_reason",
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

fn map_stop_reason(reason: &str) -> NormalizedStopReason {
    match reason {
        "stop" => NormalizedStopReason::EndTurn,
        "tool_calls" | "function_call" => NormalizedStopReason::ToolCalls,
        "length" => NormalizedStopReason::MaxOutputTokens,
        "content_filter" => NormalizedStopReason::ContentFilter,
        _ => NormalizedStopReason::Unknown,
    }
}

/// The fields of an error chunk, laid out as a failed reply's body.
const STREAM_ERROR: ProviderErrorFields = ProviderErrorFields {
    fallback_code: "openai_chat_stream_error",
    fallback_message: "OpenAI Chat Completions stream reported an error",
    ..crate::HTTP_ERROR
};
