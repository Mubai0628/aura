use std::collections::{BTreeMap, BTreeSet};

use aura_domain_common::ToolCallId;
use aura_domain_common::{ModelContentBlockId, ModelStepId};
use aura_model_domain::{
    AssistantContentBlock, ModelRefusal, ModelUsage, NormalizedModelOutput, NormalizedStopReason,
    NormalizedToolCall, PartialModelUsage, ProviderContinuationState, ProviderProtocolReference,
    ProviderToolCallId, ReasoningContentBlock, UsageSource,
};

use crate::ModelAdapterError;

struct PendingAssistantBlock {
    text: String,
    provider_reference: Option<ProviderProtocolReference>,
}

struct PendingReasoningBlock {
    text: String,
    summary: String,
    provider_reference: Option<ProviderProtocolReference>,
}

struct PendingToolCall {
    tool_call_id: ToolCallId,
    tool_name: String,
    arguments_json: String,
    position: u32,
    completed: bool,
}

pub struct NormalizedOutputBuilder {
    model_step_id: ModelStepId,
    assistant_order: Vec<ModelContentBlockId>,
    assistant: BTreeMap<ModelContentBlockId, PendingAssistantBlock>,
    reasoning_order: Vec<ModelContentBlockId>,
    reasoning: BTreeMap<ModelContentBlockId, PendingReasoningBlock>,
    tool_order: Vec<ProviderToolCallId>,
    tools: BTreeMap<ProviderToolCallId, PendingToolCall>,
    tool_positions: BTreeSet<u32>,
    refusal_seen: bool,
    refusal: String,
    refusal_provider_code: Option<String>,
    usage: PartialModelUsage,
    stop_reason: Option<NormalizedStopReason>,
    provider_request_id: Option<String>,
    provider_continuation: Option<ProviderContinuationState>,
}

impl NormalizedOutputBuilder {
    #[must_use]
    pub fn new(model_step_id: ModelStepId) -> Self {
        Self {
            model_step_id,
            assistant_order: Vec::new(),
            assistant: BTreeMap::new(),
            reasoning_order: Vec::new(),
            reasoning: BTreeMap::new(),
            tool_order: Vec::new(),
            tools: BTreeMap::new(),
            tool_positions: BTreeSet::new(),
            refusal_seen: false,
            refusal: String::new(),
            refusal_provider_code: None,
            usage: PartialModelUsage {
                input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                reasoning_tokens: None,
                cached_input_tokens: None,
                cache_write_tokens: None,
            },
            stop_reason: None,
            provider_request_id: None,
            provider_continuation: None,
        }
    }

    pub fn set_provider_request_id(&mut self, value: Option<String>) {
        if value.as_ref().is_some_and(|id| !id.trim().is_empty()) {
            self.provider_request_id = value;
        }
    }

    pub fn start_assistant_block(
        &mut self,
        provider_reference: Option<ProviderProtocolReference>,
    ) -> ModelContentBlockId {
        let id = ModelContentBlockId::new();
        self.assistant_order.push(id);
        self.assistant
            .insert(id, PendingAssistantBlock { text: String::new(), provider_reference });
        id
    }

    pub fn append_assistant(
        &mut self,
        block_id: ModelContentBlockId,
        text: &str,
    ) -> Result<(), ModelAdapterError> {
        self.assistant
            .get_mut(&block_id)
            .ok_or_else(|| {
                ModelAdapterError::protocol(
                    "assistant_block_unknown",
                    "assistant delta references an unknown content block",
                )
            })?
            .text
            .push_str(text);
        Ok(())
    }

    pub fn start_reasoning_block(
        &mut self,
        provider_reference: Option<ProviderProtocolReference>,
    ) -> ModelContentBlockId {
        let id = ModelContentBlockId::new();
        self.reasoning_order.push(id);
        self.reasoning.insert(
            id,
            PendingReasoningBlock {
                text: String::new(),
                summary: String::new(),
                provider_reference,
            },
        );
        id
    }

    pub fn append_reasoning(
        &mut self,
        block_id: ModelContentBlockId,
        text: &str,
    ) -> Result<(), ModelAdapterError> {
        self.reasoning
            .get_mut(&block_id)
            .ok_or_else(|| {
                ModelAdapterError::protocol(
                    "reasoning_block_unknown",
                    "reasoning delta references an unknown content block",
                )
            })?
            .text
            .push_str(text);
        Ok(())
    }

    pub fn append_reasoning_summary(
        &mut self,
        block_id: ModelContentBlockId,
        text: &str,
    ) -> Result<(), ModelAdapterError> {
        self.reasoning
            .get_mut(&block_id)
            .ok_or_else(|| {
                ModelAdapterError::protocol(
                    "reasoning_block_unknown",
                    "reasoning summary references an unknown content block",
                )
            })?
            .summary
            .push_str(text);
        Ok(())
    }

    pub fn append_refusal(&mut self, text: &str) {
        self.refusal_seen = true;
        self.refusal.push_str(text);
    }

    pub fn mark_refusal(&mut self, provider_code: Option<String>) {
        self.refusal_seen = true;
        self.refusal_provider_code = provider_code;
    }

    pub fn start_tool_call(
        &mut self,
        provider_call_id: ProviderToolCallId,
        tool_name: String,
        position: u32,
    ) -> Result<(), ModelAdapterError> {
        if tool_name.trim().is_empty() {
            return Err(ModelAdapterError::protocol(
                "provider_tool_name_missing",
                "provider tool call has a blank tool name",
            ));
        }
        if self.tools.contains_key(&provider_call_id) || !self.tool_positions.insert(position) {
            return Err(ModelAdapterError::protocol(
                "provider_tool_call_duplicate",
                "provider tool call ID or response position is duplicated",
            ));
        }
        self.tool_order.push(provider_call_id.clone());
        self.tools.insert(
            provider_call_id,
            PendingToolCall {
                tool_call_id: ToolCallId::new(),
                tool_name,
                arguments_json: String::new(),
                position,
                completed: false,
            },
        );
        Ok(())
    }

    pub fn append_tool_arguments(
        &mut self,
        provider_call_id: &ProviderToolCallId,
        delta: &str,
    ) -> Result<(), ModelAdapterError> {
        self.tools
            .get_mut(provider_call_id)
            .ok_or_else(|| {
                ModelAdapterError::protocol(
                    "provider_tool_call_unknown",
                    "tool arguments reference an unknown provider tool call ID",
                )
            })?
            .arguments_json
            .push_str(delta);
        Ok(())
    }

    pub fn complete_tool_call(
        &mut self,
        provider_call_id: &ProviderToolCallId,
    ) -> Result<(), ModelAdapterError> {
        let call = self.tools.get_mut(provider_call_id).ok_or_else(|| {
            ModelAdapterError::protocol(
                "provider_tool_call_unknown",
                "tool completion references an unknown provider tool call ID",
            )
        })?;
        if call.completed {
            return Err(ModelAdapterError::protocol(
                "provider_tool_call_duplicate_completion",
                "provider tool call completed more than once",
            ));
        }
        call.completed = true;
        Ok(())
    }

    pub fn update_usage(&mut self, usage: PartialModelUsage) {
        merge_optional(&mut self.usage.input_tokens, usage.input_tokens);
        merge_optional(&mut self.usage.output_tokens, usage.output_tokens);
        merge_optional(&mut self.usage.total_tokens, usage.total_tokens);
        merge_optional(&mut self.usage.reasoning_tokens, usage.reasoning_tokens);
        merge_optional(&mut self.usage.cached_input_tokens, usage.cached_input_tokens);
        merge_optional(&mut self.usage.cache_write_tokens, usage.cache_write_tokens);
    }

    pub fn complete_response(
        &mut self,
        stop_reason: NormalizedStopReason,
    ) -> Result<(), ModelAdapterError> {
        if self.stop_reason.replace(stop_reason).is_some() {
            return Err(ModelAdapterError::protocol(
                "provider_terminal_duplicate",
                "provider response produced more than one terminal event",
            ));
        }
        Ok(())
    }

    pub fn finish(&mut self) -> Result<NormalizedModelOutput, ModelAdapterError> {
        let stop_reason = self.stop_reason.ok_or_else(|| {
            ModelAdapterError::protocol(
                "provider_terminal_missing",
                "provider stream ended without a terminal response event",
            )
        })?;
        let unfinished = self
            .tools
            .iter()
            .filter(|(_, call)| !call.completed)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        if !unfinished.is_empty() {
            // A call the output limit cut off was never finished, so its arguments are
            // partial: it is dropped, never executed, and the step ends as a truncated
            // response. Any other ending with an unfinished call is still a protocol error.
            if stop_reason != NormalizedStopReason::MaxOutputTokens {
                return Err(ModelAdapterError::protocol(
                    "provider_tool_call_incomplete",
                    "provider stream ended with an incomplete tool call",
                ));
            }
            for id in &unfinished {
                self.tools.remove(id);
            }
        }
        if unfinished.is_empty()
            && self.assistant.values().all(|block| block.text.is_empty())
            && self
                .reasoning
                .values()
                .all(|block| block.text.is_empty() && block.summary.is_empty())
            && self.tools.is_empty()
            && !self.refusal_seen
        {
            return Err(ModelAdapterError {
                category: aura_model_domain::ModelFailureCategory::EmptyOutput,
                code: "provider_output_empty".to_owned(),
                message: "provider response completed without model output".to_owned(),
            });
        }
        let input_tokens = self.usage.input_tokens.ok_or_else(|| {
            ModelAdapterError::protocol(
                "provider_usage_missing",
                "provider stream did not report input token usage",
            )
        })?;
        let output_tokens = self.usage.output_tokens.ok_or_else(|| {
            ModelAdapterError::protocol(
                "provider_usage_missing",
                "provider stream did not report output token usage",
            )
        })?;
        let total_tokens =
            self.usage.total_tokens.unwrap_or_else(|| input_tokens.saturating_add(output_tokens));
        let assistant_blocks = self
            .assistant_order
            .iter()
            .filter_map(|block_id| {
                self.assistant.get(block_id).map(|block| AssistantContentBlock {
                    block_id: *block_id,
                    text: block.text.clone(),
                    provider_reference: block.provider_reference.clone(),
                })
            })
            .collect();
        let reasoning_blocks = self
            .reasoning_order
            .iter()
            .filter_map(|block_id| {
                self.reasoning.get(block_id).map(|block| ReasoningContentBlock {
                    block_id: *block_id,
                    text: block.text.clone(),
                    summary: (!block.summary.is_empty()).then(|| block.summary.clone()),
                    provider_reference: block.provider_reference.clone(),
                })
            })
            .collect();
        let tool_calls = self
            .tool_order
            .iter()
            .filter_map(|provider_call_id| {
                self.tools.get(provider_call_id).map(|call| NormalizedToolCall {
                    tool_call_id: call.tool_call_id,
                    provider_call_id: provider_call_id.clone(),
                    tool_name: call.tool_name.clone(),
                    arguments_json: call.arguments_json.clone(),
                    position: call.position,
                })
            })
            .collect();
        Ok(NormalizedModelOutput {
            model_step_id: self.model_step_id,
            assistant_blocks,
            reasoning_blocks,
            tool_calls,
            refusal: self.refusal_seen.then(|| ModelRefusal {
                message: self.refusal.clone(),
                provider_code: self.refusal_provider_code.clone(),
            }),
            stop_reason,
            usage: ModelUsage {
                input_tokens,
                output_tokens,
                total_tokens,
                reasoning_tokens: self.usage.reasoning_tokens,
                cached_input_tokens: self.usage.cached_input_tokens,
                cache_write_tokens: self.usage.cache_write_tokens,
                source: UsageSource::ProviderReported,
            },
            provider_continuation: self.provider_continuation.clone(),
            provider_request_id: self.provider_request_id.clone(),
            raw_response_ref: None,
        })
    }
}

fn merge_optional(target: &mut Option<u64>, value: Option<u64>) {
    if value.is_some() {
        *target = value;
    }
}

#[cfg(test)]
mod tests {
    use aura_model_domain::{
        NormalizedStopReason, PartialModelUsage, ProviderProtocolReference, ProviderToolCallId,
    };

    use super::NormalizedOutputBuilder;

    #[test]
    fn aura_content_and_tool_ids_remain_separate_from_provider_protocol_ids() {
        let mut output = NormalizedOutputBuilder::new(aura_domain_common::ModelStepId::new());
        let provider_reference = ProviderProtocolReference {
            item_id: Some("Provider-Item-ExactCase".to_owned()),
            block_id: Some("Provider-Block-ExactCase".to_owned()),
            index: Some(7),
        };
        let block_id = output.start_assistant_block(Some(provider_reference.clone()));
        output.append_assistant(block_id, "answer").expect("assistant text");

        let provider_call_id =
            ProviderToolCallId::new("Provider-Tool-ExactCase").expect("provider tool ID");
        output
            .start_tool_call(provider_call_id.clone(), "workspace.read".to_owned(), 0)
            .expect("tool start");
        output.append_tool_arguments(&provider_call_id, "{}").expect("tool arguments");
        output.complete_tool_call(&provider_call_id).expect("tool completion");
        output.update_usage(PartialModelUsage {
            input_tokens: Some(2),
            output_tokens: Some(3),
            total_tokens: Some(5),
            reasoning_tokens: None,
            cached_input_tokens: None,
            cache_write_tokens: None,
        });
        output.complete_response(NormalizedStopReason::ToolCalls).expect("response completion");

        let normalized = output.finish().expect("normalized output");
        assert!(block_id.to_string().starts_with("mcb_"));
        assert_eq!(normalized.assistant_blocks[0].block_id, block_id);
        assert_eq!(
            normalized.assistant_blocks[0].provider_reference.as_ref(),
            Some(&provider_reference)
        );
        assert_eq!(normalized.tool_calls[0].provider_call_id, provider_call_id);
        assert_ne!(
            normalized.tool_calls[0].tool_call_id.to_string(),
            normalized.tool_calls[0].provider_call_id.as_str()
        );
    }
}
