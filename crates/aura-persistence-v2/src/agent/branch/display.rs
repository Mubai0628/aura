//! The conversation side of a branch: its carried turns, shown before its own and paged as the
//! turns' own pages were, with their texts, tool calls and results.
use super::record::{self, CarriedDisplay, CarriedIndex};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use aura_projection_contract::AgentTurnView;
use aura_tool_projection::AgentToolActivityEntry;
use rusqlite::Connection;

const PAGE_ITEMS: usize = 10;

pub fn origin(db: &Connection, session: SessionId) -> StoreResult<Option<AgentBranchOrigin>> {
    Ok(record::load(db, session)?.map(|branch| branch.origin))
}

/// The carried turns, oldest first. Empty for a conversation that is not a branch.
pub(crate) fn carried_turn_ids(
    db: &Connection,
    session: SessionId,
) -> StoreResult<Vec<TurnTaskId>> {
    Ok(record::load(db, session)?
        .map(|branch| branch.turns.iter().map(|turn| turn.turn_task_id).collect())
        .unwrap_or_default())
}

/// One carried turn's page: the newest ten messages and steps, or those before the cursor.
pub(crate) fn carried_view(
    db: &Connection,
    session: SessionId,
    turn_task_id: TurnTaskId,
    cursor: Option<&AgentTurnHistoryCursor>,
) -> StoreResult<AgentTurnView> {
    let branch = record::load(db, session)?.ok_or(Error::Conflict)?;
    let turn = branch
        .turns
        .iter()
        .find(|turn| turn.turn_task_id == turn_task_id)
        .ok_or(Error::Conflict)?;
    let CarriedDisplay { mut view, .. } = record::read_json(db, session, turn.display_artifact_id)?;
    let users: Vec<_> = view.user_messages.iter().map(|user| user.event_id).collect();
    let end =
        page_start(&users, cursor.and_then(|c| c.before_user_event_id.as_ref()), cursor.is_none())?;
    let start = end.saturating_sub(PAGE_ITEMS);
    let before_user_event_id = (start > 0).then(|| users[start]);
    view.user_messages = view.user_messages.drain(start..end).collect();
    let steps: Vec<_> = view.assistant_messages.iter().map(|step| step.model_step_id).collect();
    let end =
        page_start(&steps, cursor.and_then(|c| c.before_model_step_id.as_ref()), cursor.is_none())?;
    let start = end.saturating_sub(PAGE_ITEMS);
    let before_model_step_id = (start > 0).then(|| steps[start]);
    view.assistant_messages = view.assistant_messages.drain(start..end).collect();
    view.next_messages =
        (before_user_event_id.is_some() || before_model_step_id.is_some()).then_some(
            AgentTurnHistoryCursor { turn_task_id, before_user_event_id, before_model_step_id },
        );
    view.imported = true;
    Ok(view)
}

fn page_start<T: PartialEq>(ids: &[T], before: Option<&T>, initial: bool) -> StoreResult<usize> {
    match before {
        Some(id) => ids.iter().position(|v| v == id).ok_or(Error::Conflict),
        None if initial => Ok(ids.len()),
        None => Ok(0),
    }
}

fn index(db: &Connection, session: SessionId) -> StoreResult<Option<CarriedIndex>> {
    let Some(branch) = record::load(db, session)? else { return Ok(None) };
    Ok(Some(record::read_json(db, session, branch.index_artifact_id)?))
}

/// A page of a carried text, or `None` when the text is not one this branch carried.
pub(crate) fn carried_text(
    db: &Connection,
    request: &AgentMessageTextRequest,
) -> StoreResult<Option<AgentMessageTextPage>> {
    let Some(index) = index(db, request.session_id)? else { return Ok(None) };
    let Some(body) = index.bodies.iter().find(|body| body.source == request.source) else {
        return Ok(None);
    };
    let content = record::read_text(db, request.session_id, body.artifact_id)?;
    if content.len() as u64 != body.total_bytes {
        return Err(Error::Conflict);
    }
    let (text, next_offset) =
        super::super::desktop_messages::text_page(&content, request.offset, request.limit)?;
    Ok(Some(AgentMessageTextPage {
        source: request.source,
        text,
        next_offset,
        total_bytes: body.total_bytes,
        body_id: None,
        sealed_revision: body.sealed_revision.clone(),
        from_offset: request.offset,
    }))
}

/// A page of a carried tool call's result, or `None` when the call is not one this branch carried.
pub(crate) fn carried_result(
    db: &Connection,
    request: &AgentResultRequest,
) -> StoreResult<Option<AgentResultView>> {
    let Some(index) = index(db, request.session_id)? else { return Ok(None) };
    let Some(result) = index.results.iter().find(|r| r.tool_call_id == request.tool_call_id) else {
        return Ok(None);
    };
    let content = record::read_text(db, request.session_id, result.artifact_id)?;
    let (text, next_offset) =
        super::super::desktop_messages::text_page(&content, request.offset, request.limit)?;
    Ok(Some(AgentResultView {
        tool_call_id: request.tool_call_id,
        artifact_id: result.artifact_id,
        text,
        next_offset,
        total_bytes: result.total_bytes,
    }))
}

/// What the conversation shows of one carried tool call, or `None` when it is not one this branch
/// carried. A carried call has no live shell output to read.
pub(crate) fn carried_activity(
    db: &Connection,
    session: SessionId,
    calls: &[ToolCallId],
) -> StoreResult<Vec<(ToolCallId, AgentToolActivityEntry)>> {
    let Some(branch) = record::load(db, session)? else { return Ok(Vec::new()) };
    let index: CarriedIndex = record::read_json(db, session, branch.index_artifact_id)?;
    let mut turns = std::collections::BTreeSet::new();
    for call in calls {
        if let Some((_, turn)) = index.calls.iter().find(|(id, _)| id == call) {
            turns.insert(*turn);
        }
    }
    let mut found = Vec::new();
    for turn in turns {
        let carried = branch.turn_at(turn).ok_or(Error::Conflict)?;
        let display: CarriedDisplay = record::read_json(db, session, carried.display_artifact_id)?;
        for entry in display.activity {
            if calls.contains(&entry.summary.tool_call_id) {
                found.push((entry.summary.tool_call_id, entry));
            }
        }
    }
    Ok(found)
}
