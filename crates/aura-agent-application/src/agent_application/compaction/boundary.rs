//! A large arrival can outgrow the remaining space in one step. Only an earlier full,
//! still-authorized request can then be summarized; the arrival stays in the live history.
use super::*;
use aura_model_domain::LogicalInstructionBlock;
use aura_persistence_v2::agent::request_boundary::RequestBoundary;
use std::collections::BTreeSet;

pub(super) struct Admitted {
    pub prefix: RequestPrefix,
    pub events: BTreeSet<EventId>,
    pub cutoff: SessionSequence,
}

pub(super) async fn previous(
    app: &Arc<AgentApplication>,
    input: &ContextPreparationInput,
    live: &RequestPrefix,
    target: &ModelExecutionSnapshot,
    fixed: &[ToolId],
) -> anyhow::Result<Option<Admitted>> {
    let Some(previous) = &input.previous_snapshot else { return Ok(None) };
    if previous.session_id != input.request.session_id
        || previous.model_profile_id != target.model_profile_id
        || previous.tool_schema_set.tools != live.tool_schema_set.tools
        || previous.attachments != live.attachments
    {
        return Ok(None);
    }
    let previous = previous.clone();
    let expected_tools = aura_domain_common::canonical_json_v1_sha256(&fixed)?;
    let step = previous.model_step_id;
    let Some(task) =
        previous.sections.iter().find(|s| s.kind == ContextSectionKind::CurrentTaskState)
    else {
        return Ok(None);
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
    let Some(marker) = marker else { return Ok(None) };
    if marker.fixed_tools != expected_tools
        || model.provider_id != target.provider_id
        || model.api_format != target.api_format
        || model.endpoint_id != target.endpoint_id
        || model.endpoint_fingerprint != target.endpoint_fingerprint
        || model.model_name != target.model_name
        || model.credential_binding_fingerprint != target.credential_binding_fingerprint
        || model.chat_instruction_role != target.chat_instruction_role
        || manifest.context_snapshot_id != previous.id
        || manifest.manifest_hash != previous.model_egress_manifest_hash
        || manifest.validate_contract_shape().is_err()
    {
        return Ok(None);
    }
    // On a changed scope, attachment, recalled note or file, do not reuse yesterday's permission.
    if !manifest.items.iter().all(|item| {
        let original_control = matches!(&item.source_ref, ContextSourceRef::RuntimeState { state_kind, .. }
            if state_kind == "agent.control_state.v1") && item.source_kind == ModelEgressSourceKind::AuraCore;
        original_control || live.egress_sources.iter().any(|s| {
            s.source_ref == item.source_ref && s.source_kind == item.source_kind && s.authorization_basis == item.authorization_basis
                && s.content_hash == item.content_hash && s.data_class == item.data_class
        })
    }) { return Ok(None); }
    let mut blocks = live.instruction_envelope.ordered_blocks.clone();
    let task_text = String::from_utf8(task_text)?;
    for (block, old) in blocks.iter_mut().zip(&previous.sections) {
        if old.kind == ContextSectionKind::CurrentTaskState {
            if ContentHash::sha256(task_text.as_bytes()) != old.content_hash {
                return Ok(None);
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
            return Ok(None);
        }
    }
    let text = blocks.iter().map(|b| b.content.as_str()).collect::<Vec<_>>().join("\n");
    let envelope = CompiledInstructionEnvelope::new(marker.serializer_version, blocks, text)?;
    if envelope.instruction_hash != previous.instruction_envelope_hash {
        return Ok(None);
    }
    let events = previous
        .sections
        .iter()
        .flat_map(|s| &s.source_refs)
        .filter_map(|r| match r {
            ContextSourceRef::Event { event_id } => Some(*event_id),
            _ => None,
        })
        .collect();
    Ok(Some(Admitted {
        events,
        cutoff: previous.history_cutoff,
        prefix: RequestPrefix {
            closing_question: marker.closing_question,
            instruction_envelope: envelope,
            conversation_items: previous.conversation_items,
            tool_schema_set: previous.tool_schema_set,
            attachments: previous.attachments,
            provider_continuation: None,
            egress_sources: prefix_egress_sources(&manifest),
            estimated_input_tokens: previous.budget.estimated_input_tokens,
        },
    }))
}
