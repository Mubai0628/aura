use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
use rusqlite::{Connection, Transaction, params};

pub fn load(db: &Connection, id: TurnTaskId) -> StoreResult<TurnTask> {
    let (revision, state, payload) = super::turn(db, id)?;
    let turn: TurnTask = serde_json::from_value(payload)?;
    if turn.turn_task_id != id
        || record::sql_u64(turn.revision.value())? != revision
        || turn_state(&turn.state) != state
    {
        return Err(Error::Conflict);
    }
    Ok(turn)
}
pub fn load_run(db: &Connection, id: AgentRunId) -> StoreResult<AgentRun> {
    let (state, revision, raw): (String, i64, String) = db.query_row(
        "SELECT state,revision,payload FROM agent_runs WHERE id=?1",
        [id.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let run: AgentRun = record::decode(&raw)?;
    if run.agent_run_id != id
        || revision != record::sql_u64(run.revision.value())?
        || state != run_state(&run.state)
    {
        return Err(Error::Conflict);
    }
    Ok(run)
}
pub fn insert(tx: &Transaction<'_>, turn: &TurnTask, run: &AgentRun) -> StoreResult<()> {
    if turn.state != TurnTaskState::Pending
        || turn.revision != Revision::INITIAL
        || run.revision != Revision::INITIAL
        || run.state != AgentRunState::PreparingContext
        || run.next_model_ordinal != 1
        || run.parent_agent_run_id.is_some()
        || run.agent_run_id != turn.agent_run_id
        || run.turn_task_id != turn.turn_task_id
    {
        return Err(Error::Conflict);
    }
    let root = super::session::user_message(tx, turn.session_id, turn.root_user_event_id)?;
    if root.session_id != turn.session_id {
        return Err(Error::Conflict);
    }
    super::insert_turn(tx, turn.turn_task_id, turn.session_id, &serde_json::to_value(turn)?)?;
    tx.execute(
        "INSERT INTO agent_runs VALUES(?1,?2,NULL,?3,1,?4)",
        params![
            run.agent_run_id.to_string(),
            run.turn_task_id.to_string(),
            run_state(&run.state),
            record::encode(run)?
        ],
    )?;
    Ok(())
}
pub fn save(tx: &Transaction<'_>, previous: &TurnTask, next: &TurnTask) -> StoreResult<()> {
    if matches!(next.state, TurnTaskState::Completed { .. }) {
        return Err(Error::Invalid("completion requires owning completion transaction"));
    }
    save_checked(tx, previous, next, false)
}
pub fn save_completed(
    tx: &Transaction<'_>,
    previous: &TurnTask,
    next: &TurnTask,
) -> StoreResult<()> {
    if !matches!(next.state, TurnTaskState::Completed { .. }) {
        return Err(Error::Conflict);
    }
    save_checked(tx, previous, next, false)
}
pub fn adopt_mode(
    tx: &Transaction<'_>,
    previous: &TurnTask,
    mode: AgentTaskMode,
    at: UtcTimestamp,
) -> StoreResult<TurnTask> {
    if previous.mode != AgentTaskMode::Normal
        || mode == AgentTaskMode::Normal
        || previous.state.terminal()
    {
        return Err(Error::Conflict);
    }
    let mut next = previous.clone();
    next.mode = mode;
    next.revision = Revision::new(previous.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    next.updated_at = at;
    save_checked(tx, previous, &next, true)?;
    Ok(next)
}
fn save_checked(
    tx: &Transaction<'_>,
    previous: &TurnTask,
    next: &TurnTask,
    adopt_mode: bool,
) -> StoreResult<()> {
    let mode_ok = if adopt_mode {
        previous.mode == AgentTaskMode::Normal && next.mode != AgentTaskMode::Normal
    } else {
        previous.mode == next.mode
    };
    if load(tx, previous.turn_task_id)? != *previous
        || previous.state.terminal()
        || previous.turn_task_id != next.turn_task_id
        || previous.session_id != next.session_id
        || previous.workspace_id != next.workspace_id
        || previous.root_user_event_id != next.root_user_event_id
        || previous.agent_run_id != next.agent_run_id
        || !mode_ok
        || previous.created_at != next.created_at
        || next.updated_at < previous.updated_at
        || next.revision.value()
            != previous.revision.value().checked_add(1).ok_or(Error::Conflict)?
    {
        return Err(Error::Conflict);
    }
    super::update_turn(
        tx,
        next.turn_task_id,
        record::sql_u64(previous.revision.value())?,
        turn_state(&next.state),
        &serde_json::to_value(next)?,
    )?;
    if let TurnTaskState::Completed { final_report } = &next.state {
        super::history::record_chunks(
            tx,
            &super::history::ChunkBody {
                owner: next.turn_task_id.to_string(),
                source: super::history::VisibleSource::Completion,
                revision: ContentHash::blake3(final_report.as_bytes()),
            },
            final_report,
            next.updated_at,
        )?;
    }
    Ok(())
}
pub fn save_run(tx: &Transaction<'_>, previous: &AgentRun, next: &AgentRun) -> StoreResult<()> {
    if next.prompt_bundle_snapshot_id != previous.prompt_bundle_snapshot_id {
        let turn = load(tx, next.turn_task_id)?;
        let bundle =
            super::model_context::bundle(tx, next.prompt_bundle_snapshot_id, turn.session_id)?;
        let pending: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM model_steps WHERE agent_run_id=?1 AND state IN('preparing','running'))", [next.agent_run_id.to_string()], |r| r.get(0))?;
        if bundle.turn_task_id != turn.turn_task_id || pending {
            return Err(Error::Conflict);
        }
    }
    if next.state == AgentRunState::Completed
        && !matches!(load(tx, next.turn_task_id)?.state, TurnTaskState::Completed { .. })
    {
        return Err(Error::Conflict);
    }
    if load_run(tx, previous.agent_run_id)? != *previous
        || next.agent_run_id != previous.agent_run_id
        || next.turn_task_id != previous.turn_task_id
        || next.parent_agent_run_id != previous.parent_agent_run_id
        || (next.prompt_bundle_snapshot_id != previous.prompt_bundle_snapshot_id
            && (previous.state != AgentRunState::PreparingContext
                || next.state != AgentRunState::PreparingContext))
        || next.created_at != previous.created_at
        || next.updated_at < previous.updated_at
        || next.next_model_ordinal < previous.next_model_ordinal
        || next.next_model_ordinal > previous.next_model_ordinal.saturating_add(1)
        || matches!(previous.state, AgentRunState::Completed | AgentRunState::Cancelled)
        || next.revision.value()
            != previous.revision.value().checked_add(1).ok_or(Error::Conflict)?
    {
        return Err(Error::Conflict);
    }
    record::changed(tx.execute(
        "UPDATE agent_runs SET state=?2,revision=?3,payload=?4 WHERE id=?1 AND revision=?5",
        params![
            next.agent_run_id.to_string(),
            run_state(&next.state),
            record::sql_u64(next.revision.value())?,
            record::encode(next)?,
            record::sql_u64(previous.revision.value())?
        ],
    )?)
}
pub fn step(db: &Connection, id: ModelStepId) -> StoreResult<ModelStep> {
    let (run, ordinal, state, raw): (String, u32, String, String) = db.query_row(
        "SELECT agent_run_id,ordinal,state,payload FROM model_steps WHERE id=?1",
        [id.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let step: ModelStep = record::decode(&raw)?;
    if step.model_step_id != id
        || step.agent_run_id.to_string() != run
        || step.ordinal != ordinal
        || step_state(&step.state) != state
    {
        return Err(Error::Conflict);
    }
    Ok(step)
}
pub fn insert_step(tx: &Transaction<'_>, step: &ModelStep) -> StoreResult<()> {
    let run = load_run(tx, step.agent_run_id)?;
    if step.state != ModelStepState::Preparing
        || step.finished_at.is_some()
        || step.ordinal != run.next_model_ordinal
        || run.state != AgentRunState::PreparingContext
    {
        return Err(Error::Conflict);
    }
    tx.execute(
        "INSERT INTO model_steps VALUES(?1,?2,?3,NULL,'preparing',?4)",
        params![
            step.model_step_id.to_string(),
            step.agent_run_id.to_string(),
            step.ordinal,
            record::encode(step)?
        ],
    )?;
    Ok(())
}
pub fn save_step(tx: &Transaction<'_>, previous: &ModelStep, next: &ModelStep) -> StoreResult<()> {
    if step(tx, previous.model_step_id)? != *previous
        || next.model_step_id != previous.model_step_id
        || next.agent_run_id != previous.agent_run_id
        || next.ordinal != previous.ordinal
        || next.continuation != previous.continuation
        || !matches!(
            (&previous.state, &next.state),
            (
                ModelStepState::Preparing,
                ModelStepState::Running { .. }
                    | ModelStepState::Failed { .. }
                    | ModelStepState::Cancelled
            ) | (
                ModelStepState::Running { .. },
                ModelStepState::Completed { .. }
                    | ModelStepState::Failed { .. }
                    | ModelStepState::Cancelled
            )
        )
    {
        return Err(Error::Conflict);
    }
    let snapshot = match &next.state {
        ModelStepState::Running { execution_snapshot_id } => {
            let snapshot =
                crate::model::snapshot(tx, *execution_snapshot_id)?.ok_or(Error::Conflict)?;
            let run = load_run(tx, next.agent_run_id)?;
            if snapshot.model_step_id != next.model_step_id
                || snapshot.prompt_bundle_snapshot_id != run.prompt_bundle_snapshot_id
            {
                return Err(Error::Conflict);
            }
            Some(execution_snapshot_id.to_string())
        }
        _ => None,
    };
    record::changed(tx.execute("UPDATE model_steps SET state=?2,snapshot_id=COALESCE(?3,snapshot_id),payload=?4 WHERE id=?1 AND state=?5",
        params![next.model_step_id.to_string(), step_state(&next.state), snapshot, record::encode(next)?, step_state(&previous.state)])?)?;
    // A step that will not receive another delta still needs its derived byte
    // index. Completion repairs again when the response is sealed; failure and
    // cancellation have no later append to hang the repair on.
    if matches!(next.state, ModelStepState::Failed { .. } | ModelStepState::Cancelled) {
        super::stream::repair_byte_index(
            tx,
            next.model_step_id,
            next.finished_at.unwrap_or(next.created_at),
        )?;
    }
    Ok(())
}
pub(super) fn turn_state(state: &TurnTaskState) -> &'static str {
    match state {
        TurnTaskState::Pending => "pending",
        TurnTaskState::Running => "running",
        TurnTaskState::AwaitingApproval => "awaiting_approval",
        TurnTaskState::AwaitingQuery { .. } => "awaiting_query",
        TurnTaskState::Completed { .. } => "completed",
        TurnTaskState::Failed { .. } => "failed",
        TurnTaskState::Cancelled => "cancelled",
        TurnTaskState::RecoveryRequired { .. } => "recovery_required",
    }
}
fn run_state(state: &AgentRunState) -> &'static str {
    match state {
        AgentRunState::PreparingContext => "preparing_context",
        AgentRunState::AwaitingContextRemediation { .. } => "awaiting_context_remediation",
        AgentRunState::ModelRunning { .. } => "model_running",
        AgentRunState::ToolsRunning { .. } => "tools_running",
        AgentRunState::AwaitingApproval { .. } => "awaiting_approval",
        AgentRunState::AwaitingQuery { .. } => "awaiting_query",
        AgentRunState::Completed => "completed",
        AgentRunState::Cancelled => "cancelled",
        AgentRunState::RecoveryRequired { .. } => "recovery_required",
    }
}
fn step_state(state: &ModelStepState) -> &'static str {
    match state {
        ModelStepState::Preparing => "preparing",
        ModelStepState::Running { .. } => "running",
        ModelStepState::Completed { .. } => "completed",
        ModelStepState::Failed { .. } => "failed",
        ModelStepState::Cancelled => "cancelled",
    }
}
