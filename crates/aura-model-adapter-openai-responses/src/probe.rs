use super::*;

impl ModelGenerationProbeProtocol for OpenAiResponsesAdapter {
    fn compile_generation_probe(
        &self,
        input: ModelAvailabilityProbeCompilationInput<'_>,
    ) -> Result<ProviderRequestPlan, ModelAdapterError> {
        validate_generation_probe_input(input, ModelApiFormat::OpenAiResponses)?;
        let body = serde_json::to_vec(&json!({
            "model": input.model_profile.provider_model_name,
            "input": [{
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": MODEL_GENERATION_PROBE_PROMPT,
                }],
            }],
            "max_output_tokens": MODEL_GENERATION_PROBE_MAX_OUTPUT_TOKENS,
            "stream": false,
            "text": {"format": {"type": "text"}},
        }))
        .map_err(|_| {
            ModelAdapterError::invalid_request(
                "model_generation_probe.serialization_failed",
                "OpenAI Responses generation probe could not be serialized",
            )
        })?;
        let mut headers = common_openai_headers("application/json");
        if let Some(organization_id) = &input.endpoint.openai_organization_id {
            headers.push(NonSecretProviderHeader {
                name: "openai-organization".to_owned(),
                value: organization_id.clone(),
                class: ProviderRequestHeaderClass::SemanticRouting,
            });
        }
        if let Some(project_id) = &input.endpoint.openai_project_id {
            headers.push(NonSecretProviderHeader {
                name: "openai-project".to_owned(),
                value: project_id.clone(),
                class: ProviderRequestHeaderClass::SemanticRouting,
            });
        }
        build_generation_probe_request_plan(input, body, headers)
    }

    fn validate_generation_probe_success_body(&self, body: &[u8]) -> Result<(), ModelAdapterError> {
        let value: Value = serde_json::from_slice(body).map_err(|_| {
            ModelAdapterError::protocol(
                "model_generation_probe.responses_invalid_json",
                "OpenAI Responses generation probe body is not valid JSON",
            )
        })?;
        if value.get("status").and_then(Value::as_str) != Some("completed") {
            return Err(ModelAdapterError::protocol(
                "model_generation_probe.responses_terminal_invalid",
                "OpenAI Responses generation probe did not reach a completed terminal",
            ));
        }
        let has_visible_text = value
            .get("output")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|item| item.get("type").and_then(Value::as_str) == Some("message"))
            .flat_map(|item| item.get("content").and_then(Value::as_array).into_iter().flatten())
            .any(|part| {
                part.get("type").and_then(Value::as_str) == Some("output_text")
                    && part
                        .get("text")
                        .and_then(Value::as_str)
                        .is_some_and(|text| !text.trim().is_empty())
            });
        if !has_visible_text {
            return Err(ModelAdapterError::protocol(
                "model_generation_probe.visible_text_missing",
                "OpenAI Responses generation probe returned no non-blank visible text",
            ));
        }
        Ok(())
    }
}
