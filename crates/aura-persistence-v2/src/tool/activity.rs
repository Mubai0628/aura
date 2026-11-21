//! Session-bound pagination and real start/result facts for the desktop.
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use aura_tool_contract::*;
use aura_tool_projection::*;
use rusqlite::{Connection, OptionalExtension};
mod browser;
mod chrome;
mod computer;
mod display;
mod entry;
pub(crate) use display::title as tool_title;
mod git;
mod mcp;
mod memory;
mod network_route;
mod package;
mod query;
mod shell;
mod team;
mod unbound;
mod user_text;
mod workspace_init;

/// What a person reads first about a call of tool `name` that did not succeed, for the screens
/// beside the conversation's rows; none for a call that succeeded or carries no error.
pub(crate) fn failure_sentence(
    name: &str,
    result: &aura_tool_contract::ToolResultV1,
) -> Option<String> {
    user_text::line(name, result)
}

/// The short line of a call that was not run for `reason`.
pub(crate) fn not_run_line(reason: ToolNotExecutedReasonV1) -> &'static str {
    display::not_run_text(reason)
}

/// The display class of one stored error category.
///
/// `Validation` covers both the argument envelope and the tool's own schema, because
/// what a person has to do about either is the same: nothing ran, and the request has to
/// be corrected. A `cancelled` or `transport` failure is deliberately `Other`: it is a
/// real failure with a real cause, and this vocabulary does not claim to know which
/// correction it needs.
fn classify(category: aura_domain_common::ErrorCategory) -> ToolCallErrorClassV1 {
    use aura_domain_common::ErrorCategory as Category;
    match category {
        Category::NotFound => ToolCallErrorClassV1::NotFound,
        Category::Permission => ToolCallErrorClassV1::Permission,
        Category::Conflict => ToolCallErrorClassV1::Conflict,
        Category::Validation => ToolCallErrorClassV1::InvalidRequest,
        Category::Unavailable | Category::Compatibility => ToolCallErrorClassV1::Unavailable,
        Category::Cancelled | Category::Transport | Category::Protocol | Category::Internal => {
            ToolCallErrorClassV1::Other
        }
    }
}

/// The attempt a call was given, if it was given one: the lowest ordinal, the only one an
/// admission reserves.
pub(crate) fn attempt_of(
    db: &Connection,
    call: ToolCallId,
) -> StoreResult<Option<aura_execution_contract::ExecutionAttempt>> {
    let payload: Option<String> = db
        .query_row(
            "SELECT payload FROM execution_attempts WHERE operation_id=\
             (SELECT operation_id FROM tool_calls WHERE id=?1) ORDER BY ordinal LIMIT 1",
            [call.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    payload.map(|payload| aura_persistence_core::record::decode(&payload)).transpose()
}

/// Whether the run is over: completed, cancelled or waiting for recovery.
fn run_ended(db: &Connection, run: AgentRunId) -> StoreResult<bool> {
    let state: Option<String> = db
        .query_row("SELECT state FROM agent_runs WHERE id=?1", [run.to_string()], |r| r.get(0))
        .optional()?;
    Ok(matches!(state.as_deref(), Some("completed" | "cancelled" | "recovery_required")))
}

pub fn page(
    db: &Connection,
    input: &AgentToolActivityRequest,
) -> StoreResult<AgentToolActivityPage> {
    if input.limit == 0 || input.limit > 100 {
        return Err(Error::Invalid("activity limit"));
    }
    let mut carried = Vec::new();
    let mut unreadable_call_ids = Vec::new();
    let rows = if let Some(ids) = &input.call_ids {
        if ids.is_empty()
            || ids.len() > input.limit as usize
            || input.before_call_id.is_some()
            || ids.iter().collect::<std::collections::BTreeSet<_>>().len() != ids.len()
        {
            return Err(Error::Invalid("visible activity identities"));
        }
        // A conversation that does not exist is refused; inside one, a call is read only there.
        db.query_row("SELECT 1 FROM sessions WHERE id=?1", [input.session_id.to_string()], |_| {
            Ok(())
        })?;
        let mut rows = Vec::with_capacity(ids.len());
        // A call of a turn a branch carried is shown from what the branch kept of it.
        carried = crate::agent::branch::carried_activity(
            db,
            input.session_id,
            ids,
            &mut unreadable_call_ids,
        )?;
        for id in ids.iter().filter(|id| {
            !carried.iter().any(|(call, _)| call == *id) && !unreadable_call_ids.contains(*id)
        }) {
            // A call not stored (yet) - one the model wrote whose batch is not admitted, or
            // never was because its step was interrupted - is left out rather than failing the
            // read of every call asked with it.
            rows.extend(db.query_row("SELECT c.id,c.batch_id FROM tool_calls c JOIN tool_batches b ON c.batch_id=b.id WHERE c.id=?1 AND b.session_id=?2",
                [id.to_string(), input.session_id.to_string()], |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?))).optional()?);
        }
        rows
    } else {
        let before: i64 = if let Some(call) = input.before_call_id {
            db.query_row("SELECT c.rowid FROM tool_calls c JOIN tool_batches b ON c.batch_id=b.id WHERE c.id=?1 AND b.session_id=?2", [call.to_string(), input.session_id.to_string()], |r| r.get(0))?
        } else {
            i64::MAX
        };
        let mut statement = db.prepare("SELECT c.id,c.batch_id FROM tool_calls c JOIN tool_batches b ON c.batch_id=b.id WHERE b.session_id=?1 AND c.rowid<?2 ORDER BY c.rowid DESC LIMIT ?3")?;
        statement
            .query_map(
                rusqlite::params![input.session_id.to_string(), before, input.limit + 1],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )?
            .collect::<Result<Vec<_>, _>>()?
    };
    let more = rows.len() > input.limit as usize;
    let mut calls = Vec::new();
    let mut last_scanned = None;
    for (id, batch_id) in rows.into_iter().take(input.limit as usize) {
        let id: ToolCallId = id.parse().map_err(|_| Error::Conflict)?;
        last_scanned = Some(id);
        let row = batch_id
            .parse()
            .map_err(|_| Error::Conflict)
            .and_then(|batch_id| entry::project(db, input.session_id, id, batch_id));
        match row {
            Ok(entry) => calls.push(entry),
            Err(error) if row_data_error(&error) => unreadable_call_ids.push(id),
            Err(error) => return Err(error),
        }
    }
    calls.extend(carried.into_iter().map(|(_, entry)| entry));
    Ok(AgentToolActivityPage {
        session_id: input.session_id,
        calls,
        unreadable_call_ids,
        next_before_call_id: if more { last_scanned } else { None },
    })
}

// Only a fault in one stored record can be isolated. Database access and I/O errors affect the read.
pub(crate) fn row_data_error(error: &Error) -> bool {
    matches!(
        error,
        Error::Json(_)
            | Error::Invalid(_)
            | Error::Conflict
            | Error::Sqlite(
                rusqlite::Error::QueryReturnedNoRows
                    | rusqlite::Error::InvalidColumnType(..)
                    | rusqlite::Error::FromSqlConversionFailure(..)
            )
    )
}
