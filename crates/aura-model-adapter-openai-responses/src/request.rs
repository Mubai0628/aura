use super::*;

impl ModelProtocolCompiler for OpenAiResponsesAdapter {
    fn compile_request(
        &self,
        input: ModelRequestCompilationInput<'_>,
    ) -> Result<ProviderRequestPlan, ModelAdapterError> {
        validate_model_compilation_input(input, ModelApiFormat::OpenAiResponses)?;
        let tools = aura_model_transport::ProviderTools::compile(
            &input.request.tool_schema_set,
            ModelApiFormat::OpenAiResponses,
        )?;
        let layout = aura_model_transport::instruction_layout(input.request);
        let archive_old = aura_model_transport::uses_deepseek_history_archive(
            input.snapshot,
            &input.request.instruction_envelope,
            &input.request.tool_schema_set,
        );
        let message = |role: &str, text: &str| json!({"role":role,"content":[{"type":"input_text","text":text}]});
        let compile = |items: &[LogicalConversationItem], updates: &[_]| {
            history::compile_responses_history(
                &input.request.conversation_items,
                items,
                input.snapshot,
                input.resolved_attachments,
                archive_old,
                updates.iter(),
            )
        };
        let replay = input.request.instruction_envelope.task_state_replay.as_ref();
        let mut conversation =
            compile(&input.request.conversation_items, replay.map_or(&[], |r| &r.updates))?;
        prepend_background(&mut conversation, layout.background.as_deref());
        // A message after the step's request - a compaction's question or a retry's note - is a
        // user message of its own after the step's input, and what followed a question when the
        // model called tools comes next, so all before them is the step's input as it was sent.
        let closing = input
            .request
            .closing_question
            .as_deref()
            .map(|question| message("user", question))
            .into_iter()
            .chain(compile(&input.request.closing_exchange, &[])?);
        // Where the request's profile asks for it - at OpenAI's cache - the task's state goes before
        // the conversation: OpenAI kept a turn's steps only as far as the instructions while a few
        // hundred tokens or more followed the conversation, and as far as the last step once
        // nothing did. A provider that caches whole earlier requests keeps the conversation only
        // while each step's input begins with the one before, so in a conversation the first state
        // stays in front and each change is added where it happened instead of replacing it.
        // Elsewhere the state stays after the conversation, where DeepSeek cached nearly every
        // step.
        let leading = aura_model_transport::state_before_conversation(input.snapshot);
        let history = if let Some(replay) = replay {
            std::iter::once(message("developer", &replay.leading))
                .chain(conversation)
                .chain(closing)
                .collect()
        } else {
            match (layout.checkpoint, leading, input.session_id) {
                (true, false, _) => conversation.into_iter().chain(closing).collect(),
                (false, true, Some(session)) => {
                    let placement = aura_model_transport::place_state(
                        session,
                        &layout.stable,
                        &conversation,
                        &layout.current,
                    );
                    let mut history = vec![message("developer", &placement.leading)];
                    let mut from = 0;
                    for (at, state) in &placement.changes {
                        history.extend(
                            conversation.get(from..*at).unwrap_or_default().iter().cloned(),
                        );
                        history.push(message("developer", state));
                        from = from.max(*at);
                    }
                    history.extend(conversation.get(from..).unwrap_or_default().iter().cloned());
                    history.extend(closing);
                    history
                }
                (_, true, _) => std::iter::once(message("developer", &layout.current))
                    .chain(conversation)
                    .chain(closing)
                    .collect(),
                (false, false, _) => conversation
                    .into_iter()
                    .chain(std::iter::once(message("developer", &layout.current)))
                    .chain(closing)
                    .collect(),
            }
        };
        let mut body = Map::new();
        body.insert("model".to_owned(), Value::String(input.snapshot.model_name.clone()));
        if let Some(key) =
            aura_model_transport::routing_key_field(input.snapshot, input.request, &layout)
        {
            body.insert(key.name, Value::String(key.value));
        }
        let instructions = if leading { layout.stable } else { layout.including_checkpoint() };
        body.insert("instructions".to_owned(), Value::String(instructions));
        body.insert("input".to_owned(), Value::Array(history));
        body.insert(
            "max_output_tokens".to_owned(),
            Value::from(input.snapshot.request_options.effective_max_output_tokens),
        );
        body.insert("stream".to_owned(), Value::Bool(true));
        if aura_model_transport::store_off(input.snapshot) {
            body.insert("store".to_owned(), Value::Bool(false));
        }
        if aura_model_transport::encrypted_reasoning(input.snapshot) {
            body.insert("include".to_owned(), json!(["reasoning.encrypted_content"]));
        }
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
        body.extend(aura_model_transport::speed_mode_fields(input.snapshot));
        // The tools stay on a question after the step's request so its prefix still caches.
        // The closing question itself asks for a text answer, with no forced tool choice.
        let (definitions, tool_name_bindings) = tools.into_wire(&input.request.fixed_tool_ids);
        if !definitions.is_empty() {
            body.insert("tools".to_owned(), Value::Array(definitions));
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

/// Earlier task states the ordinary request will carry in addition to its current state.
pub fn replayed_task_states(
    session: aura_domain_common::SessionId,
    snapshot: &ModelExecutionSnapshot,
    envelope: &aura_model_domain::CompiledInstructionEnvelope,
    conversation: &[LogicalConversationItem],
    resolved: &[aura_model_domain::ResolvedModelAttachment],
) -> Result<Vec<String>, ModelAdapterError> {
    let layout = aura_model_transport::instruction_envelope_layout(envelope);
    let mut wire = history::compile_responses_history(
        conversation,
        conversation,
        snapshot,
        resolved,
        false,
        [].iter(),
    )?;
    prepend_background(&mut wire, layout.background.as_deref());
    let mut placement =
        aura_model_transport::preview_state(session, &layout.stable, &wire, &layout.current);
    if placement.changes.pop().is_none() {
        return Ok(vec![]);
    }
    Ok(std::iter::once(placement.leading)
        .chain(placement.changes.into_iter().map(|(_, state)| state))
        .collect())
}

fn prepend_background(messages: &mut Vec<Value>, background: Option<&str>) {
    if let Some(text) = background {
        messages
            .insert(0, json!({"role":"assistant","content":[{"type":"output_text","text":text}]}));
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
