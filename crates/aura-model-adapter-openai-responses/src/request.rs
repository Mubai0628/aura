use super::*;

impl ModelProtocolCompiler for OpenAiResponsesAdapter {
    fn compile_request(
        &self,
        input: ModelRequestCompilationInput<'_>,
    ) -> Result<ProviderRequestPlan, ModelAdapterError> {
        validate_model_compilation_input(input, ModelApiFormat::OpenAiResponses)?;
        let tool_name_bindings = provider_tool_name_bindings(input.request)?;
        let layout = aura_model_transport::instruction_layout(input.request);
        let message = |role: &str, text: &str| json!({"role":role,"content":[{"type":"input_text","text":text}]});
        let compile = |items: &[LogicalConversationItem]| {
            history::compile_responses_history(
                input.request,
                items,
                input.snapshot,
                input.resolved_attachments,
            )
        };
        let conversation = compile(&input.request.conversation_items)?;
        // A question asked after the step's request - a compaction - is a user message of its own
        // after the step's input, and what followed it when the model called tools comes next, so
        // all before them is the step's input as it was sent.
        let closing = input
            .request
            .closing_question
            .as_deref()
            .map(|question| message("user", question))
            .into_iter()
            .chain(compile(&input.request.closing_exchange)?);
        let state = message("developer", &layout.current);
        // At OpenAI's cache the task's current state goes before the conversation: it kept a
        // turn's steps only as far as the instructions while a few hundred tokens or more followed
        // the conversation, and as far as the last step once nothing did; the state changes rarely
        // within a turn, so each step's input begins with the one before. Elsewhere the state stays
        // after the conversation, where DeepSeek cached nearly every step.
        let history = if aura_model_transport::reaches_openai_cache(input.snapshot) {
            std::iter::once(state).chain(conversation).chain(closing).collect::<Vec<_>>()
        } else {
            conversation.into_iter().chain(std::iter::once(state)).chain(closing).collect()
        };
        let mut body = Map::new();
        body.insert("model".to_owned(), Value::String(input.snapshot.model_name.clone()));
        if let Some(key) =
            aura_model_transport::prompt_cache_key(input.snapshot, input.request, &layout)
        {
            body.insert("prompt_cache_key".to_owned(), Value::String(key));
        }
        body.insert("instructions".to_owned(), Value::String(layout.stable));
        body.insert("input".to_owned(), Value::Array(history));
        body.insert(
            "max_output_tokens".to_owned(),
            Value::from(input.snapshot.request_options.effective_max_output_tokens),
        );
        body.insert("stream".to_owned(), Value::Bool(true));
        body.insert(
            "text".to_owned(),
            json!({"format": responses_output_format(&input.request.output_request.format)}),
        );
        if let Some(temperature) = input.snapshot.request_options.temperature {
            body.insert("temperature".to_owned(), Value::from(temperature));
        }
        if let Some(top_p) = input.snapshot.request_options.top_p {
            body.insert("top_p".to_owned(), Value::from(top_p));
        }
        // The thinking level the profile asks for.
        if let Some(level) = input.snapshot.request_options.reasoning_effort {
            body.insert("reasoning".to_owned(), json!({"effort": level.openai_name()}));
        }
        if !input.request.tool_schema_set.tools.is_empty() {
            let provider_tools =
                aura_model_transport::ordered_tool_bindings(input.request, &tool_name_bindings)
                    .into_iter()
                    .map(|(tool, binding)| -> Result<Value, ModelAdapterError> {
                        let mut wire = Map::new();
                        wire.insert("type".to_owned(), Value::String("function".to_owned()));
                        wire.insert(
                            "name".to_owned(),
                            Value::String(binding.provider_wire_name.clone()),
                        );
                        wire.insert(
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
                        wire.insert("parameters".to_owned(), wrapped_schema);
                        if strict {
                            wire.insert("strict".to_owned(), Value::Bool(true));
                        }
                        Ok(Value::Object(wire))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
            // The tools of a question asked after the step's request are there for the cache, and
            // no tool choice is named for it: DeepSeek leaves the tools out of the prompt it reads
            // when none may be called, so the request was cached only as far as the instructions,
            // and thinking models refuse the forced forms. The question itself asks that no tool
            // be called.
            body.insert("tools".to_owned(), Value::Array(provider_tools));
        }
        let body = serde_json::to_vec(&Value::Object(body)).map_err(|_| {
            ModelAdapterError::invalid_request(
                "model_request.serialization_failed",
                "OpenAI Responses request could not be serialized",
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

pub(super) fn responses_output_format(format: &ModelOutputFormat) -> Value {
    match format {
        ModelOutputFormat::Text => json!({"type": "text"}),
        ModelOutputFormat::JsonObject => json!({"type": "json_object"}),
        ModelOutputFormat::JsonSchema { name, schema, strict, .. } => json!({
            "type": "json_schema",
            "name": name,
            "schema": schema,
            "strict": strict,
        }),
    }
}

pub(super) fn common_openai_headers(accept: &str) -> Vec<NonSecretProviderHeader> {
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
