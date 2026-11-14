//! What a restart of Core leaves of the sub-agents, settled inside the startup's own transaction:
//! each one that was working, or waiting for the user, is told to its main agent as interrupted
//! and waits for it to say go on or stop. Aura continues none of them by itself, and nothing they
//! were doing runs again.
use super::*;
use aura_agent_team::restart::{self, LastTurn, RESTARTED};
use aura_persistence_core::{OptionalExtension, PersistenceCoreError, StoreResult, Transaction};
use aura_persistence_v2::agent::{
    recovery::{self, InterruptedTurn},
    session,
};

/// Settles the sub-agents a previous process left running and takes their turns out of the ones
/// Aura may continue by itself. A sub-agent that cannot be settled is left as it was, and why is
/// logged: it never keeps Core from starting.
pub(in crate::agent_application) fn recover_after_restart(
    tx: &Transaction<'_>,
    interrupted: &mut Vec<InterruptedTurn>,
    at: UtcTimestamp,
) -> StoreResult<()> {
    for member in team_store::running_members(tx)? {
        tx.execute_batch("SAVEPOINT aura_team_restart")?;
        match settle(tx, &member, at) {
            Ok(()) => tx.execute_batch("RELEASE aura_team_restart")?,
            Err(error) => {
                tx.execute_batch("ROLLBACK TO aura_team_restart; RELEASE aura_team_restart")?;
                append_core_log(
                    "agents.restart_settle_failed",
                    serde_json::json!({
                        "session_id": member.child_session_id,
                        "reason": error.to_string(),
                    }),
                );
            }
        }
    }
    let mut kept = Vec::with_capacity(interrupted.len());
    for cut in interrupted.drain(..) {
        if team_store::member_by_child(tx, cut.session_id)?.is_none() {
            kept.push(cut);
        }
    }
    *interrupted = kept;
    Ok(())
}

fn settle(
    tx: &Transaction<'_>,
    member: &aura_agent_team::Member,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let child = member.child_session_id;
    // A turn still waiting for an approval, an answer or a context decision stops as the restart
    // stopped the running ones: what it waited to run does not run.
    if let Some(turn) = session::load(tx, child)?.active_turn {
        let run = turns::load(tx, turn)?.agent_run_id;
        recovery::interrupt(tx, run, RESTARTED, false, at)?;
    }
    let last = match last_turn(tx, child)? {
        None => LastTurn::None,
        Some(turn) => match turn_end(&turn) {
            Some(end)
                if end.outcome == "recovery_required"
                    && end.reason.as_deref() == Some(RESTARTED) =>
            {
                LastTurn::CutOff(turn.turn_task_id)
            }
            Some(end) => LastTurn::Ended(end, stop_cause(tx, &turn)?),
            None => return Ok(()),
        },
    };
    restart::after_restart(tx, member, last, at)
}

fn last_turn(
    db: &aura_persistence_core::Connection,
    session: SessionId,
) -> StoreResult<Option<TurnTask>> {
    let id: Option<String> = db
        .query_row(
            "SELECT id FROM turn_tasks WHERE session_id=?1 ORDER BY rowid DESC LIMIT 1",
            [session.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    id.map(|id| {
        let id = id.parse().map_err(|_| PersistenceCoreError::Invalid("turn task id"))?;
        turns::load(db, id)
    })
    .transpose()
}

/// How `turn` ended, as the team records it; none while it is under way.
pub(in crate::agent_application) fn turn_end(turn: &TurnTask) -> Option<aura_agent_team::TurnEnd> {
    let (outcome, answer, reason) = match &turn.state {
        TurnTaskState::Completed { final_report } => {
            ("completed", Some(final_report.clone()), None)
        }
        TurnTaskState::Failed { reason } => ("failed", None, Some(reason.clone())),
        TurnTaskState::Cancelled => ("cancelled", None, None),
        TurnTaskState::RecoveryRequired { reason } => {
            ("recovery_required", None, Some(reason.clone()))
        }
        _ => return None,
    };
    Some(aura_agent_team::TurnEnd {
        turn_task_id: turn.turn_task_id,
        outcome: outcome.to_owned(),
        answer,
        reason,
    })
}

/// Why `turn` stopped before it finished, in words the user reads too; none when it completed or
/// was cancelled.
pub(in crate::agent_application) fn stop_cause(
    db: &aura_persistence_core::Connection,
    turn: &TurnTask,
) -> StoreResult<Option<String>> {
    let (TurnTaskState::Failed { reason } | TurnTaskState::RecoveryRequired { reason }) =
        &turn.state
    else {
        return Ok(None);
    };
    let cause = aura_persistence_v2::agent::model_context::stop_cause(
        db,
        turn.agent_run_id,
        reason,
        false,
    )?;
    Ok(Some(cause.chinese()))
}
