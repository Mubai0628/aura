//! A branch's older turns, carried one short step at a time after it opened
//! with its newest ones, so making a branch never holds the store for the whole conversation and
//! the branch is usable meanwhile. When the last turn has been carried, [`finish`] puts the
//! history the model is sent in order; until then a run in the branch waits.
use super::import::{SourceSnapshot, carry};
use super::record::{self, CarriedCompaction, CarriedIndex, CarriedModel, CarriedUserMessage};
use super::timing::BranchTiming;
use crate::agent::{facts, turns};
use crate::context::compaction::Compaction;
use aura_agent_domain::AgentBranchCopyingView;
use aura_context_domain::HistoryAtomicGroupPreimageV1;
use aura_domain_common::*;
use aura_model_domain::LogicalConversationItem;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact};
use rusqlite::{Connection, Transaction, params};

/// What one step did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CopyStep {
    /// One more older turn is in the branch; more may follow.
    Carried,
    /// Every turn is in the branch and its history is in order.
    Finished,
    /// The branch is gone, or its copy stopped earlier; there is nothing to do.
    Stopped,
}

/// Carries the next older turn, or puts the history in order once they are all there.
pub fn step(
    tx: &Transaction<'_>,
    branch: SessionId,
    at: UtcTimestamp,
    snapshot: &mut Option<SourceSnapshot>,
    timing: &mut BranchTiming,
) -> StoreResult<CopyStep> {
    let Some(mut value) = record::load(tx, branch)? else { return Ok(CopyStep::Stopped) };
    let Some(copying) = &mut value.copying else { return Ok(CopyStep::Finished) };
    if copying.failed.is_some() {
        return Ok(CopyStep::Stopped);
    }
    let Some(next) = copying.remaining.pop() else {
        finish(tx, branch, at, timing)?;
        return Ok(CopyStep::Finished);
    };
    // The popped turn's place in the whole list is the number still before it.
    let place = copying.remaining.len();
    let source = value.origin.source_session_id;
    let old_index = value.index_artifact_id;
    let mut index: CarriedIndex = record::read_json(tx, branch, old_index)?;
    let carried = carry(tx, branch, source, place, &next, &mut index, snapshot, timing, at)?;
    value.turns.insert(0, carried);
    value.index_artifact_id = record::put_json(tx, branch, &index, at)?;
    artifact::release_owner(tx, old_index, &branch.to_string())?;
    record::save(tx, &value, at)?;
    Ok(CopyStep::Carried)
}

/// Puts the carried history in order: the tool groups and user messages the model's requests and
/// reads find, and the branch's own copies of the compactions made during the carried turns.
pub fn finish(
    tx: &Transaction<'_>,
    branch: SessionId,
    at: UtcTimestamp,
    timing: &mut BranchTiming,
) -> StoreResult<()> {
    let mut value = record::load(tx, branch)?.ok_or(Error::Conflict)?;
    let Some(copying) = value.copying.take() else { return Ok(()) };
    if !copying.remaining.is_empty() || copying.failed.is_some() {
        return Err(Error::Conflict);
    }
    let started = std::time::Instant::now();
    let source = value.origin.source_session_id;
    let mut groups: Vec<HistoryAtomicGroupPreimageV1> = Vec::new();
    let mut users: Vec<CarriedUserMessage> = Vec::new();
    for turn in &value.turns {
        let model: CarriedModel = record::read_json(tx, branch, turn.model_artifact_id)?;
        groups.extend(model.groups.iter().cloned());
        for item in &model.items {
            if let LogicalConversationItem::User { source_event_id, content, .. } = item {
                users
                    .push(CarriedUserMessage { event_id: *source_event_id, text: content.clone() });
            }
        }
    }
    // The compactions of the source's carried turns, then those made while its own turns ran,
    // in the order they were made.
    let mut compactions = Vec::new();
    if copying.shared != 0 {
        let source_branch = record::load(tx, source)?.ok_or(Error::Conflict)?;
        for copy in source_branch.compactions.iter().filter(|copy| copy.turn < copying.shared) {
            let made = crate::context::compaction::load(tx, copy.compaction_id)?;
            compactions.push(copy_compaction(tx, &made, branch, copy.original_id, copy.turn, at)?);
        }
    }
    if !copying.own.is_empty() {
        let mut statement = tx.prepare(
            "SELECT payload FROM context_compactions WHERE session_id=?1 AND state='completed' \
             ORDER BY json_extract(payload,'$.sequence')",
        )?;
        let made = statement
            .query_map([source.to_string()], |row| row.get::<_, String>(0))?
            .map(|raw| aura_persistence_core::record::decode::<Compaction>(&raw?))
            .collect::<StoreResult<Vec<_>>>()?;
        drop(statement);
        for compaction in made {
            let run = turns::load_run(tx, compaction.agent_run_id)?;
            if let Some(k) = copying.own.iter().position(|id| *id == run.turn_task_id) {
                let original = compaction.id;
                let place = copying.shared + k;
                compactions.push(copy_compaction(tx, &compaction, branch, original, place, at)?);
            }
        }
    }
    for group in &groups {
        let key = group.group_ref().map_err(|_| Error::Invalid("history atomic group"))?;
        tx.execute(
            "INSERT INTO history_atomic_groups SELECT ?1,COALESCE(MAX(ordinal),0)+1,?2,?3 \
             FROM history_atomic_groups WHERE session_id=?1",
            params![
                branch.to_string(),
                aura_persistence_core::record::encode(&key)?,
                aura_persistence_core::record::encode(group)?
            ],
        )?;
    }
    for message in &users {
        facts::append(tx, EventId::new(), &record::owner(branch), record::USER_KIND, message, at)?;
    }
    value.compactions = compactions;
    record::save(tx, &value, at)?;
    *timing.phases.entry("write.rows").or_default() += started.elapsed().as_micros();
    Ok(())
}

/// Records why a branch's copy stopped: the branch keeps the turns it has, and a run in it is
/// refused with that reason instead of waiting.
pub fn fail(
    tx: &Transaction<'_>,
    branch: SessionId,
    reason: &str,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let Some(mut value) = record::load(tx, branch)? else { return Ok(()) };
    let Some(copying) = &mut value.copying else { return Ok(()) };
    copying.failed = Some(reason.chars().take(300).collect());
    record::save(tx, &value, at)
}

/// How far a branch's copy is, while it is not finished.
pub fn progress(
    db: &Connection,
    session: SessionId,
) -> StoreResult<Option<AgentBranchCopyingView>> {
    Ok(record::load(db, session)?.and_then(|value| {
        let copying = value.copying.as_ref()?;
        Some(AgentBranchCopyingView {
            copied_turns: u32::try_from(value.turns.len()).unwrap_or(u32::MAX),
            total_turns: u32::try_from(value.turns.len() + copying.remaining.len())
                .unwrap_or(u32::MAX),
            failed: copying.failed.is_some(),
        })
    }))
}

/// The branches whose copy has not finished, with the conversation each copies from.
pub fn unfinished(db: &Connection) -> StoreResult<Vec<(SessionId, SessionId, bool)>> {
    // Every branch record is an owner of this prefix: a range of the index.
    let mut statement = db.prepare(
        "SELECT e.payload FROM events e \
         WHERE e.owner>='agent-branch:' AND e.owner<'agent-branch;' AND e.kind=?1 AND e.version=1 \
         AND e.sequence=(SELECT MAX(sequence) FROM events WHERE owner=e.owner AND kind=?1) \
         AND json_extract(e.payload,'$.copying') IS NOT NULL",
    )?;
    let rows = statement
        .query_map([record::KIND], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|raw| {
            let value: record::BranchRecord = aura_persistence_core::record::decode(&raw)?;
            let failed = value.copying.as_ref().is_some_and(|c| c.failed.is_some());
            Ok((value.session_id, value.origin.source_session_id, failed))
        })
        .collect()
}

/// Whether a branch is still copying from this conversation, so it must not be deleted yet.
pub fn copying_from(db: &Connection, source: SessionId) -> StoreResult<bool> {
    Ok(unfinished(db)?.iter().any(|(_, from, failed)| *from == source && !failed))
}

/// The branch's own copy of a compaction made during a carried turn. Its summary is unchanged, so
/// the model is sent exactly what it was; its artifacts are the branch's.
fn copy_compaction(
    tx: &Transaction<'_>,
    value: &Compaction,
    branch: SessionId,
    original_id: ContextProjectionSnapshotId,
    turn: usize,
    at: UtcTimestamp,
) -> StoreResult<CarriedCompaction> {
    let owner = value.id.to_string();
    let result_id = value.result_artifact_id.ok_or(Error::Conflict)?;
    let result = artifact::read(tx, result_id, &owner)?;
    let sources = artifact::read(tx, value.source_artifact_id, &owner)?;
    let mut copy = value.clone();
    copy.id = ContextProjectionSnapshotId::new();
    copy.session_id = branch;
    // The model attempts that wrote it stay the source's; the copy is complete as it is.
    copy.attempts = Vec::new();
    copy.result_artifact_id = Some(ArtifactId::new());
    copy.source_artifact_id = ArtifactId::new();
    let copy_owner = copy.id.to_string();
    artifact::put(
        tx,
        copy.result_artifact_id.ok_or(Error::Conflict)?,
        &copy_owner,
        "application/vnd.aura.compaction+json",
        &result,
        at,
    )?;
    artifact::put(
        tx,
        copy.source_artifact_id,
        &copy_owner,
        "application/vnd.aura.compaction-sources+json",
        &sources,
        at,
    )?;
    tx.execute(
        "INSERT INTO context_compactions VALUES(?1,?2,'completed',?3,?4)",
        params![
            copy.id.to_string(),
            branch.to_string(),
            i64::try_from(copy.revision.value()).map_err(|_| Error::Conflict)?,
            aura_persistence_core::record::encode(&copy)?
        ],
    )?;
    Ok(CarriedCompaction { compaction_id: copy.id, original_id, turn })
}
