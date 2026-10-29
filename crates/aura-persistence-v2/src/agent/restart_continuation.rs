//! Turns Aura continued by itself after Core restarted while Aura stayed open, and how often in a
//! row it may. The record is a fact of the turn that says "继续", in its own kind, so a version
//! that does not know it reads that turn as it always did.
use super::{facts, session, turns};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::StoreResult;
use rusqlite::{Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

const KIND: &str = "agent.auto_continued";
/// At most this many automatic continuations in a row of one task within `WINDOW_SECONDS`.
const LIMIT: usize = 2;
const WINDOW_SECONDS: i64 = 30 * 60;
/// Core restarting again this soon after an automatic continuation is a restart loop.
const RESTART_LOOP_SECONDS: i64 = 2 * 60;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct AutoContinued {
    cause: CoreRestartCause,
    restarted_at: UtcTimestamp,
    after_turn_task_id: TurnTaskId,
}

/// Whether Aura continues a cut-off turn by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Continue,
    /// The turn is no longer the session's cut-off last turn, or another turn is under way.
    Moved,
    /// The task was already continued twice in the last 30 minutes.
    LimitReached,
    /// The cut-off turn was itself an automatic continuation, started under two minutes ago.
    RestartLoop,
}

/// Whether `turn` is still what a restart cut off: the session's last turn, stopped by the
/// restart, with nothing under way after it.
pub fn cut_off(db: &Connection, turn: TurnTaskId) -> StoreResult<bool> {
    let value = turns::load(db, turn)?;
    if !matches!(&value.state, TurnTaskState::RecoveryRequired { reason } if reason == "agent.core_restarted")
    {
        return Ok(false);
    }
    let last: Option<String> = db
        .query_row(
            "SELECT id FROM turn_tasks WHERE session_id=?1 ORDER BY rowid DESC LIMIT 1",
            [value.session_id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    Ok(last.as_deref() == Some(turn.to_string().as_str())
        && session::load(db, value.session_id)?.active_turn.is_none())
}

/// Whether Aura continues `turn` by itself at `now`.
pub fn decide(db: &Connection, turn: TurnTaskId, now: UtcTimestamp) -> StoreResult<Decision> {
    if !cut_off(db, turn)? {
        return Ok(Decision::Moved);
    }
    let mut continued = 0;
    let mut current = turn;
    while let Some(record) = read(db, current)? {
        if current == turn
            && seconds_between(turns::load(db, turn)?.created_at, now) < RESTART_LOOP_SECONDS
        {
            return Ok(Decision::RestartLoop);
        }
        if seconds_between(record.restarted_at, now) >= WINDOW_SECONDS {
            break;
        }
        continued += 1;
        if continued >= LIMIT {
            return Ok(Decision::LimitReached);
        }
        current = record.after_turn_task_id;
    }
    Ok(Decision::Continue)
}

/// Records that Aura said "继续" by itself in `turn`, continuing `after`.
pub fn record(
    tx: &Transaction<'_>,
    turn: TurnTaskId,
    after: TurnTaskId,
    cause: CoreRestartCause,
    restarted_at: UtcTimestamp,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let record = AutoContinued { cause, restarted_at, after_turn_task_id: after };
    facts::append(tx, EventId::new(), &turn.to_string(), KIND, &record, at)
}

/// What the conversation says about a turn Aura continued by itself.
pub fn view(db: &Connection, turn: TurnTaskId) -> StoreResult<Option<AgentAutoContinuedView>> {
    Ok(read(db, turn)?.map(|record| AgentAutoContinuedView {
        cause: record.cause,
        restarted_at: record.restarted_at,
    }))
}

fn seconds_between(earlier: UtcTimestamp, later: UtcTimestamp) -> i64 {
    (later.value() - earlier.value()).whole_seconds()
}

/// A record in a shape this version does not know (a later version's) is no record.
fn read(db: &Connection, turn: TurnTaskId) -> StoreResult<Option<AutoContinued>> {
    let raw: Option<serde_json::Value> = facts::latest(db, &turn.to_string(), KIND)?;
    Ok(raw.and_then(|value| serde_json::from_value(value).ok()))
}
