//! Exact model history read model. Raw streams and private diagnostics never
//! enter the conversation. Tool calls are visible only with their sealed group.
use super::{history, session, turns};
use crate::tool::call_name::from_frozen_output as call_name;
use aura_agent_domain::*;
use aura_context_domain::*;
use aura_domain_common::*;
use aura_model_domain::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact, record};
use rusqlite::Connection;
mod continuation;

pub struct ModelHistory {
    pub items: Vec<LogicalConversationItem>,
    pub wire_bindings: Vec<ProviderToolCallWireBinding>,
    pub groups: Vec<HistoryAtomicGroupPreimageV1>,
    pub cutoff: SessionSequence,
}
pub fn bundle(
    db: &Connection,
    id: PromptBundleSnapshotId,
    session: SessionId,
) -> StoreResult<PromptBundleSnapshot> {
    let raw: String = db.query_row(
        "SELECT payload FROM prompt_bundles WHERE id=?1 AND session_id=?2",
        [id.to_string(), session.to_string()],
        |r| r.get(0),
    )?;
    let bundle: PromptBundleSnapshot = record::decode(&raw)?;
    bundle.validate_contract_shape().map_err(|_| Error::Conflict)?;
    if bundle.id != id {
        return Err(Error::Conflict);
    }
    Ok(bundle)
}
pub fn steps(db: &Connection, run: AgentRunId) -> StoreResult<Vec<ModelStep>> {
    let mut q = db.prepare("SELECT id FROM model_steps WHERE agent_run_id=?1 ORDER BY ordinal")?;
    q.query_map([run.to_string()], |r| r.get::<_, String>(0))?
        .map(|id| turns::step(db, id?.parse().map_err(|_| Error::Conflict)?))
        .collect()
}
pub fn history_for_model(
    db: &Connection,
    run_id: AgentRunId,
    initial: bool,
) -> StoreResult<ModelHistory> {
    let run = turns::load_run(db, run_id)?;
    let turn = turns::load(db, run.turn_task_id)?;
    let mut q=db.prepare("SELECT id FROM turn_tasks WHERE session_id=?1 AND rowid<=(SELECT rowid FROM turn_tasks WHERE id=?2) ORDER BY rowid")?;
    let ids = q
        .query_map([turn.session_id.to_string(), turn.turn_task_id.to_string()], |r| {
            r.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut items = vec![];
    let mut wire_bindings = vec![];
    let groups = crate::context::history(db, turn.session_id)?;
    // A branch begins with the turns it carried, exactly as they were sent.
    if let Some(prefix) = super::branch::model_prefix(db, turn.session_id)? {
        items.extend(prefix.items);
        wire_bindings.extend(prefix.wire_bindings);
    }
    for id in ids {
        let previous = turns::load(db, id.parse().map_err(|_| Error::Conflict)?)?;
        if initial && previous.turn_task_id == turn.turn_task_id {
            continue;
        }
        turn_items(db, &previous, &groups, Some(run_id), &mut items, &mut wire_bindings)?;
    }
    wire_bindings.sort_by_key(|b| b.tool_call_id);
    let cutoff: i64 =
        db.query_row("SELECT COALESCE(MAX(sequence),0) FROM events", [], |r| r.get(0))?;
    Ok(ModelHistory {
        items,
        wire_bindings,
        groups,
        cutoff: SessionSequence::new(cutoff.try_into().map_err(|_| Error::Conflict)?),
    })
}
/// One turn's history, as the model is sent it: its user messages, then each completed step's
/// response with its tool results. `current_run` is the run being prepared, whose unpaired tool
/// group is a conflict; an earlier interrupted group is left out.
pub(crate) fn turn_items(
    db: &Connection,
    previous: &TurnTask,
    groups: &[HistoryAtomicGroupPreimageV1],
    current_run: Option<AgentRunId>,
    items: &mut Vec<LogicalConversationItem>,
    wire_bindings: &mut Vec<ProviderToolCallWireBinding>,
) -> StoreResult<()> {
    let user = session::user_message(db, previous.session_id, previous.root_user_event_id)?;
    items.push(LogicalConversationItem::User {
        source_event_id: user.event_id,
        content: user.text.clone(),
        content_hash: ContentHash::sha256(user.text.as_bytes()),
    });
    let mut user_ids = super::query::events(db, previous.turn_task_id)?;
    user_ids.extend(
        super::inbox::consumed(db, previous.turn_task_id)?
            .into_iter()
            .flat_map(|g| g.user_event_ids)
            .filter(|id| *id != previous.root_user_event_id),
    );
    let user_messages = session::ordered_messages(db, previous.session_id, user_ids)?;
    let mut answers = user_messages.into_iter().peekable();
    for step in steps(db, previous.agent_run_id)? {
        while answers.peek().is_some_and(|u| u.at <= step.created_at) {
            let u = answers.next().ok_or(Error::Conflict)?;
            items.push(LogicalConversationItem::User {
                source_event_id: u.event_id,
                content_hash: ContentHash::sha256(u.text.as_bytes()),
                content: u.text,
            });
        }
        let ModelStepState::Completed { assistant_event_id, tool_batch_id, .. } = step.state else {
            continue;
        };
        let output = history::output(db, step.model_step_id)?;
        let snapshot = crate::model::step_snapshot(db, step.model_step_id)?;
        let evidence = crate::model::request_evidence(db, snapshot.id)?.ok_or(Error::Conflict)?;
        // The calls as admitted, with any arguments a `tool.before_call` Hook changed.
        let normalized = crate::hooks::admitted_calls(db, &snapshot, &evidence, &output)?;
        // History names each call as the model made it, so a `tools.call` stays
        // `tools.call` with its own arguments; the admitted calls pair with the batch.
        let made = derive_p07_normalized_tool_calls_v1(&snapshot, &evidence, &output)
            .map_err(|_| Error::Conflict)?;
        if made.len() != normalized.len()
            || made.iter().zip(&normalized).any(|(m, n)| m.tool_call_id != n.tool_call_id)
        {
            return Err(Error::Conflict);
        }
        let mut results = vec![];
        if let Some(batch_id) = tool_batch_id {
            let Some(group) = groups
                .iter()
                .find(|g| g.tool_batch_id == batch_id && g.model_step_id == step.model_step_id)
            else {
                // A prior interrupted response is not a valid paired history
                // group. Its unresolved operations remain in recovery facts.
                if current_run == Some(previous.agent_run_id) {
                    return Err(Error::Conflict);
                }
                continue;
            };
            let batch = crate::tool::execution::batch(db, batch_id)?;
            if batch.batch.calls != normalized || group.calls.len() != normalized.len() {
                return Err(Error::Conflict);
            }
            let terminal = batch.result.as_ref().ok_or(Error::Conflict)?;
            for (index, call) in normalized.iter().enumerate() {
                let result = terminal.results.get(index).ok_or(Error::Conflict)?;
                let member = &group.calls[index];
                if member.tool_call_id != call.tool_call_id
                    || result.tool_call_id != call.tool_call_id
                    || member.tool_call_fact_hash
                        != jcs::canonical_sha256(call).map_err(|_| Error::Conflict)?
                    || member.tool_result_fact_hash
                        != jcs::canonical_sha256(result).map_err(|_| Error::Conflict)?
                {
                    return Err(Error::Conflict);
                }
                let raw = artifact::read(
                    db,
                    result.model_visible_result_ref.ok_or(Error::Conflict)?,
                    &previous.agent_run_id.to_string(),
                )?;
                let envelope: aura_tool_contract::ModelVisibleToolResultEnvelopeV1 =
                    serde_json::from_slice(&raw)?;
                envelope.validate().map_err(|_| Error::Conflict)?;
                let succeeded =
                    matches!(result.outcome, aura_tool_contract::ToolResultOutcomeV1::Succeeded);
                // A `tool.after_call` / `tool.on_error` Hook may have replaced what
                // the model sees; the stored result itself is unchanged.
                let content = match crate::hooks::result_override(db, call.tool_call_id, succeeded)?
                {
                    Some(text) => text,
                    // The model reads the result's view, not its stored envelope.
                    None => envelope.model_text(),
                };
                let event:String=db.query_row("SELECT event_id FROM events WHERE owner=?1 AND kind='tool.call_completed' AND json_extract(payload,'$.tool_call_id')=?2",[batch_id.to_string(),call.tool_call_id.to_string()],|r|r.get(0))?;
                // A result is named by the tool that ran - for a call made through
                // `tools.call`, the tool it named - so its data class and its cleared line stay
                // that tool's.
                let name = executed_name(db, call, &made[index], &evidence, &output)?;
                results.push(LogicalConversationItem::ToolResult {
                    source_event_id: event.parse().map_err(|_| Error::Conflict)?,
                    tool_call_id: call.tool_call_id,
                    tool_name: name,
                    content_hash: ContentHash::sha256(content.as_bytes()),
                    content,
                    is_error: !matches!(
                        result.outcome,
                        aura_tool_contract::ToolResultOutcomeV1::Succeeded
                    ),
                });
            }
        } else if !normalized.is_empty() {
            return Err(Error::Conflict);
        }
        // A `model.after_call` Hook may have changed what later steps see of this one.
        let text = match crate::hooks::step_text_override(db, step.model_step_id)? {
            Some(text) => text,
            None => history::visible_text(&output),
        };
        let content = (!text.is_empty()).then_some(text);
        if content.is_none() && normalized.is_empty() {
            continue;
        }
        let tool_calls = made
            .iter()
            .map(|call| {
                let name = call_name(call, &evidence, &output)?;
                let raw = output
                    .tool_calls
                    .iter()
                    .find(|c| c.tool_call_id == call.tool_call_id)
                    .ok_or(Error::Conflict)?;
                // History stores one complete JSON value per call, and the
                // hash describes exactly what is emitted. A Provider that
                // streamed an unparseable argument fragment keeps its bytes
                // in the response artifact; history carries the reserved
                // marker so this conversation stays readable and recoverable.
                let arguments_json = history_tool_call_arguments_v1(&raw.arguments_json);
                Ok(LogicalHistoricalToolCall {
                    tool_call_id: call.tool_call_id,
                    tool_id: call.definition_ref.as_ref().map(|r| r.tool_id),
                    canonical_name: name,
                    arguments_hash: ContentHash::sha256(arguments_json.as_bytes()),
                    arguments_json,
                })
            })
            .collect::<StoreResult<Vec<_>>>()?;
        for binding in derive_provider_tool_call_protocol_bindings(&snapshot, &evidence, &output)
            .map_err(|_| Error::Conflict)?
        {
            wire_bindings.push(ProviderToolCallWireBinding {
                tool_call_id: binding.tool_call_id,
                tool_id: binding.tool_definition.as_ref().map(|r| r.tool_id),
                provider_wire_tool_name: binding.provider_wire_tool_name,
                wire_identity: ProviderToolCallWireIdentity::ProviderOpaque {
                    provider_tool_call_id: binding.provider_tool_call_id,
                    reuse_domain: ProviderToolCallReuseDomain {
                        endpoint_id: snapshot.endpoint_id,
                        endpoint_security_fingerprint: snapshot.endpoint_fingerprint.clone(),
                        api_format: snapshot.api_format,
                    },
                },
            });
        }
        items.push(LogicalConversationItem::Assistant {
            provider_history: continuation::read(db, step.model_step_id, &snapshot, &output)?,
            source_event_id: assistant_event_id,
            content_hash: content.as_ref().map(|v| ContentHash::sha256(v.as_bytes())),
            content,
            tool_calls,
        });
        items.extend(results);
    }
    for u in answers {
        items.push(LogicalConversationItem::User {
            source_event_id: u.event_id,
            content_hash: ContentHash::sha256(u.text.as_bytes()),
            content: u.text,
        });
    }
    Ok(())
}
/// The name of the tool `admitted` ran: the request's own name for it, or - for a call made through
/// `tools.call` - its name in the step's registry catalog.
fn executed_name(
    db: &Connection,
    admitted: &aura_tool_contract::NormalizedToolCallV1,
    made: &aura_tool_contract::NormalizedToolCallV1,
    evidence: &ProviderRequestEvidence,
    output: &NormalizedModelOutput,
) -> StoreResult<String> {
    if admitted.definition_ref == made.definition_ref {
        return call_name(admitted, evidence, output);
    }
    let reference = admitted.definition_ref.as_ref().ok_or(Error::Conflict)?;
    let registry =
        crate::tool::registry(db, reference.registry_snapshot_ref.tool_registry_snapshot_id)?
            .ok_or(Error::Conflict)?;
    crate::tool::entry::catalog_name(&registry, reference.tool_id).ok_or(Error::Conflict)
}
/// The text of a user message the model is sent, for the egress check that it is exactly what the
/// user wrote: the conversation's own message, or one a branch carried.
pub fn egress_user_text(
    db: &Connection,
    session_id: SessionId,
    event_id: EventId,
) -> StoreResult<Option<String>> {
    if let Some(message) = user_event(db, session_id, event_id)? {
        return Ok(Some(message.text));
    }
    super::branch::carried_user_text(db, session_id, event_id)
}
pub fn user_event(
    db: &Connection,
    session_id: SessionId,
    event_id: EventId,
) -> StoreResult<Option<OriginalUserMessage>> {
    let exists:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM events WHERE event_id=?1 AND owner=?2 AND kind='agent.user_message')",[event_id.to_string(),format!("agent-user:{session_id}")],|r|r.get(0))?;
    exists.then(|| session::user_message(db, session_id, event_id)).transpose()
}
