//! The model side of a branch: the history its requests begin with, and what the model can read
//! back of it with `context.read_events`, `context.read_tool_result` and `context.read_compaction`.
use super::record::{
    self, CarriedModel, CarriedRow, CarriedToolResult, CarriedUserMessage, USER_KIND,
};
use aura_domain_common::*;
use aura_model_domain::{LogicalConversationItem, ProviderToolCallWireBinding};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::BTreeMap;

/// The carried turns' history, as the model was sent it, oldest first.
pub struct ModelPrefix {
    pub items: Vec<LogicalConversationItem>,
    pub wire_bindings: Vec<ProviderToolCallWireBinding>,
}

/// The branch's record for the model side. While its older turns are still being carried
/// the model is sent nothing of it; a run waits for the copy to finish first.
fn finished(db: &Connection, session: SessionId) -> StoreResult<Option<record::BranchRecord>> {
    match record::load(db, session)? {
        Some(branch) if branch.copying.is_some() => Err(Error::Invalid("agent.branch_copying")),
        branch => Ok(branch),
    }
}

fn carried(db: &Connection, session: SessionId) -> StoreResult<Vec<CarriedModel>> {
    let Some(branch) = finished(db, session)? else { return Ok(Vec::new()) };
    branch.turns.iter().map(|turn| record::read_json(db, session, turn.model_artifact_id)).collect()
}

pub fn model_prefix(db: &Connection, session: SessionId) -> StoreResult<Option<ModelPrefix>> {
    if finished(db, session)?.is_none() {
        return Ok(None);
    }
    let mut prefix = ModelPrefix { items: Vec::new(), wire_bindings: Vec::new() };
    for model in carried(db, session)? {
        prefix.items.extend(model.items);
        prefix.wire_bindings.extend(model.wire_bindings);
    }
    Ok(Some(prefix))
}

/// Each carried item's place in the conversation. A compaction orders history units by it; the
/// conversation they came from may no longer exist to ask.
pub fn history_sequences(
    db: &Connection,
    session: SessionId,
) -> StoreResult<BTreeMap<EventId, u64>> {
    let mut sequences = BTreeMap::new();
    for model in carried(db, session)? {
        if model.items.len() != model.sequences.len() {
            return Err(Error::Conflict);
        }
        for (item, sequence) in model.items.iter().zip(model.sequences) {
            sequences.insert(event_of(item), sequence);
        }
    }
    Ok(sequences)
}

pub(super) fn event_of(item: &LogicalConversationItem) -> EventId {
    match item {
        LogicalConversationItem::User { source_event_id, .. }
        | LogicalConversationItem::Assistant { source_event_id, .. }
        | LogicalConversationItem::ToolResult { source_event_id, .. } => *source_event_id,
    }
}

/// The text of a user message this branch carried, for the check that the model is sent exactly
/// what the user wrote.
pub fn carried_user_text(
    db: &Connection,
    session: SessionId,
    event: EventId,
) -> StoreResult<Option<String>> {
    let raw: Option<String> = db
        .query_row(
            "SELECT payload FROM events WHERE owner=?1 AND kind=?2 AND version=1 \
             AND json_extract(payload,'$.event_id')=?3",
            params![record::owner(session), USER_KIND, event.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    raw.map(|raw| {
        let message: CarriedUserMessage = aura_persistence_core::record::decode(&raw)?;
        if message.event_id != event {
            return Err(Error::Conflict);
        }
        Ok(message.text)
    })
    .transpose()
}

/// What `context.read_events` reads of the carried turns, oldest first.
pub(crate) fn carried_rows(db: &Connection, session: SessionId) -> StoreResult<Vec<CarriedRow>> {
    Ok(carried(db, session)?.into_iter().flat_map(|model| model.rows).collect())
}

/// What `context.read_tool_result` reads of one carried tool call.
pub(crate) fn carried_tool_result(
    db: &Connection,
    session: SessionId,
    call: ToolCallId,
) -> StoreResult<Option<CarriedToolResult>> {
    Ok(carried(db, session)?
        .into_iter()
        .flat_map(|model| model.tool_results)
        .find(|result| result.tool_call_id == call))
}

/// The branch's copy of a compaction the model knows by its original identity.
pub(crate) fn carried_compaction(
    db: &Connection,
    session: SessionId,
    original: ContextProjectionSnapshotId,
) -> StoreResult<Option<ContextProjectionSnapshotId>> {
    Ok(finished(db, session)?.and_then(|branch| {
        branch
            .compactions
            .iter()
            .find(|copy| copy.original_id == original)
            .map(|copy| copy.compaction_id)
    }))
}

/// Each carried turn's root user message, by turn.
pub(crate) fn carried_roots(
    db: &Connection,
    session: SessionId,
) -> StoreResult<BTreeMap<TurnTaskId, EventId>> {
    Ok(finished(db, session)?
        .map(|branch| {
            branch.turns.iter().map(|turn| (turn.turn_task_id, turn.root_user_event_id)).collect()
        })
        .unwrap_or_default())
}
