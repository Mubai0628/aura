//! What a restart of Core leaves of the sub-agents that were working. One the restart cut off is
//! told to its main agent as interrupted and waits for it to say go on (`agents.send`) or stop;
//! nothing it was doing runs again by itself. One whose turn ended before the restart without the
//! team hearing reports that end, and one whose task sheet was never handed over is stopped.
use crate::{Member, MemberState, Report, ReportStatus, TurnEnd, store, team::from_turn_end};
use aura_domain_common::{TurnTaskId, UtcTimestamp};
use aura_persistence_core::StoreResult;
use rusqlite::Transaction;

/// How a running sub-agent's last turn stands once the restart settled every run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LastTurn {
    /// It never had one: its task sheet was never handed over.
    None,
    /// The restart cut it off, or stopped it while it waited.
    CutOff(TurnTaskId),
    /// It ended before the restart, and the team never heard; why it stopped, in words, when it
    /// did not complete.
    Ended(TurnEnd, Option<String>),
}

/// The reason a turn the restart cut off ended with.
pub const RESTARTED: &str = "agent.core_restarted";

/// Settles `member`, which was running when Core stopped. A member no longer running is left as
/// it is, so settling the same restart twice writes nothing more.
pub fn after_restart(
    tx: &Transaction<'_>,
    member: &Member,
    last: LastTurn,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let child = member.child_session_id;
    match last {
        LastTurn::CutOff(turn) => {
            let end = TurnEnd {
                turn_task_id: turn,
                outcome: "recovery_required".to_owned(),
                answer: None,
                reason: Some(RESTARTED.to_owned()),
            };
            store::record_turn_end(
                tx,
                child,
                &end,
                &interrupted(),
                &format!("restart:{turn}"),
                at,
            )?;
        }
        LastTurn::Ended(end, cause) => {
            let source = store::turn_end_source(end.turn_task_id);
            let report = from_turn_end(&end, cause.as_deref());
            store::record_turn_end(tx, child, &end, &report, &source, at)?;
        }
        LastTurn::None => {
            let Some(current) = store::member_by_child(tx, child)? else { return Ok(()) };
            if current.state == MemberState::Running {
                let mut next = current.clone();
                next.state = MemberState::Stopped;
                next.revision += 1;
                next.updated_at = at;
                store::save_member(tx, &current, &next)?;
            }
        }
    }
    Ok(())
}

/// The report the main agent is given for a sub-agent the restart cut off.
fn interrupted() -> Report {
    Report {
        status: ReportStatus::Blocked,
        summary: "Aura 重启时它这一轮被中断了，没给出最终回答。结果不明的操作不会自动重做；\
                  要它接着干就用 agents.send（它会从自己的记录里看到哪些调用没跑完），不要了就用 agents.stop。"
            .to_owned(),
        question: None,
        files: Vec::new(),
        checks: Vec::new(),
        remaining: None,
    }
}
