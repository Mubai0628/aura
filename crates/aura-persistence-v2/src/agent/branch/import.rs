//! Making a branch: the source conversation up to one finished turn. What it writes belongs to the
//! branch alone, so deleting either conversation leaves the other whole.
//!
//! [`begin`] carries the newest turns while the user waits and plans the rest; the older turns
//! follow one step at a time ([`super::copying`]), and the history the model is sent is put in
//! order when the last of them has been carried.
use super::capture;
use super::copying;
use super::record::{
    self, BranchCopying, BranchRecord, CarriedBody, CarriedIndex, CarriedResult, CarriedTurn,
    PendingTurn,
};
use super::timing::BranchTiming;
use crate::agent::turns;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::Transaction;

/// A branch to make. Core has already created the conversation itself.
pub struct BranchImport {
    pub branch: SessionId,
    pub source: SessionId,
    pub source_title: String,
    pub turn_task_id: TurnTaskId,
    pub command_invocation_id: CommandInvocationId,
    pub at: UtcTimestamp,
}

/// What a branch carries before it
/// opens: its newest turns, newest first, at most `turns` of them and no more once `budget` has
/// passed. The newest is always carried; the rest follow in the background.
#[derive(Clone, Copy, Debug)]
pub struct FirstBatch {
    pub turns: usize,
    pub budget: std::time::Duration,
}

impl FirstBatch {
    /// Everything at once, however long it takes.
    pub const WHOLE: Self = Self { turns: usize::MAX, budget: std::time::Duration::MAX };
}

/// Where the branch point is in the source: among the turns it carried itself, or its own.
enum Place {
    Carried(usize),
    Own(usize),
}

/// The whole branch in one transaction: either all of it is there or none of it is.
pub fn import(
    tx: &Transaction<'_>,
    request: &BranchImport,
) -> StoreResult<(AgentBranchOrigin, BranchTiming)> {
    let (origin, mut timing) = begin(tx, request, FirstBatch::WHOLE)?;
    copying::finish(tx, request.branch, request.at, &mut timing)?;
    Ok((origin, timing))
}

/// Opens the branch with its newest turns ([`FirstBatch`]) and plans the older ones.
pub fn begin(
    tx: &Transaction<'_>,
    request: &BranchImport,
    first: FirstBatch,
) -> StoreResult<(AgentBranchOrigin, BranchTiming)> {
    let mut timing = BranchTiming::default();
    let (branch, source, at) = (request.branch, request.source, request.at);
    if let Some(existing) = record::load(tx, branch)? {
        // The same command made it before: answer with what it made.
        if existing.command_invocation_id != request.command_invocation_id
            || existing.origin.source_session_id != source
            || existing.origin.source_turn_task_id != request.turn_task_id
        {
            return Err(Error::Conflict);
        }
        return Ok((existing.origin, timing));
    }
    let own: i64 = tx.query_row(
        "SELECT COUNT(*) FROM turn_tasks WHERE session_id=?1",
        [branch.to_string()],
        |row| row.get(0),
    )?;
    if branch == source || own != 0 {
        return Err(Error::Invalid("agent.branch_target_not_new"));
    }
    let source_branch = record::load(tx, source)?;
    if source_branch.as_ref().is_some_and(|b| b.copying.is_some()) {
        // A branch still being carried is not yet whole enough to branch again.
        return Err(Error::Invalid("agent.branch_source_copying"));
    }
    let carried_ids: Vec<TurnTaskId> = source_branch
        .as_ref()
        .map(|b| b.turns.iter().map(|turn| turn.turn_task_id).collect())
        .unwrap_or_default();
    let mut statement =
        tx.prepare("SELECT id FROM turn_tasks WHERE session_id=?1 ORDER BY rowid")?;
    let own_ids = statement
        .query_map([source.to_string()], |row| row.get::<_, String>(0))?
        .map(|id| id?.parse::<TurnTaskId>().map_err(|_| Error::Conflict))
        .collect::<StoreResult<Vec<_>>>()?;
    drop(statement);
    let place = if let Some(i) = carried_ids.iter().position(|id| *id == request.turn_task_id) {
        Place::Carried(i)
    } else if let Some(i) = own_ids.iter().position(|id| *id == request.turn_task_id) {
        Place::Own(i)
    } else {
        return Err(Error::Invalid("agent.branch_turn_unknown"));
    };
    let turn_number = match place {
        Place::Carried(i) => i + 1,
        Place::Own(i) => carried_ids.len() + i + 1,
    };
    // Every turn the branch carries, oldest first: the source's carried turns up to the branch
    // point, then its own.
    let (shared, own_chosen) = match place {
        Place::Carried(i) => (i + 1, Vec::new()),
        Place::Own(last) => (carried_ids.len(), own_ids[..=last].to_vec()),
    };
    match place {
        Place::Carried(i) => {
            let source_branch = source_branch.as_ref().ok_or(Error::Conflict)?;
            let display: record::CarriedDisplay =
                record::read_json(tx, source, source_branch.turns[i].display_artifact_id)?;
            ensure_finished(&display.view.turn)?;
        }
        Place::Own(_) => ensure_finished(&turns::load(tx, request.turn_task_id)?)?,
    }
    let plan: Vec<PendingTurn> = carried_ids[..shared]
        .iter()
        .map(|id| PendingTurn { turn_task_id: *id, shared: true })
        .chain(own_chosen.iter().map(|id| PendingTurn { turn_task_id: *id, shared: false }))
        .collect();
    let origin = AgentBranchOrigin {
        source_session_id: source,
        source_title: request.source_title.clone(),
        source_turn_task_id: request.turn_task_id,
        turn_number: u32::try_from(turn_number).map_err(|_| Error::Conflict)?,
        created_at: at,
    };
    let mut index = CarriedIndex::default();
    let mut snapshot = None;
    let carried = newest_first(plan.len(), first, |place| {
        carry(tx, branch, source, place, &plan[place], &mut index, &mut snapshot, &mut timing, at)
    })?;
    let remaining = plan[..plan.len() - carried.len()].to_vec();
    timing.turns = carried.len();
    let value = BranchRecord {
        session_id: branch,
        command_invocation_id: request.command_invocation_id,
        origin: origin.clone(),
        turns: carried,
        compactions: Vec::new(),
        index_artifact_id: record::put_json(tx, branch, &index, at)?,
        copying: Some(BranchCopying { remaining, shared, own: own_chosen, failed: None }),
    };
    record::save(tx, &value, at)?;
    if value.copying.as_ref().is_some_and(|copying| copying.remaining.is_empty()) {
        // Every turn is already there: the history is put in order at once.
        copying::finish(tx, branch, at, &mut timing)?;
    }
    Ok((origin, timing))
}

/// Carries the newest of `places` first until `first` says stop, and returns them oldest first.
fn newest_first<T>(
    places: usize,
    first: FirstBatch,
    mut carry: impl FnMut(usize) -> StoreResult<T>,
) -> StoreResult<Vec<T>> {
    let started = std::time::Instant::now();
    let mut carried = Vec::new();
    for place in (0..places).rev().take(first.turns.max(1)) {
        if !carried.is_empty() && started.elapsed() >= first.budget {
            break;
        }
        carried.push(carry(place)?);
    }
    carried.reverse();
    Ok(carried)
}

/// What a step reads of the source once and keeps: its history groups and rows. The turns a
/// branch carries are finished, so neither changes while the branch is being made.
pub struct SourceSnapshot {
    groups: Vec<aura_context_domain::HistoryAtomicGroupPreimageV1>,
    rows: Vec<crate::context::read::Row>,
}

/// Carries one turn at its place in the whole list, adding its texts, results and calls to the
/// index.
#[allow(clippy::too_many_arguments)]
pub(super) fn carry(
    tx: &Transaction<'_>,
    branch: SessionId,
    source: SessionId,
    place: usize,
    pending: &PendingTurn,
    index: &mut CarriedIndex,
    snapshot: &mut Option<SourceSnapshot>,
    timing: &mut BranchTiming,
    at: UtcTimestamp,
) -> StoreResult<CarriedTurn> {
    if pending.shared {
        // A turn the source itself carried: shared, not copied.
        let source_branch = record::load(tx, source)?.ok_or(Error::Conflict)?;
        let turn = source_branch.turn_at(place).ok_or(Error::Conflict)?.clone();
        if turn.turn_task_id != pending.turn_task_id {
            return Err(Error::Conflict);
        }
        record::share(tx, turn.model_artifact_id, source, branch)?;
        record::share(tx, turn.display_artifact_id, source, branch)?;
        let source_index: CarriedIndex =
            record::read_json(tx, source, source_branch.index_artifact_id)?;
        for body in source_index.bodies.into_iter().filter(|body| body.turn == place) {
            record::share(tx, body.artifact_id, source, branch)?;
            index.bodies.push(body);
        }
        for result in source_index.results.into_iter().filter(|result| result.turn == place) {
            record::share(tx, result.artifact_id, source, branch)?;
            index.results.push(result);
        }
        index.calls.extend(source_index.calls.into_iter().filter(|(_, turn)| *turn == place));
        return Ok(turn);
    }
    if snapshot.is_none() {
        let groups = timing.time("source.groups", || crate::context::history(tx, source))?;
        let rows =
            timing.time("source.rows", || crate::context::read::own_rows(tx, source, i64::MAX))?;
        *snapshot = Some(SourceSnapshot { groups, rows });
    }
    let source_now = snapshot.as_ref().ok_or(Error::Conflict)?;
    let turn = turns::load(tx, pending.turn_task_id)?;
    if turn.session_id != source {
        return Err(Error::Conflict);
    }
    let captured = capture::capture(tx, &turn, &source_now.groups, &source_now.rows, timing)?;
    let started = std::time::Instant::now();
    for text in captured.texts {
        index.bodies.push(CarriedBody {
            turn: place,
            source: text.source,
            artifact_id: record::put_text(tx, branch, &text.text, at)?,
            total_bytes: text.text.len() as u64,
            sealed_revision: text.sealed_revision,
        });
    }
    for (tool_call_id, text) in captured.results {
        index.results.push(CarriedResult {
            turn: place,
            tool_call_id,
            artifact_id: record::put_text(tx, branch, &text, at)?,
            total_bytes: text.len() as u64,
        });
    }
    for entry in &captured.display.activity {
        index.calls.push((entry.summary.tool_call_id, place));
    }
    let carried = CarriedTurn {
        turn_task_id: turn.turn_task_id,
        root_user_event_id: turn.root_user_event_id,
        model_artifact_id: record::put_json(tx, branch, &captured.model, at)?,
        display_artifact_id: record::put_json(tx, branch, &captured.display, at)?,
    };
    *timing.phases.entry("write.turn").or_default() += started.elapsed().as_micros();
    Ok(carried)
}

/// Only a turn that finished with an answer can be branched from.
fn ensure_finished(turn: &TurnTask) -> StoreResult<()> {
    if matches!(turn.state, TurnTaskState::Completed { .. }) {
        Ok(())
    } else {
        Err(Error::Invalid("agent.branch_turn_unfinished"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn the_first_batch_is_the_newest_turns_until_its_time_is_up() {
        let all = newest_first(10, FirstBatch { turns: 6, budget: Duration::MAX }, Ok);
        assert_eq!(all.expect("carried"), [4, 5, 6, 7, 8, 9], "at most six, oldest first");
        let few = newest_first(3, FirstBatch { turns: 6, budget: Duration::MAX }, Ok);
        assert_eq!(few.expect("carried"), [0, 1, 2]);
        // A slow turn ends the batch once the time is up, after the newest.
        let slow = newest_first(10, FirstBatch { turns: 6, budget: Duration::ZERO }, |place| {
            std::thread::sleep(Duration::from_millis(1));
            Ok(place)
        });
        assert_eq!(slow.expect("carried"), [9], "the newest is always carried");
    }
}
