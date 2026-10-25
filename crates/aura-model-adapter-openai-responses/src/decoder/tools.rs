use super::*;

impl ResponsesDecoder {
    pub(super) fn output_item_added(
        &mut self,
        value: &Value,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        let item = value.get("item").ok_or_else(|| {
            ModelAdapterError::protocol(
                "openai_responses_item_missing",
                "response.output_item.added has no item",
            )
        })?;
        if string_at(item, &["type"]) != Some("function_call") {
            return Ok(Vec::new());
        }
        let item_id = required_nested_string(item, &["id"], "function call item id")?.to_owned();
        if self.tool_items.contains_key(&item_id) {
            return Err(ModelAdapterError::protocol(
                "openai_responses_item_duplicate",
                "OpenAI Responses item ID is duplicated",
            ));
        }
        let provider_call_id = string_at(item, &["call_id"])
            .map(ProviderToolCallId::new)
            .transpose()
            .map_err(|_| {
                ModelAdapterError::protocol(
                    "provider_tool_call_id_invalid",
                    "OpenAI Responses function call ID is blank",
                )
            })?;
        let name = string_at(item, &["name"]).map(ToOwned::to_owned);
        if name.as_ref().is_some_and(|name| name.trim().is_empty()) {
            return Err(ModelAdapterError::protocol(
                "provider_tool_name_missing",
                "OpenAI Responses function name is blank",
            ));
        }
        let position = required_u32(value, "output_index")?;
        let started = provider_call_id.is_some() && name.is_some();
        let mut events = Vec::new();
        if let (Some(provider_call_id), Some(name)) = (&provider_call_id, &name) {
            self.output.start_tool_call(provider_call_id.clone(), name.clone(), position)?;
            events.push(ModelStreamEvent::ToolCallStarted {
                provider_call_id: provider_call_id.clone(),
                tool_name: name.clone(),
                position,
            });
        }
        self.tool_items.insert(
            item_id,
            PendingToolItem {
                provider_call_id,
                name,
                position,
                arguments: String::new(),
                started,
                completed: false,
            },
        );
        self.has_tool_calls = true;
        Ok(events)
    }
    pub(super) fn tool_arguments_delta(
        &mut self,
        value: &Value,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        let item_id = required_string(value, "item_id")?;
        let delta = required_string(value, "delta")?;
        let item = self.tool_items.get_mut(item_id).ok_or_else(|| {
            ModelAdapterError::protocol(
                "openai_responses_tool_item_unknown",
                "function call arguments reference an unknown item ID",
            )
        })?;
        if item.completed {
            return Err(ModelAdapterError::protocol(
                "openai_responses_tool_item_completed",
                "function call arguments arrived after completion",
            ));
        }
        item.arguments.push_str(delta);
        let provider_call_id = item.started.then(|| item.provider_call_id.clone()).flatten();
        let Some(provider_call_id) = provider_call_id else {
            return Ok(Vec::new());
        };
        self.output.append_tool_arguments(&provider_call_id, delta)?;
        Ok(vec![ModelStreamEvent::ToolCallArgumentsDelta {
            provider_call_id,
            delta: delta.to_owned(),
        }])
    }
    pub(super) fn output_item_done(
        &mut self,
        value: &Value,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        let item = value.get("item").ok_or_else(|| {
            ModelAdapterError::protocol(
                "openai_responses_item_missing",
                "response.output_item.done has no item",
            )
        })?;
        let item_type = string_at(item, &["type"]).ok_or_else(|| {
            ModelAdapterError::protocol(
                "openai_responses_item_type_missing",
                "response.output_item.done item has no type",
            )
        })?;
        let output_index = required_u32(value, "output_index")?;
        match item_type {
            "reasoning" => {
                if self.continuation_items.insert(output_index, item.clone()).is_some() {
                    return Err(ModelAdapterError::protocol(
                        "openai_responses_reasoning_item_duplicate",
                        "OpenAI Responses duplicated a completed reasoning item position",
                    ));
                }
                return Ok(Vec::new());
            }
            "message" => return Ok(Vec::new()),
            "function_call" => {}
            unknown => {
                return Ok(vec![ModelStreamEvent::ProviderWarning {
                    code: Some("openai_responses_output_item_unknown".to_owned()),
                    message: format!(
                        "unrecognized OpenAI Responses completed output item {unknown}; raw item remains in the InternalOnly stream Artifact"
                    ),
                }]);
            }
        }
        let item_id = required_nested_string(item, &["id"], "function call item id")?;
        let mut pending = self.tool_items.remove(item_id).ok_or_else(|| {
            ModelAdapterError::protocol(
                "openai_responses_tool_item_unknown",
                "completed function call references an unknown item ID",
            )
        })?;
        if pending.completed {
            return Err(ModelAdapterError::protocol(
                "openai_responses_tool_item_completed",
                "function call item completed more than once",
            ));
        }
        if cut_off(item) {
            // The output limit ended the call before its arguments were complete. It stays
            // unfinished: the response builder drops it when the response ends at the output
            // limit, and refuses the response for any other ending.
            self.tool_items.insert(item_id.to_owned(), pending);
            return Ok(vec![ModelStreamEvent::ProviderWarning {
                code: Some("openai_responses_function_call_cut_off".to_owned()),
                message:
                    "a function call ended before its arguments were complete; it is not executed"
                        .to_owned(),
            }]);
        }
        let done_call_id = ProviderToolCallId::new(required_nested_string(
            item,
            &["call_id"],
            "function call provider ID",
        )?)
        .map_err(|_| {
            ModelAdapterError::protocol(
                "provider_tool_call_id_invalid",
                "OpenAI Responses function call ID is blank",
            )
        })?;
        let done_name = required_nested_string(item, &["name"], "function name")?;
        if done_name.trim().is_empty() {
            return Err(ModelAdapterError::protocol(
                "provider_tool_name_missing",
                "OpenAI Responses function name is blank",
            ));
        }
        if pending.provider_call_id.as_ref().is_some_and(|current| current != &done_call_id)
            || pending.name.as_deref().is_some_and(|current| current != done_name)
            || output_index != pending.position
        {
            return Err(ModelAdapterError::protocol(
                "openai_responses_tool_identity_changed",
                "function call identity changed between added and done events",
            ));
        }
        let full_arguments = required_nested_string(item, &["arguments"], "function arguments")?;
        let suffix = full_arguments.strip_prefix(&pending.arguments).ok_or_else(|| {
            ModelAdapterError::protocol(
                "openai_responses_tool_arguments_mismatch",
                "final function arguments do not match streamed argument deltas",
            )
        })?;
        let mut events = Vec::new();
        if !pending.started {
            self.output.start_tool_call(
                done_call_id.clone(),
                done_name.to_owned(),
                pending.position,
            )?;
            events.push(ModelStreamEvent::ToolCallStarted {
                provider_call_id: done_call_id.clone(),
                tool_name: done_name.to_owned(),
                position: pending.position,
            });
            if !full_arguments.is_empty() {
                self.output.append_tool_arguments(&done_call_id, full_arguments)?;
                events.push(ModelStreamEvent::ToolCallArgumentsDelta {
                    provider_call_id: done_call_id.clone(),
                    delta: full_arguments.to_owned(),
                });
            }
        } else if !suffix.is_empty() {
            self.output.append_tool_arguments(&done_call_id, suffix)?;
            events.push(ModelStreamEvent::ToolCallArgumentsDelta {
                provider_call_id: done_call_id.clone(),
                delta: suffix.to_owned(),
            });
        }
        self.output.complete_tool_call(&done_call_id)?;
        pending.provider_call_id = Some(done_call_id.clone());
        pending.name = Some(done_name.to_owned());
        pending.arguments = full_arguments.to_owned();
        pending.started = true;
        pending.completed = true;
        events.push(ModelStreamEvent::ToolCallCompleted { provider_call_id: done_call_id });
        self.tool_items.insert(item_id.to_owned(), pending);
        Ok(events)
    }
}

/// A completed function call item that the Provider did not finish: marked incomplete, or
/// closed without arguments and without saying it completed (DeepSeek at max_output_tokens).
fn cut_off(item: &Value) -> bool {
    match string_at(item, &["status"]) {
        Some("incomplete") => true,
        Some("completed") => false,
        _ => string_at(item, &["arguments"]).is_none_or(str::is_empty),
    }
}
