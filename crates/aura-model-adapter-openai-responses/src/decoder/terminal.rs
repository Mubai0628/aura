use super::*;

impl ResponsesDecoder {
    pub(super) fn response_started(
        &mut self,
        value: &Value,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        let provider_request_id = string_at(value, &["response", "id"])
            .or_else(|| string_at(value, &["id"]))
            .map(ToOwned::to_owned);
        self.output.set_provider_request_id(provider_request_id.clone());
        let mut events = vec![ModelStreamEvent::RequestAccepted { provider_request_id }];
        if !self.response_started {
            self.response_started = true;
            events.push(ModelStreamEvent::ResponseStarted);
        }
        Ok(events)
    }
    pub(super) fn response_completed(
        &mut self,
        value: &Value,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        self.ensure_single_terminal()?;
        if self.refusal_blocks.values().any(|block| !block.completed) {
            return Err(ModelAdapterError::protocol(
                "openai_responses_refusal_incomplete",
                "response.completed arrived before refusal text was finalized",
            ));
        }
        let response = value.get("response").unwrap_or(value);
        let usage = usage_from_response(response)?;
        self.output.update_usage(usage.clone());
        let stop_reason = if !self.refusal_blocks.is_empty() {
            NormalizedStopReason::Refusal
        } else if self.has_tool_calls {
            NormalizedStopReason::ToolCalls
        } else {
            NormalizedStopReason::EndTurn
        };
        self.output.complete_response(stop_reason)?;
        Ok(vec![
            ModelStreamEvent::UsageUpdated { usage },
            ModelStreamEvent::ResponseCompleted { stop_reason },
        ])
    }
    pub(super) fn response_incomplete(
        &mut self,
        value: &Value,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        self.ensure_single_terminal()?;
        let response = value.get("response").unwrap_or(value);
        let usage = usage_from_response(response)?;
        self.output.update_usage(usage.clone());
        let code =
            string_at(response, &["incomplete_details", "reason"]).unwrap_or("unknown").to_owned();
        let (reason, stop_reason) = match code.as_str() {
            "max_output_tokens" => {
                (ModelIncompleteReason::MaxOutputTokens, NormalizedStopReason::MaxOutputTokens)
            }
            "content_filter" => {
                (ModelIncompleteReason::ContentFilter, NormalizedStopReason::ContentFilter)
            }
            _ => (
                ModelIncompleteReason::ProviderReported { code },
                NormalizedStopReason::ProviderIncomplete,
            ),
        };
        self.output.complete_response(stop_reason)?;
        Ok(vec![
            ModelStreamEvent::UsageUpdated { usage },
            ModelStreamEvent::ResponseIncomplete { reason },
        ])
    }
    pub(super) fn ensure_single_terminal(&mut self) -> Result<(), ModelAdapterError> {
        if std::mem::replace(&mut self.terminal_seen, true) {
            Err(ModelAdapterError::protocol(
                "openai_responses_terminal_duplicate",
                "OpenAI Responses stream produced multiple terminal events",
            ))
        } else {
            Ok(())
        }
    }
}
