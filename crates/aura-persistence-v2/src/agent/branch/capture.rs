//! What a branch carries of one of the source conversation's own turns. It is read once, through
//! the readers the conversation and the model already use, so what is carried is exactly what was
//! shown and what was sent.
use super::record::{CarriedDisplay, CarriedModel, CarriedRow};
use super::timing::BranchTiming;
use crate::agent::{desktop, desktop_messages, model_context};
use aura_agent_domain::*;
use aura_context_domain::HistoryAtomicGroupPreimageV1;
use aura_domain_common::*;
use aura_model_domain::LogicalConversationItem;
use aura_persistence_core::StoreResult;
use aura_tool_projection::AgentToolActivityRequest;
use rusqlite::Connection;
use std::collections::BTreeSet;

const PAGE: u32 = 65_536;

pub(super) struct CapturedText {
    pub source: AgentMessageTextSource,
    pub text: String,
    pub sealed_revision: Option<String>,
}

pub(super) struct Captured {
    pub model: CarriedModel,
    pub display: CarriedDisplay,
    pub texts: Vec<CapturedText>,
    pub results: Vec<(ToolCallId, String)>,
}

/// One finished turn of the source: its model history, its full conversation view, every text it
/// shows, its tool calls and their results.
pub(super) fn capture(
    db: &Connection,
    turn: &TurnTask,
    groups: &[HistoryAtomicGroupPreimageV1],
    rows: &[crate::context::read::Row],
    timing: &mut BranchTiming,
) -> StoreResult<Captured> {
    let session = turn.session_id;
    let mut items = Vec::new();
    let mut wire_bindings = Vec::new();
    timing.time("model.items", || {
        model_context::turn_items(db, turn, groups, None, &mut items, &mut wire_bindings)
    })?;
    let sequences = timing.time("model.sequences", || {
        items
            .iter()
            .map(|item| sequence(db, super::model::event_of(item)))
            .collect::<StoreResult<Vec<_>>>()
    })?;
    let users: BTreeSet<EventId> = items
        .iter()
        .filter_map(|item| match item {
            LogicalConversationItem::User { source_event_id, .. } => Some(*source_event_id),
            _ => None,
        })
        .collect();
    let rows = rows
        .iter()
        .filter(|row| row.turn == Some(turn.turn_task_id) || users.contains(&row.view.event_id))
        .map(|row| CarriedRow { turn: row.turn, view: row.view.clone() })
        .collect();
    let tool_results = timing.time("model.tool_results", || {
        items
            .iter()
            .filter_map(|item| match item {
                LogicalConversationItem::ToolResult { tool_call_id, .. } => Some(*tool_call_id),
                _ => None,
            })
            .map(|call| crate::context::read::whole_tool_result(db, session, call))
            .collect::<StoreResult<Vec<_>>>()
    })?;
    let model = CarriedModel {
        sequences,
        wire_bindings,
        groups: groups.iter().filter(|g| g.turn_task_id == turn.turn_task_id).cloned().collect(),
        rows,
        tool_results,
        items,
    };

    let mut view =
        timing.time("display.view", || desktop::full_turn_view(db, turn.turn_task_id))?;
    view.imported = true;
    let started = std::time::Instant::now();
    let mut texts = Vec::new();
    for user in &view.user_messages {
        texts.push(whole_text(
            db,
            session,
            AgentMessageTextSource::User { event_id: user.event_id },
        )?);
    }
    for step in &view.assistant_messages {
        let id = step.model_step_id;
        texts.push(whole_text(
            db,
            session,
            AgentMessageTextSource::Assistant { model_step_id: id },
        )?);
        if step.reasoning_total_bytes > 0 {
            texts.push(whole_text(
                db,
                session,
                AgentMessageTextSource::Reasoning { model_step_id: id },
            )?);
        }
    }
    if matches!(view.final_text_source, Some(AgentFinalTextSource::CompletionReport)) {
        let source = AgentMessageTextSource::Completion { turn_task_id: turn.turn_task_id };
        texts.push(whole_text(db, session, source)?);
    }
    *timing.phases.entry("display.texts").or_default() += started.elapsed().as_micros();
    let calls: Vec<ToolCallId> = view
        .assistant_messages
        .iter()
        .flat_map(|step| step.tool_call_ids.iter().copied())
        .collect();
    timing.calls += calls.len();
    let mut activity = Vec::new();
    let mut results = Vec::new();
    for call in calls {
        let started = std::time::Instant::now();
        // A call the model wrote that never ran has no record, here or in the source.
        let Ok(page) = crate::tool::activity::page(
            db,
            &AgentToolActivityRequest {
                session_id: session,
                before_call_id: None,
                limit: 1,
                call_ids: Some(vec![call]),
            },
        ) else {
            continue;
        };
        if page.calls.is_empty() {
            continue;
        }
        for mut entry in page.calls {
            // A carried call has no live process or shell output to read.
            entry.shell_query_available = false;
            activity.push(entry);
        }
        *timing.phases.entry("display.activity").or_default() += started.elapsed().as_micros();
        if let Some(text) = timing.time("display.results", || whole_result(db, session, call)) {
            results.push((call, text));
        }
    }
    Ok(Captured { model, display: CarriedDisplay { view, activity }, texts, results })
}

fn sequence(db: &Connection, event: EventId) -> StoreResult<u64> {
    let value: i64 =
        db.query_row("SELECT sequence FROM events WHERE event_id=?1", [event.to_string()], |r| {
            r.get(0)
        })?;
    u64::try_from(value).map_err(|_| aura_persistence_core::PersistenceCoreError::Conflict)
}

/// A whole text, read a page at a time the way the conversation reads it.
fn whole_text(
    db: &Connection,
    session: SessionId,
    source: AgentMessageTextSource,
) -> StoreResult<CapturedText> {
    let mut text = String::new();
    let mut offset = 0;
    let mut sealed_revision = None;
    loop {
        let page = desktop_messages::text(
            db,
            &AgentMessageTextRequest { session_id: session, source, offset, limit: PAGE },
        )?;
        if offset == 0 {
            sealed_revision = page.sealed_revision;
        }
        text.push_str(&page.text);
        match page.next_offset {
            Some(next) if next > offset => offset = next,
            _ => break,
        }
    }
    Ok(CapturedText { source, text, sealed_revision })
}

/// A call's whole result as the conversation shows it, or `None` when it has none.
fn whole_result(db: &Connection, session: SessionId, call: ToolCallId) -> Option<String> {
    let mut text = String::new();
    let mut offset = 0;
    loop {
        let page = desktop::result(
            db,
            &AgentResultRequest { session_id: session, tool_call_id: call, offset, limit: PAGE },
        )
        .ok()?;
        text.push_str(&page.text);
        match page.next_offset {
            Some(next) if next > offset => offset = next,
            _ => return Some(text),
        }
    }
}
