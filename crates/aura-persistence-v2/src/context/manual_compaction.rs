//! Durable user commands, separate from conversation inputs and model Tools.
use aura_agent_domain::{AgentRunRequest, AgentRunState};
use aura_context_domain::*;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact, event};
use rusqlite::{Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Stored {
    input: AgentCompactionRequest,
    record: ManualCompactionRequestRecord,
}
fn command(db: &Connection, id: CommandInvocationId) -> StoreResult<Option<Stored>> {
    let raw: Option<String> = db.query_row("SELECT payload FROM events WHERE owner=?1 AND kind='context.manual_compaction' ORDER BY sequence DESC LIMIT 1",
        [id.to_string()], |r| r.get(0)).optional()?;
    raw.map(|s| Ok(serde_json::from_str(&s)?)).transpose()
}
/// A manual compaction's record is an event of its command, so the records are found in
/// the range of command owners of the `` index instead of in the whole journal,
/// which every step, run completion and history read used to read.
pub(crate) fn command_owners() -> (String, String) {
    (format!("{}_", CommandInvocationId::PREFIX), format!("{}`", CommandInvocationId::PREFIX))
}
fn latest(db: &Connection, run: AgentRunId) -> StoreResult<Option<Stored>> {
    let (from, to) = command_owners();
    let raw: Option<String> = db.query_row("SELECT payload FROM events WHERE owner>=?2 AND owner<?3 AND kind='context.manual_compaction' AND json_extract(payload,'$.record.agent_run_id')=?1 ORDER BY sequence DESC LIMIT 1",
        rusqlite::params![run.to_string(), from, to], |r| r.get(0)).optional()?;
    raw.map(|s| Ok(serde_json::from_str(&s)?)).transpose()
}
fn save(tx: &Transaction<'_>, value: &Stored, at: UtcTimestamp) -> StoreResult<()> {
    event::append(
        tx,
        &event::EventDraft {
            id: EventId::new(),
            owner: &value.record.command_invocation_id.to_string(),
            kind: "context.manual_compaction",
            version: 1,
            payload: &serde_json::to_value(value)?,
            at,
        },
        "context",
    )?;
    Ok(())
}
fn pending(value: &Stored) -> bool {
    matches!(
        value.record.state,
        ManualCompactionRequestState::Requested | ManualCompactionRequestState::Running
    )
}
fn owner(db: &Connection, session: SessionId, run: AgentRunId, active: bool) -> StoreResult<()> {
    let run = crate::agent::turns::load_run(db, run)?;
    let turn = crate::agent::turns::load(db, run.turn_task_id)?;
    if turn.session_id != session
        || (active
            && (turn.state.terminal()
                || crate::agent::session::load(db, session)?.active_turn
                    != Some(turn.turn_task_id)
                || crate::agent::application::cancellation_requested(db, run.agent_run_id)?))
    {
        return Err(Error::Conflict);
    }
    Ok(())
}
pub fn status(db: &Connection, input: &AgentRunRequest) -> StoreResult<AgentCompactionStatus> {
    owner(db, input.session_id, input.agent_run_id, false)?;
    Ok(AgentCompactionStatus {
        session_id: input.session_id,
        agent_run_id: input.agent_run_id,
        request: latest(db, input.agent_run_id)?.map(|v| v.record),
    })
}
pub fn request(
    tx: &Transaction<'_>,
    id: CommandInvocationId,
    input: &AgentCompactionRequest,
    at: UtcTimestamp,
) -> StoreResult<AgentCompactionStatus> {
    if let Some(old) = command(tx, id)? {
        if old.input != *input {
            return Err(Error::Conflict);
        }
        return Ok(AgentCompactionStatus {
            session_id: input.session_id,
            agent_run_id: input.agent_run_id,
            request: Some(old.record),
        });
    }
    owner(tx, input.session_id, input.agent_run_id, true)?;
    if input.focus.as_ref().is_some_and(|s| {
        s.chars().count() > MANUAL_COMPACTION_FOCUS_MAX_CHARS
            || s.chars()
                .any(|c| (c <= '\u{001f}' && !matches!(c, '\n' | '\r' | '\t')) || c == '\u{007f}')
    }) {
        return Err(Error::Invalid("context.compaction_invalid_focus"));
    }
    if latest(tx, input.agent_run_id)?.is_some_and(|v| pending(&v)) {
        return Err(Error::Invalid("context.compaction_already_requested"));
    }
    let focus_id = input.focus.as_ref().map(|_| ArtifactId::new());
    if let (Some(focus), Some(artifact_id)) = (&input.focus, focus_id) {
        artifact::put(
            tx,
            artifact_id,
            &id.to_string(),
            "text/plain; charset=utf-8",
            focus.as_bytes(),
            at,
        )?;
    }
    let record = ManualCompactionRequestRecord {
        session_id: input.session_id,
        agent_run_id: input.agent_run_id,
        command_invocation_id: id,
        command_event_id: EventId::new(),
        focus_artifact_id: focus_id,
        focus_hash: input.focus.as_ref().map(|s| ContentHash::sha256(s.as_bytes())),
        state: ManualCompactionRequestState::Requested,
        source_context_snapshot_id: None,
        compaction_sequence: None,
        error_code: None,
        created_at: at,
        terminal_at: None,
    };
    // The command Event is not an authenticated UserMessage; it never becomes
    // a task requirement, Query answer, or queued/steered conversation message.
    event::append(
        tx,
        &event::EventDraft {
            id: record.command_event_id,
            owner: &id.to_string(),
            kind: "context.manual_compaction_command",
            version: 1,
            payload: &serde_json::to_value(input)?,
            at,
        },
        "context",
    )?;
    save(tx, &Stored { input: input.clone(), record: record.clone() }, at)?;
    Ok(AgentCompactionStatus {
        session_id: input.session_id,
        agent_run_id: input.agent_run_id,
        request: Some(record),
    })
}
pub fn requested(
    db: &Connection,
    run: AgentRunId,
) -> StoreResult<Option<ManualCompactionRequestRecord>> {
    Ok(latest(db, run)?
        .filter(|v| v.record.state == ManualCompactionRequestState::Requested)
        .map(|v| v.record))
}
pub fn focus(
    db: &Connection,
    record: &ManualCompactionRequestRecord,
) -> StoreResult<Option<String>> {
    record
        .focus_artifact_id
        .map(|id| {
            let bytes = artifact::read(db, id, &record.command_invocation_id.to_string())?;
            if Some(ContentHash::sha256(&bytes)) != record.focus_hash {
                return Err(Error::Conflict);
            }
            String::from_utf8(bytes).map_err(|_| Error::Conflict)
        })
        .transpose()
}
pub fn consume(
    tx: &Transaction<'_>,
    record: &ManualCompactionRequestRecord,
    compaction: Option<&super::compaction::Compaction>,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let mut stored = command(tx, record.command_invocation_id)?.ok_or(Error::Conflict)?;
    owner(tx, record.session_id, record.agent_run_id, true)?;
    if stored.record != *record
        || record.state != ManualCompactionRequestState::Requested
        || crate::agent::turns::load_run(tx, record.agent_run_id)?.state
            != AgentRunState::PreparingContext
    {
        return Err(Error::Conflict);
    }
    let model_running: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM model_steps WHERE agent_run_id=?1 AND state='running')",
        [record.agent_run_id.to_string()],
        |r| r.get(0),
    )?;
    if model_running {
        return Err(Error::Conflict);
    }
    if let Some(compaction) = compaction {
        if compaction.agent_run_id != record.agent_run_id
            || compaction.session_id != record.session_id
            || super::compaction::load(tx, compaction.id)? != *compaction
        {
            return Err(Error::Conflict);
        }
        stored.record.source_context_snapshot_id = Some(compaction.id);
        stored.record.compaction_sequence = Some(compaction.sequence);
        stored.record.state = ManualCompactionRequestState::Running;
    } else {
        // The retention planner found no eligible history. No model request or
        // history rewrite is needed; this is a successful no-op at the safe point.
        stored.record.state = ManualCompactionRequestState::Completed;
        stored.record.terminal_at = Some(at);
    }
    save(tx, &stored, at)
}
pub fn finish_linked(
    tx: &Transaction<'_>,
    compaction: &super::compaction::Compaction,
    state: ManualCompactionRequestState,
    error: Option<String>,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if let Some(mut stored) = latest(tx, compaction.agent_run_id)?
        && stored.record.state == ManualCompactionRequestState::Running
        && stored.record.source_context_snapshot_id == Some(compaction.id)
    {
        stored.record.state = state;
        stored.record.error_code = error;
        stored.record.terminal_at = Some(at);
        save(tx, &stored, at)?;
    }
    Ok(())
}
/// Whether the user asked for this compaction.
pub(crate) fn linked(
    db: &Connection,
    compaction: &super::compaction::Compaction,
) -> StoreResult<bool> {
    Ok(latest(db, compaction.agent_run_id)?
        .is_some_and(|stored| stored.record.source_context_snapshot_id == Some(compaction.id)))
}
pub fn cancel_run(
    tx: &Transaction<'_>,
    run: AgentRunId,
    reason: &str,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if let Some(mut stored) = latest(tx, run)?.filter(pending) {
        stored.record.state = ManualCompactionRequestState::Cancelled;
        stored.record.error_code = Some(reason.into());
        stored.record.terminal_at = Some(at);
        save(tx, &stored, at)?;
    }
    Ok(())
}
pub fn fail_request(
    tx: &Transaction<'_>,
    id: CommandInvocationId,
    cancelled: bool,
    error: &str,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if let Some(mut stored) = command(tx, id)?.filter(pending) {
        stored.record.state = if cancelled {
            ManualCompactionRequestState::Cancelled
        } else {
            ManualCompactionRequestState::Failed
        };
        stored.record.error_code = Some(error.into());
        stored.record.terminal_at = Some(at);
        save(tx, &stored, at)?;
    }
    Ok(())
}
