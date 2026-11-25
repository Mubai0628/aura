//! A large arrival can outgrow the remaining space in one step. Only an earlier full,
//! still-authorized request can then be summarized; the arrival stays in the live history.
use super::*;
use aura_model_domain::LogicalInstructionBlock;
use aura_persistence_v2::agent::request_boundary::RequestBoundary;
use std::collections::BTreeSet;

pub(super) struct Admitted {
    pub prefix: RequestPrefix,
    pub events: BTreeSet<EventId>,
    pub wire_bindings: Vec<aura_model_domain::ProviderToolCallWireBinding>,
    pub cutoff: SessionSequence,
}

pub(super) async fn previous(
    app: &Arc<AgentApplication>,
    input: &ContextPreparationInput,
    live: &RequestPrefix,
    target: &ModelExecutionSnapshot,
    fixed: &[ToolId],
    read_gap: bool,
) -> anyhow::Result<Option<Admitted>> {
    let unavailable = |reason: &str| {
        if read_gap {
            aura_core_base::append_core_log(
                "context.adopted_prefix_unavailable",
                serde_json::json!({
                    "model_step_id":input.request.model_step_id, "reason":reason,
                }),
            );
        }
        Ok(None)
    };
    let Some(previous) = &input.previous_snapshot else { return unavailable("snapshot") };
    if previous.session_id != input.request.session_id
        || previous.model_profile_id != target.model_profile_id
        || previous.tool_schema_set.tools != live.tool_schema_set.tools
        || previous.attachments != live.attachments
    {
        return unavailable("route_or_schema");
    }
    let previous = previous.clone();
    let expected_tools = aura_domain_common::canonical_json_v1_sha256(&fixed)?;
    let step = previous.model_step_id;
    let Some(task) =
        previous.sections.iter().find(|s| s.kind == ContextSectionKind::CurrentTaskState)
    else {
        return unavailable("state_artifact");
    };
    let task_id = task.content_artifact_id;
    let manifest_id = previous.model_egress_manifest_artifact_id;
    let (marker, model, manifest, task_text) = app
        .store
        .read(
            StoreStep::run(input.request.agent_run_id, "agent.compaction.admitted_boundary"),
            move |db| {
                let model = aura_persistence_v2::model::step_snapshot(db, step)?;
                let marker = RequestBoundary::load(db, step)?;
                let read = |id| aura_persistence_core::artifact::read(db, id, &step.to_string());
                let manifest =
                    serde_json::from_slice::<ModelEgressManifestV1>(&read(manifest_id)?)?;
                Ok((marker, model, manifest, read(task_id)?))
            },
        )
        .await?;
    let Some(marker) = marker else { return unavailable("request_boundary") };
    if marker.fixed_tools != expected_tools
        || marker.serializer_version != live.instruction_envelope.serializer_version
        || model.provider_id != target.provider_id
        || model.api_format != target.api_format
        || model.endpoint_id != target.endpoint_id
        || model.endpoint_fingerprint != target.endpoint_fingerprint
        || model.model_name != target.model_name
        || model.credential_binding_fingerprint != target.credential_binding_fingerprint
        || model.chat_instruction_role != target.chat_instruction_role
        || model.request_options.image_input != target.request_options.image_input
        || manifest.context_snapshot_id != previous.id
        || manifest.manifest_hash != previous.model_egress_manifest_hash
        || manifest.validate_contract_shape().is_err()
    {
        return unavailable("rendering_or_manifest");
    }
    let mut materialized = Vec::new();
    let replay_authorized = previous.task_state_replay.as_ref().is_some_and(|previous| {
        live.instruction_envelope
            .task_state_replay
            .as_ref()
            .is_some_and(|current| previous.is_authorized_by(current))
    });
    let mut refresh_task_state = previous.task_state_replay.is_some() && !replay_authorized;
    for item in &manifest.items {
        if admitted_state(&item.source_ref)
            || replay_authorized && task.source_refs.contains(&item.source_ref)
        {
            // The checkpoint is either re-admitted with the same source authority or
            // omitted together with every old state entry below.
            continue;
        }
        let original_control = matches!(&item.source_ref, ContextSourceRef::RuntimeState { state_kind, .. }
            if state_kind == "agent.control_state.v1")
            && item.source_kind == ModelEgressSourceKind::AuraCore;
        if original_control
            || live.egress_sources.iter().any(|s| {
                (s.source_ref == item.source_ref
                    || read_gap && same_runtime_state(&s.source_ref, &item.source_ref))
                    && s.source_kind == item.source_kind
                    && s.authorization_basis == item.authorization_basis
                    && s.content_hash == item.content_hash
                    && s.data_class == item.data_class
            })
        {
            continue;
        }
        if read_gap
            && matches!(&item.source_ref, ContextSourceRef::RuntimeState { .. })
            && task.source_refs.contains(&item.source_ref)
        {
            // Regenerate the entire task-state block when mutable supporting evidence changed.
            // The old block and its sources never leave in this new compaction request.
            refresh_task_state = true;
            continue;
        }
        let ContextSourceRef::Event { event_id } = item.source_ref else {
            return unavailable("non_historical_source_changed");
        };
        // Only ordinary historical events may be missing because of the read cap. External grants
        // and mutable fragments still need the exact live authorization and representation above.
        if !read_gap
            || live.egress_sources.iter().any(|s| s.source_ref == item.source_ref)
            || !previous.conversation_items.iter().any(|item| super::event_id(item) == event_id)
            || !ordinary_history_source(
                item.source_kind,
                item.data_class,
                &item.authorization_basis,
            )
        {
            return unavailable("historical_source_changed");
        }
        materialized.push((event_id, item.source_kind));
    }
    if !materialized.is_empty() {
        let (session, run) = (input.request.session_id, input.request.agent_run_id);
        let authorized = app
            .store
            .read(StoreStep::run(run, "agent.compaction.admitted_source_scope"), move |db| {
                for (event, kind) in materialized {
                    if !history_in_scope(db, session, run, event, kind)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            })
            .await?;
        if !authorized {
            return unavailable("historical_source_out_of_scope");
        }
    }
    let mut blocks = live.instruction_envelope.ordered_blocks.clone();
    let task_text = String::from_utf8(task_text)?;
    for (block, old) in blocks.iter_mut().zip(&previous.sections) {
        if old.kind == ContextSectionKind::CurrentTaskState && refresh_task_state {
            continue;
        }
        if old.kind == ContextSectionKind::CurrentTaskState {
            if ContentHash::sha256(task_text.as_bytes()) != old.content_hash {
                return unavailable("state_integrity");
            }
            let ids = old
                .source_refs
                .iter()
                .filter_map(|r| match r {
                    ContextSourceRef::Event { event_id } => Some(*event_id),
                    _ => None,
                })
                .collect::<BTreeSet<_>>();
            *block = LogicalInstructionBlock {
                kind: block.kind,
                content: task_text.clone(),
                content_hash: old.content_hash.clone(),
                source_event_ids: ids.into_iter().collect(),
            };
        } else if old.content_hash != block.content_hash {
            return unavailable("instruction_changed");
        }
    }
    let text = blocks.iter().map(|b| b.content.as_str()).collect::<Vec<_>>().join("\n");
    let mut envelope = CompiledInstructionEnvelope::new(marker.serializer_version, blocks, text)?;
    if !refresh_task_state {
        envelope.task_state_replay = previous.task_state_replay;
    }
    if !refresh_task_state && envelope.instruction_hash != previous.instruction_envelope_hash {
        return unavailable("envelope_integrity");
    }
    let mut sources = prefix_egress_sources(&manifest);
    let mut conversation_items = previous.conversation_items;
    let mut estimated_input_tokens = previous.budget.estimated_input_tokens;
    if refresh_task_state {
        let current = input
            .section_inputs
            .iter()
            .find(|section| section.kind == ContextSectionKind::CurrentTaskState)
            .ok_or_else(|| anyhow::anyhow!("context.task_state_missing"))?;
        sources.retain(|item| {
            !admitted_state(&item.source_ref) && !task.source_refs.contains(&item.source_ref)
        });
        sources.extend(
            live.egress_sources
                .iter()
                .filter(|item| {
                    current.fragments.iter().any(|fragment| fragment.source_ref == item.source_ref)
                })
                .cloned(),
        );
        if let Some(block) = envelope
            .ordered_blocks
            .iter()
            .find(|block| block.kind == aura_model_domain::LogicalInstructionKind::TaskState)
        {
            estimated_input_tokens =
                estimated_input_tokens.saturating_add(block.content.len() as u64);
        }
        for item in &mut conversation_items {
            if let LogicalConversationItem::Assistant { provider_history, .. } = item {
                *provider_history = None;
            }
        }
        aura_core_base::append_core_log(
            "context.adopted_prefix_rebuilt",
            serde_json::json!({
                "model_step_id":input.request.model_step_id, "reason":"task_state_sources_changed",
            }),
        );
    }
    let events = previous
        .sections
        .iter()
        .flat_map(|section| &section.source_refs)
        .filter_map(|reference| match reference {
            ContextSourceRef::Event { event_id } => Some(*event_id),
            _ => None,
        })
        .collect();
    Ok(Some(Admitted {
        events,
        cutoff: previous.history_cutoff,
        wire_bindings: model.historical_tool_call_wire_bindings,
        prefix: RequestPrefix {
            closing_question: if refresh_task_state { None } else { marker.closing_question },
            instruction_envelope: envelope,
            conversation_items,
            tool_schema_set: previous.tool_schema_set,
            attachments: previous.attachments,
            provider_continuation: None,
            egress_sources: sources,
            estimated_input_tokens,
        },
    }))
}

fn admitted_state(source: &ContextSourceRef) -> bool {
    matches!(source, ContextSourceRef::RuntimeState { state_kind, .. }
        if state_kind == aura_context_runtime::ADMITTED_STATE)
}

fn same_runtime_state(first: &ContextSourceRef, second: &ContextSourceRef) -> bool {
    matches!((first, second),
        (ContextSourceRef::RuntimeState { state_kind: a, .. },
         ContextSourceRef::RuntimeState { state_kind: b, .. }) if a == b)
}

fn ordinary_history_source(
    kind: ModelEgressSourceKind,
    class: aura_security_contract::NetworkDataClass,
    basis: &ModelEgressAuthorizationBasis,
) -> bool {
    use aura_security_contract::NetworkDataClass as Class;
    matches!(
        (kind, class, basis),
        (
            ModelEgressSourceKind::UserEvent,
            Class::UserFileContent,
            ModelEgressAuthorizationBasis::UserSubmission
        ) | (
            ModelEgressSourceKind::AssistantEvent,
            Class::PublicContent,
            ModelEgressAuthorizationBasis::ModelChannelPolicy
        ) | (
            ModelEgressSourceKind::ToolResult,
            Class::WorkspaceSource,
            ModelEgressAuthorizationBasis::ModelChannelPolicy
        )
    )
}

fn history_in_scope(
    db: &aura_persistence_core::Connection,
    session: SessionId,
    run: AgentRunId,
    event: EventId,
    kind: ModelEgressSourceKind,
) -> aura_persistence_core::StoreResult<bool> {
    let event_kind = match kind {
        ModelEgressSourceKind::UserEvent => "agent.user_message",
        ModelEgressSourceKind::AssistantEvent => "agent.assistant_response",
        ModelEgressSourceKind::ToolResult => "tool.call_completed",
        _ => return Ok(false),
    };
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM events e WHERE e.event_id=?1 AND e.kind=?2 AND
         ((e.kind='agent.user_message' AND e.owner='agent-user:' || ?3 AND NOT EXISTS(
             SELECT 1 FROM agent_collaboration_deliveries WHERE target_run_id=?4))
         OR (e.kind<>'agent.user_message' AND EXISTS(
             SELECT 1 FROM turn_tasks t JOIN agent_runs r ON r.turn_task_id=t.id
             JOIN model_steps s ON s.agent_run_id=r.id AND s.state='completed'
             WHERE t.session_id=?3 AND (r.id=?4 OR NOT EXISTS(
                 SELECT 1 FROM agent_collaboration_deliveries WHERE target_run_id IN (?4,r.id)))
             AND ((e.kind='agent.assistant_response' AND e.owner=r.id
                   AND json_extract(s.payload,'$.state.assistant_event_id')=e.event_id)
                  OR (e.kind='tool.call_completed' AND json_extract(s.payload,'$.state.tool_batch_id')=e.owner))))))",
        [event.to_string(), event_kind.to_owned(), session.to_string(), run.to_string()],
        |row| row.get(0),
    )?)
}
