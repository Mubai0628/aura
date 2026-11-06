//! One chunk's `delta`: visible text, thinking, a refusal and tool-call fragments.
use aura_domain_common::ModelContentBlockId;
use aura_model_domain::{ModelStreamEvent, ProviderProtocolReference};
use aura_model_transport::ModelAdapterError;
use serde_json::Value;

use super::ChatDecoder;

/// The field compatible services most often send thinking in, and the only one Aura sends back
/// in the next request's history.
const REASONING_CONTENT: &str = "reasoning_content";

/// The single-text thinking fields, in the order they are read: a service that sends two of them
/// sends the same text twice.
const SCALAR_REASONING: [&str; 3] = [REASONING_CONTENT, "reasoning", "reasoning_text"];

/// The thinking one delta carries, and the field it came in.
#[derive(Default)]
struct Thinking {
    field: &'static str,
    text: String,
    summary: String,
}

impl Thinking {
    fn is_empty(&self) -> bool {
        self.text.is_empty() && self.summary.is_empty()
    }
}

/// A scalar field when there is one, else the text and summaries of `reasoning_details` (the
/// structured form some routers send beside or instead of it). An encrypted detail has no text.
fn thinking(delta: &Value) -> Thinking {
    for field in SCALAR_REASONING {
        if let Some(text) = delta.get(field).and_then(Value::as_str).filter(|text| !text.is_empty())
        {
            return Thinking { field, text: text.to_owned(), summary: String::new() };
        }
    }
    let mut found = Thinking { field: "reasoning_details", ..Thinking::default() };
    for detail in delta.get("reasoning_details").and_then(Value::as_array).into_iter().flatten() {
        match detail.get("type").and_then(Value::as_str) {
            Some("reasoning.text") => {
                found.text.push_str(detail.get("text").and_then(Value::as_str).unwrap_or_default());
            }
            Some("reasoning.summary") => found
                .summary
                .push_str(detail.get("summary").and_then(Value::as_str).unwrap_or_default()),
            _ => {}
        }
    }
    found
}

/// Whether a delta adds anything to the response, as opposed to a role or empty strings.
pub(super) fn carries_content(delta: &Value) -> bool {
    let text =
        |field: &str| delta.get(field).and_then(Value::as_str).is_some_and(|text| !text.is_empty());
    text("content")
        || text("refusal")
        || !thinking(delta).is_empty()
        || delta.get("tool_calls").and_then(Value::as_array).is_some_and(|calls| !calls.is_empty())
}

impl ChatDecoder {
    pub(super) fn apply_delta(
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
        let thinking = thinking(delta);
        if !thinking.text.is_empty() {
            let block_id = self.reasoning_block(thinking.field, events);
            self.output.append_reasoning(block_id, &thinking.text)?;
            events.push(ModelStreamEvent::ReasoningDelta { block_id, text: thinking.text });
        }
        if !thinking.summary.is_empty() {
            let block_id = self.reasoning_block(thinking.field, events);
            self.output.append_reasoning_summary(block_id, &thinking.summary)?;
            events
                .push(ModelStreamEvent::ReasoningSummaryDelta { block_id, text: thinking.summary });
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
        if delta.get("thinking").is_some() {
            events.push(ModelStreamEvent::ProviderWarning {
                code: Some("openai_chat_reasoning_extension_unmapped".to_owned()),
                message: "unmapped compatible-endpoint field delta.thinking".to_owned(),
            });
        }
        Ok(())
    }

    /// The response's thinking block, started by the first thinking that arrives. Its reference
    /// names the field the thinking came in unless that is `reasoning_content`: only thinking that
    /// came as `reasoning_content` is sent back, so a service that refuses that field in history
    /// is never sent it.
    fn reasoning_block(
        &mut self,
        field: &str,
        events: &mut Vec<ModelStreamEvent>,
    ) -> ModelContentBlockId {
        if let Some(block_id) = self.reasoning_block {
            return block_id;
        }
        let block_id = self.output.start_reasoning_block(Some(ProviderProtocolReference {
            item_id: None,
            block_id: (field != REASONING_CONTENT).then(|| field.to_owned()),
            index: Some(0),
        }));
        self.reasoning_block = Some(block_id);
        events.push(ModelStreamEvent::ReasoningStarted { block_id });
        block_id
    }
}
