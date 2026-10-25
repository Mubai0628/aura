//! Session-scoped desktop facts. Pagination cursors and result references must
//! belong to this session; no frontend-retained run identity is required.
use super::{application, desktop_messages, facts, history, inbox, session, turns, work};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact};
use aura_projection_contract::{AgentSessionProjection, AgentTurnView};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Deserialize;

mod poll_backfill;
pub use poll_backfill::{PollBackfillCursor, backfill_turn_poll};
mod tool_input;
pub use tool_input::{LiveToolInput, with_tool_input};

pub fn projection(
    db: &Connection,
    request: &AgentHistoryRequest,
    runtime_instance_id: RuntimeInstanceId,
) -> StoreResult<AgentSessionProjection> {
    if !(1..=50).contains(&request.limit) {
        return Err(Error::Invalid("history page limit"));
    }
    let state = session::load(db, request.session_id)?;
    if request.within_turn.is_some() && request.before_turn_task_id.is_some() {
        return Err(Error::Invalid("mixed history cursors"));
    }
    // A branch shows the turns it carried before its own.
    let carried = super::branch::carried_turn_ids(db, request.session_id)?;
    let (ids, next_before_turn_task_id) = if let Some(cursor) =
        request.within_turn.as_ref().filter(|cursor| carried.contains(&cursor.turn_task_id))
    {
        (vec![cursor.turn_task_id], None)
    } else if request.within_turn.is_none() && !carried.is_empty() {
        branch_page(db, request, &carried)?
    } else if let Some(cursor) = &request.within_turn {
        let turn = turns::load(db, cursor.turn_task_id)?;
        if turn.session_id != request.session_id {
            return Err(Error::Conflict);
        }
        (vec![turn.turn_task_id], None)
    } else {
        let before: i64 = match request.before_turn_task_id {
            Some(id) => db.query_row(
                "SELECT rowid FROM turn_tasks WHERE id=?1 AND session_id=?2",
                [id.to_string(), request.session_id.to_string()],
                |r| r.get(0),
            )?,
            None => i64::MAX,
        };
        let limit = request.limit.min(10);
        let mut q = db.prepare("SELECT id FROM turn_tasks WHERE session_id=?1 AND rowid<?2 ORDER BY rowid DESC LIMIT ?3")?;
        let mut ids = q
            .query_map(params![request.session_id.to_string(), before, limit + 1], |r| {
                r.get::<_, String>(0)
            })?
            .map(|r| r?.parse().map_err(|_| Error::Conflict))
            .collect::<StoreResult<Vec<TurnTaskId>>>()?;
        let more = ids.len() > limit as usize;
        ids.truncate(limit as usize);
        let next = more.then(|| ids.last().copied()).flatten();
        ids.reverse();
        (ids, next)
    };
    let mut active = state
        .active_turn
        .map(|id| {
            let turn = turns::load(db, id)?;
            application::projection(
                db,
                &AgentRunRequest {
                    session_id: request.session_id,
                    agent_run_id: turn.agent_run_id,
                },
            )
        })
        .transpose()?;
    if let Some(active) = &mut active {
        // The live control header needs recent steps only; complete step history
        // is independently reachable through the turn message cursor.
        let skip = active.model_steps.len().saturating_sub(10);
        active.model_steps.drain(..skip);
    }
    // Every contributor is monotonic. Turn/run state-only CAS transitions and
    // visible stream events both advance the projection across reconnects.
    let revision: i64 = db.query_row("SELECT 1 + COALESCE((SELECT MAX(sequence) FROM events),0) + COALESCE((SELECT SUM(revision) FROM turn_tasks),0) + COALESCE((SELECT SUM(revision) FROM agent_runs),0)", [], |r| r.get(0))?;
    let projection = AgentSessionProjection {
        session_id: request.session_id,
        runtime_instance_id,
        revision: Revision::new(u64::try_from(revision).map_err(|_| Error::Conflict)?),
        active,
        inbox: inbox::load(db, request.session_id)?,
        turns: ids
            .into_iter()
            .map(|id| {
                if carried.contains(&id) {
                    super::branch::carried_view(
                        db,
                        request.session_id,
                        id,
                        request.within_turn.as_ref(),
                    )
                } else {
                    turn_view(db, id, request.within_turn.as_ref())
                }
            })
            .collect::<StoreResult<_>>()?,
        next_before_turn_task_id,
        mode_episode: Some(super::mode::view(db, request.session_id)?),
        branch_origin: super::branch::origin(db, request.session_id)?,
        branch_copying: super::branch::progress(db, request.session_id)?,
    };
    // Reject an oversized legacy plan/control fact explicitly before IPC framing.
    // No historical fact is silently dropped to make a response fit.
    if serde_json::to_vec(&projection)?.len() > 5 * 1024 * 1024 {
        return Err(Error::Invalid("agent.history_page_too_large"));
    }
    Ok(projection)
}

/// A branch's page of turns: the carried turns come first, then its own, and a page is cut from
/// that one line exactly as a conversation's own pages are.
fn branch_page(
    db: &Connection,
    request: &AgentHistoryRequest,
    carried: &[TurnTaskId],
) -> StoreResult<(Vec<TurnTaskId>, Option<TurnTaskId>)> {
    let mut q = db.prepare("SELECT id FROM turn_tasks WHERE session_id=?1 ORDER BY rowid")?;
    let own = q
        .query_map([request.session_id.to_string()], |r| r.get::<_, String>(0))?
        .map(|r| r?.parse().map_err(|_| Error::Conflict))
        .collect::<StoreResult<Vec<TurnTaskId>>>()?;
    let line: Vec<TurnTaskId> = carried.iter().copied().chain(own).collect();
    let end = match request.before_turn_task_id {
        Some(id) => line.iter().position(|turn| *turn == id).ok_or(Error::Conflict)?,
        None => line.len(),
    };
    let start = end.saturating_sub(request.limit.min(10) as usize);
    Ok((line[start..end].to_vec(), (start > 0).then(|| line[start])))
}

/// Every message and step of one turn in a single view, for a branch to carry. The
/// pages are the ones the conversation reads, joined oldest first.
pub(crate) fn full_turn_view(db: &Connection, id: TurnTaskId) -> StoreResult<AgentTurnView> {
    let mut view = turn_view(db, id, None)?;
    let mut cursor = view.next_messages.take();
    while let Some(next) = cursor {
        let mut older = turn_view(db, id, Some(&next))?;
        older.user_messages.append(&mut view.user_messages);
        older.assistant_messages.append(&mut view.assistant_messages);
        view.user_messages = older.user_messages;
        view.assistant_messages = older.assistant_messages;
        cursor = older.next_messages;
    }
    Ok(view)
}

fn turn_view(
    db: &Connection,
    id: TurnTaskId,
    cursor: Option<&AgentTurnHistoryCursor>,
) -> StoreResult<AgentTurnView> {
    let (turn, completion) = projected_turn(db, id)?;
    let messages = desktop_messages::page(db, &turn, cursor)?;
    // The completion source decides which single place renders the final answer:
    // a natural completion answers with its own ModelStep response, every other
    // completion carries its report. The natural record's report is not read here;
    // only the step identity is, so a history poll does not receive that body.
    let natural = natural_step(db, id)?;
    let final_text_source = match natural {
        Some((turn_task_id, agent_run_id, model_step_id))
            if turn_task_id == id && agent_run_id == turn.agent_run_id =>
        {
            Some(AgentFinalTextSource::AssistantStep { model_step_id })
        }
        Some(_) => return Err(Error::Conflict),
        None if matches!(turn.state, TurnTaskState::Completed { .. }) => {
            Some(AgentFinalTextSource::CompletionReport)
        }
        None => None,
    };
    let completion = match final_text_source {
        Some(AgentFinalTextSource::CompletionReport) => completion,
        _ => None,
    };
    Ok(AgentTurnView {
        todo: work::todo(db, id)?,
        plan: facts::latest(db, &id.to_string(), "agent.plan_completed")?,
        goal: (turn.mode == AgentTaskMode::Goal).then(|| work::goal(db, id)).transpose()?,
        diagnostic: facts::latest::<serde_json::Value>(
            db,
            &turn.agent_run_id.to_string(),
            "agent.application_error",
        )?
        .and_then(|value| value["error"].as_str().map(|s| s.chars().take(2048).collect())),
        source: facts::latest(db, &id.to_string(), "agent.related_turn")?,
        turn,
        user_messages: messages.users,
        assistant_messages: messages.assistants,
        next_messages: messages.next,
        final_text_source,
        completion,
        imported: false,
    })
}

/// The turn as a history poll may see it. A completed report is replaced with an
/// empty string in this copy; the stored turn is not rewritten. Length, prefix and
/// digest are read as their own values, so the report body is not a column of the
/// result.
fn projected_turn(
    db: &Connection,
    id: TurnTaskId,
) -> StoreResult<(TurnTask, Option<AgentCompletionSummary>)> {
    let located = completion_body(db, id)?;
    let (revision, state, payload, total_bytes, preview): (
        i64,
        String,
        String,
        Option<i64>,
        Option<Vec<u8>>,
    ) = if let Some((revision, state, payload)) = poll_row(db, id)? {
        match located {
            Some((total, preview)) => (revision, state, payload, Some(total), Some(preview)),
            None => (revision, state, payload, None, None),
        }
    } else if let Some((total, preview)) = located {
        let (revision, state, payload) = legacy_turn_without_report(db, id)?;
        (revision, state, payload, Some(total), Some(preview))
    } else {
        db.query_row(
            "SELECT revision, state, json_replace(payload, '$.state.final_report', ''), \
             length(CAST(json_extract(payload, '$.state.final_report') AS BLOB)), \
             substr(CAST(json_extract(payload, '$.state.final_report') AS BLOB), 1, 2048) \
             FROM turn_tasks WHERE id=?1",
            [id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )?
    };
    let turn: TurnTask = aura_persistence_core::record::decode(&payload)?;
    if turn.turn_task_id != id
        || aura_persistence_core::record::sql_u64(turn.revision.value())? != revision
        || turns::turn_state(&turn.state) != state
    {
        return Err(Error::Conflict);
    }
    let completion = match (&turn.state, total_bytes) {
        (TurnTaskState::Completed { .. }, Some(total)) => {
            let total_bytes = u64::try_from(total).map_err(|_| Error::Conflict)?;
            let preview = preview.unwrap_or_default();
            let preview = String::from_utf8(trim_preview(preview)).map_err(|_| Error::Conflict)?;
            let digest = completion_digest(db, id)?;
            Some(AgentCompletionSummary { total_bytes, preview, digest })
        }
        _ => None,
    };
    Ok((turn, completion))
}

fn poll_row(db: &Connection, id: TurnTaskId) -> StoreResult<Option<(i64, String, String)>> {
    db.query_row(
        "SELECT revision, state, payload FROM turn_poll WHERE id=?1",
        [id.to_string()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .optional()
    .map_err(Error::from)
}

fn legacy_turn_without_report(
    db: &Connection,
    id: TurnTaskId,
) -> StoreResult<(i64, String, String)> {
    db.query_row(
        "SELECT revision, state, json_replace(payload, '$.state.final_report', '') \
         FROM turn_tasks WHERE id=?1",
        [id.to_string()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .map_err(Error::from)
}

/// Length and a 2,048-byte prefix from the chunk table, when this turn has one.
fn completion_body(db: &Connection, id: TurnTaskId) -> StoreResult<Option<(i64, Vec<u8>)>> {
    let present: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_visible_body WHERE owner=?1 AND source='completion')",
        [id.to_string()],
        |row| row.get(0),
    )?;
    if !present {
        return Ok(None);
    }
    let total: i64 = db.query_row(
        "SELECT COALESCE(SUM(byte_len), 0) FROM agent_visible_body \
         WHERE owner=?1 AND source='completion' AND revision=( \
         SELECT revision FROM agent_visible_body WHERE owner=?1 AND source='completion' \
         ORDER BY rowid DESC LIMIT 1)",
        [id.to_string()],
        |row| row.get(0),
    )?;
    let preview: String = db.query_row(
        "SELECT text FROM agent_visible_body WHERE owner=?1 AND source='completion' AND ordinal=0 \
         AND revision=(SELECT revision FROM agent_visible_body WHERE owner=?1 AND source='completion' \
         ORDER BY rowid DESC LIMIT 1)",
        [id.to_string()],
        |row| row.get(0),
    )?;
    let mut bytes = preview.into_bytes();
    if bytes.len() > 2048 {
        bytes.truncate(2048);
    }
    Ok(Some((total, trim_preview(bytes))))
}

fn trim_preview(mut bytes: Vec<u8>) -> Vec<u8> {
    while !bytes.is_empty() && std::str::from_utf8(&bytes).is_err() {
        bytes.pop();
    }
    bytes
}

pub(super) fn completion_digest(db: &Connection, id: TurnTaskId) -> StoreResult<Option<String>> {
    #[derive(Deserialize)]
    struct Digest {
        digest: String,
    }
    Ok(facts::latest::<Digest>(db, &id.to_string(), "agent.completion_digest")?
        .map(|fact| fact.digest))
}

/// The step a natural completion named, without the record's copy of the answer.
fn natural_step(
    db: &Connection,
    id: TurnTaskId,
) -> StoreResult<Option<(TurnTaskId, AgentRunId, ModelStepId)>> {
    let row: Option<(String, String, String)> = db
        .query_row(
            "SELECT json_extract(payload, '$.turn_task_id'), \
             json_extract(payload, '$.agent_run_id'), \
             json_extract(payload, '$.model_step_id') \
             FROM events WHERE owner=?1 AND kind='agent.natural_completed' AND version=1 \
             ORDER BY sequence DESC LIMIT 1",
            [id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    row.map(|(turn, run, step)| {
        Ok((
            turn.parse().map_err(|_| Error::Conflict)?,
            run.parse().map_err(|_| Error::Conflict)?,
            step.parse().map_err(|_| Error::Conflict)?,
        ))
    })
    .transpose()
}

pub fn result(db: &Connection, request: &AgentResultRequest) -> StoreResult<AgentResultView> {
    if !(4..=65_536).contains(&request.limit) {
        return Err(Error::Invalid("result page limit"));
    }
    if let Some(view) = super::branch::carried_result(db, request)? {
        return Ok(view);
    }
    let batch: String = db.query_row("SELECT b.id FROM tool_calls c JOIN tool_batches b ON c.batch_id=b.id WHERE c.id=?1 AND b.session_id=?2",
        [request.tool_call_id.to_string(), request.session_id.to_string()], |r| r.get(0))?;
    let batch = crate::tool::execution::batch(db, batch.parse().map_err(|_| Error::Conflict)?)?;
    let call = crate::tool::execution::call(db, request.tool_call_id)?;
    let artifact_id =
        call.result.and_then(|r| r.model_visible_result_ref).ok_or(Error::Conflict)?;
    // Read the model-visible envelope only. Raw process output and private
    // diagnostics are deliberately not reachable through this desktop route.
    let raw =
        artifact::read(db, artifact_id, &batch.batch.invocation_identity.agent_run_id.to_string())?;
    let content = std::str::from_utf8(&raw).map_err(|_| Error::Conflict)?;
    let (text, next_offset) = desktop_messages::text_page(content, request.offset, request.limit)?;
    Ok(AgentResultView {
        tool_call_id: request.tool_call_id,
        artifact_id,
        text,
        next_offset,
        total_bytes: raw.len() as u64,
    })
}
