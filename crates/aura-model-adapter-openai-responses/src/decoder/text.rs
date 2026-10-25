use super::*;

impl ResponsesDecoder {
    pub(super) fn output_text_delta(
        &mut self,
        value: &Value,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        let output_index = required_u32(value, "output_index")?;
        let content_index = required_u32(value, "content_index")?;
        let delta = required_string(value, "delta")?;
        let key = (output_index, content_index);
        let item_id = string_at(value, &["item_id"]).map(ToOwned::to_owned);
        let mut events = Vec::new();
        let block_id = if let Some(block_id) = self.assistant_blocks.get(&key) {
            *block_id
        } else {
            let block_id = self.output.start_assistant_block(Some(ProviderProtocolReference {
                item_id,
                block_id: None,
                index: Some(output_index),
            }));
            self.assistant_blocks.insert(key, block_id);
            events.push(ModelStreamEvent::AssistantTextStarted { block_id });
            block_id
        };
        self.output.append_assistant(block_id, delta)?;
        events.push(ModelStreamEvent::AssistantTextDelta { block_id, text: delta.to_owned() });
        Ok(events)
    }
    pub(super) fn refusal_delta(
        &mut self,
        value: &Value,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        let output_index = required_u32(value, "output_index")?;
        let content_index = required_u32(value, "content_index")?;
        let item_id = required_string(value, "item_id")?;
        let delta = required_string(value, "delta")?;
        let pending =
            self.refusal_blocks.entry((output_index, content_index)).or_insert_with(|| {
                PendingRefusalBlock {
                    item_id: item_id.to_owned(),
                    text: String::new(),
                    completed: false,
                }
            });
        if pending.item_id != item_id {
            return Err(ModelAdapterError::protocol(
                "openai_responses_refusal_identity_changed",
                "refusal item ID changed for one output/content position",
            ));
        }
        if pending.completed {
            return Err(ModelAdapterError::protocol(
                "openai_responses_refusal_completed",
                "refusal delta arrived after its done event",
            ));
        }
        pending.text.push_str(delta);
        self.output.append_refusal(delta);
        Ok(vec![ModelStreamEvent::RefusalDelta { text: delta.to_owned() }])
    }
    pub(super) fn refusal_done(
        &mut self,
        value: &Value,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        let output_index = required_u32(value, "output_index")?;
        let content_index = required_u32(value, "content_index")?;
        let item_id = required_string(value, "item_id")?;
        let refusal = required_string(value, "refusal")?;
        let key = (output_index, content_index);
        if let Some(pending) = self.refusal_blocks.get_mut(&key) {
            if pending.item_id != item_id || pending.text != refusal {
                return Err(ModelAdapterError::protocol(
                    "openai_responses_refusal_mismatch",
                    "final refusal text does not match its streamed deltas",
                ));
            }
            if std::mem::replace(&mut pending.completed, true) {
                return Err(ModelAdapterError::protocol(
                    "openai_responses_refusal_completed",
                    "refusal text completed more than once",
                ));
            }
            return Ok(Vec::new());
        }

        self.output.mark_refusal(None);
        self.output.append_refusal(refusal);
        self.refusal_blocks.insert(
            key,
            PendingRefusalBlock {
                item_id: item_id.to_owned(),
                text: refusal.to_owned(),
                completed: true,
            },
        );
        Ok((!refusal.is_empty())
            .then(|| ModelStreamEvent::RefusalDelta { text: refusal.to_owned() })
            .into_iter()
            .collect())
    }
    pub(super) fn reasoning_summary_delta(
        &mut self,
        value: &Value,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        let output_index = required_u32(value, "output_index")?;
        let summary_index = value
            .get("summary_index")
            .and_then(Value::as_u64)
            .and_then(|index| u32::try_from(index).ok())
            .unwrap_or(0);
        let delta = required_string(value, "delta")?;
        let key = (output_index, summary_index);
        let item_id = string_at(value, &["item_id"]).map(ToOwned::to_owned);
        let mut events = Vec::new();
        let block_id = if let Some(block_id) = self.reasoning_blocks.get(&key) {
            *block_id
        } else {
            let block_id = self.output.start_reasoning_block(Some(ProviderProtocolReference {
                item_id,
                block_id: None,
                index: Some(output_index),
            }));
            self.reasoning_blocks.insert(key, block_id);
            events.push(ModelStreamEvent::ReasoningStarted { block_id });
            block_id
        };
        self.output.append_reasoning_summary(block_id, delta)?;
        events.push(ModelStreamEvent::ReasoningSummaryDelta { block_id, text: delta.to_owned() });
        Ok(events)
    }
    pub(super) fn reasoning_text_delta(
        &mut self,
        value: &Value,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        let output_index = required_u32(value, "output_index")?;
        let content_index = required_u32(value, "content_index")?;
        let item_id = required_string(value, "item_id")?;
        let delta = required_string(value, "delta")?;
        let key = (output_index, content_index);
        let mut events = Vec::new();
        let pending = if let Some(pending) = self.reasoning_text_blocks.get_mut(&key) {
            if pending.item_id != item_id {
                return Err(ModelAdapterError::protocol(
                    "openai_responses_reasoning_identity_changed",
                    "reasoning text item ID changed for one output/content position",
                ));
            }
            if pending.completed {
                return Err(ModelAdapterError::protocol(
                    "openai_responses_reasoning_completed",
                    "reasoning text delta arrived after its done event",
                ));
            }
            pending
        } else {
            let block_id = self.output.start_reasoning_block(Some(ProviderProtocolReference {
                item_id: Some(item_id.to_owned()),
                block_id: None,
                index: Some(output_index),
            }));
            events.push(ModelStreamEvent::ReasoningStarted { block_id });
            self.reasoning_text_blocks.entry(key).or_insert(PendingReasoningTextBlock {
                block_id,
                item_id: item_id.to_owned(),
                text: String::new(),
                completed: false,
            })
        };
        pending.text.push_str(delta);
        self.output.append_reasoning(pending.block_id, delta)?;
        events.push(ModelStreamEvent::ReasoningDelta {
            block_id: pending.block_id,
            text: delta.to_owned(),
        });
        Ok(events)
    }
    pub(super) fn reasoning_text_done(
        &mut self,
        value: &Value,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        let output_index = required_u32(value, "output_index")?;
        let content_index = required_u32(value, "content_index")?;
        let item_id = required_string(value, "item_id")?;
        let text = required_string(value, "text")?;
        let pending = self
            .reasoning_text_blocks
            .get_mut(&(output_index, content_index))
            .ok_or_else(|| {
                ModelAdapterError::protocol(
                    "openai_responses_reasoning_unknown",
                    "reasoning text done event references an unknown output/content position",
                )
            })?;
        if pending.item_id != item_id || pending.text != text {
            return Err(ModelAdapterError::protocol(
                "openai_responses_reasoning_mismatch",
                "final reasoning text does not match its streamed deltas",
            ));
        }
        if std::mem::replace(&mut pending.completed, true) {
            return Err(ModelAdapterError::protocol(
                "openai_responses_reasoning_completed",
                "reasoning text completed more than once",
            ));
        }
        Ok(Vec::new())
    }
}
