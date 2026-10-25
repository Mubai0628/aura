use super::*;

impl ModelProtocolCompiler for OpenAiResponsesAdapter {
    fn compile_request(
        &self,
        input: ModelRequestCompilationInput<'_>,
    ) -> Result<ProviderRequestPlan, ModelAdapterError> {
        validate_model_compilation_input(input, ModelApiFormat::OpenAiResponses)?;
        let tool_name_bindings = provider_tool_name_bindings(input.request)?;
        let layout = aura_model_transport::instruction_layout(input.request);
        let mut history = history::compile_responses_history(
            input.request,
            input.snapshot,
            input.resolved_attachments,
        )?;
        history.push(
            json!({"role":"developer","content":[{"type":"input_text","text":layout.current}]}),
        );
        let mut body = Map::new();
        body.insert("model".to_owned(), Value::String(input.snapshot.model_name.clone()));
        if let Some(key) = aura_model_transport::openai_prompt_cache_key(input.snapshot, &layout) {
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
