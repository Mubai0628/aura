//! Authenticated user submission and model progression share the Agent Store.
//! Continuations only reference prior durable facts; they cannot add user events.
use super::{lineage, session, turns};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::Transaction;

pub fn start(
    tx: &Transaction<'_>,
    user: &OriginalUserMessage,
    mode: AgentTaskMode,
    goal_budget: Option<GoalBudget>,
    bundle: PromptBundleSnapshotId,
) -> StoreResult<TurnTask> {
    start_for(tx, user, mode, goal_budget, bundle, AgentTaskPurpose::Conversation)
}

/// A `/init` or idle `/compact` turn is Normal and never binds to the session's mode episode,
/// so an active Plan or Goal continues with the next ordinary turn exactly as it was left.
pub fn start_for(
    tx: &Transaction<'_>,
    user: &OriginalUserMessage,
    mode: AgentTaskMode,
    goal_budget: Option<GoalBudget>,
    bundle: PromptBundleSnapshotId,
    purpose: AgentTaskPurpose,
) -> StoreResult<TurnTask> {
    if purpose.is_command() && (mode != AgentTaskMode::Normal || goal_budget.is_some()) {
        return Err(Error::Conflict);
    }
    if user.origin != UserMessageOrigin::UserInput {
        return Err(Error::Conflict);
    }
    let scope = crate::workspace::session(tx, user.session_id)?.ok_or(Error::Conflict)?;
    let previous = session::load(tx, user.session_id)?;
    if previous.active_turn.is_some() || !previous.pending_user_event_ids.is_empty() {
        return Err(Error::Conflict);
    }
    let bundle_owned: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM prompt_bundles WHERE id=?1 AND session_id=?2)",
        [bundle.to_string(), user.session_id.to_string()],
        |r| r.get(0),
    )?;
    if !bundle_owned {
        return Err(Error::Conflict);
    }
    let bundle_raw: String = tx.query_row(
        "SELECT payload FROM prompt_bundles WHERE id=?1",
        [bundle.to_string()],
        |r| r.get(0),
    )?;
    let frozen: aura_context_domain::PromptBundleSnapshot = serde_json::from_str(&bundle_raw)?;
    frozen.validate_contract_shape().map_err(|_| Error::Conflict)?;
    let (mode, goal_budget, inherited) = if purpose.is_command() {
        (AgentTaskMode::Normal, None, None)
    } else {
        super::mode::bind_start(
            tx,
            user.session_id,
            mode,
            goal_budget,
            frozen.turn_task_id,
            user.at,
        )?
    };
    if mode != AgentTaskMode::Goal && goal_budget.is_some() {
        return Err(Error::Conflict);
    }
    for id in &previous.open_lineages {
        let old = lineage::load(tx, *id)?;
        if old.state == TaskLineageState::Active {
            let mut next = old.clone();
            next.state = TaskLineageState::InterruptedResumable;
            next.completion_gate = CompletionGate::Unreviewed;
            next.advance_revision(old.revision, user.at).map_err(|_| Error::Conflict)?;
            lineage::save(tx, &old, &next)?;
        }
    }
    session::add_user_message(tx, user)?;
    let turn = TurnTask {
        turn_task_id: frozen.turn_task_id,
        session_id: user.session_id,
        workspace_id: scope.workspace_id,
        root_user_event_id: user.event_id,
        lineage_id: TaskLineageId::new(),
        lineage_resolution_required: false,
        agent_run_id: AgentRunId::new(),
        mode,
        state: TurnTaskState::Pending,
        revision: Revision::INITIAL,
        created_at: user.at,
        updated_at: user.at,
    };
    let run = AgentRun {
        agent_run_id: turn.agent_run_id,
        turn_task_id: turn.turn_task_id,
        parent_agent_run_id: None,
        state: AgentRunState::PreparingContext,
        revision: Revision::INITIAL,
        next_model_ordinal: 1,
        prompt_bundle_snapshot_id: bundle,
        created_at: user.at,
        updated_at: user.at,
    };
    turns::insert(tx, &turn, &run)?;
    let mut next = previous.clone();
    next.active_turn = Some(turn.turn_task_id);
    next.active_lineage = None;
    lineage::insert(
        tx,
        &TaskLineage::from_root(turn.lineage_id, user, turn.turn_task_id)
            .map_err(|_| Error::Conflict)?,
    )?;
    next.active_lineage = Some(turn.lineage_id);
    next.open_lineages.push(turn.lineage_id);
    next.revision = Revision::new(previous.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    session::save(tx, &previous, &next, user.at)?;
    if mode == AgentTaskMode::Goal {
        super::work::initialize_goal(
            tx,
            &GoalState {
                turn_task_id: turn.turn_task_id,
                revision: Revision::INITIAL,
                budget: goal_budget.unwrap_or_default(),
                consumed_tokens: 0,
                consumed_turns: 0,
                latest_check_model_step: None,
                completed: false,
            },
            user.at,
        )?;
        if let Some(inherited) = inherited {
            super::facts::append(
                tx,
                EventId::new(),
                &turn.turn_task_id.to_string(),
                "agent.goal_inherited_usage",
                &inherited,
                user.at,
            )?;
        }
    }
    Ok(turn)
}
pub fn prepare_step(
    tx: &Transaction<'_>,
    run_id: AgentRunId,
    continuation: AgentContinuation,
    at: UtcTimestamp,
) -> StoreResult<ModelStep> {
    let run = turns::load_run(tx, run_id)?;
    let mut turn = turns::load(tx, run.turn_task_id)?;
    if turn.state.terminal() || run.state != AgentRunState::PreparingContext {
        return Err(Error::Conflict);
    }
    if turn.lineage_resolution_required {
        ensure_current_lineage(tx, &turn, at)?;
        turn = turns::load(tx, run.turn_task_id)?;
    }
    sync_goal_budget(tx, &turn, at)?;
    // A step asked again after a passing provider failure rechecks its continuation too.
    let rechecking = crate::context::remediation::recheck_source(tx, run_id)?
        .is_some_and(|s| s.continuation == continuation)
        || crate::model::transient_retry::source(tx, run_id)?
            .is_some_and(|s| s.continuation == continuation);
    match &continuation {
        AgentContinuation::Initial { user_event_id } => {
            if (!rechecking && run.next_model_ordinal != 1)
                || *user_event_id != turn.root_user_event_id
            {
                return Err(Error::Conflict);
            }
        }
        AgentContinuation::ToolBatch { tool_batch_id, history_group_hash } => {
            let histories = crate::context::history(tx, turn.session_id)?;
            if !histories.iter().any(|h| {
                h.agent_run_id == run_id
                    && h.tool_batch_id == *tool_batch_id
                    && h.group_ref().is_ok_and(|r| r.group_hash == *history_group_hash)
            }) {
                return Err(Error::Conflict);
            }
        }
        AgentContinuation::CompletionReminder { previous_model_step_id } => {
            let previous = turns::step(tx, *previous_model_step_id)?;
            if previous.agent_run_id != run_id
                || (!rechecking && previous.ordinal.checked_add(1) != Some(run.next_model_ordinal))
                || !matches!(previous.state, ModelStepState::Completed { tool_batch_id: None, .. })
            {
                return Err(Error::Conflict);
            }
        }
        AgentContinuation::QueryAnswered { query_id, user_event_id } => {
            let query = super::work::query(tx, turn.turn_task_id)?.ok_or(Error::Conflict)?;
            if query.query_id != *query_id
                || !matches!(query.state, UserQueryState::Answered { user_event_id: id, .. } if id == *user_event_id)
            {
                return Err(Error::Conflict);
            }
        }
        // Recovery/Compaction need their owning application to commit and verify
        // a durable recovery episode/context snapshot before this entry is used.
        AgentContinuation::Recovered { recovery_episode } => {
            let ready: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM recovery_episodes WHERE agent_run_id=?1 AND ordinal=?2 AND json_extract(payload,'$.resolved_at') IS NOT NULL)", rusqlite::params![run_id.to_string(), recovery_episode], |r| r.get(0))?;
            if !ready {
                return Err(Error::Conflict);
            }
        }
        AgentContinuation::Compacted { context_snapshot_id } => {
            let snapshot =
                crate::context::snapshot(tx, *context_snapshot_id)?.ok_or(Error::Conflict)?;
            if snapshot.session_id != turn.session_id {
                return Err(Error::Conflict);
            }
        }
    }
    let step = ModelStep {
        model_step_id: ModelStepId::new(),
        agent_run_id: run_id,
        ordinal: run.next_model_ordinal,
        continuation,
        state: ModelStepState::Preparing,
        created_at: at,
        finished_at: None,
    };
    turns::insert_step(tx, &step)?;
    if turn.state != TurnTaskState::Running {
        let mut next = turn.clone();
        next.transition(turn.revision, TurnTaskState::Running, at).map_err(|_| Error::Conflict)?;
        turns::save(tx, &turn, &next)?;
    }
    Ok(step)
}

pub fn ensure_current_lineage(
    tx: &Transaction<'_>,
    turn: &TurnTask,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if lineage::load(tx, turn.lineage_id).is_ok() {
        let mut next = turn.clone();
        next.lineage_resolution_required = false;
        next.revision = Revision::new(turn.revision.value().checked_add(1).ok_or(Error::Conflict)?);
        next.updated_at = at;
        turns::save(tx, turn, &next)?;
        let previous = session::load(tx, turn.session_id)?;
        let mut state = previous.clone();
        state.active_lineage = Some(turn.lineage_id);
        if !state.open_lineages.contains(&turn.lineage_id) {
            state.open_lineages.push(turn.lineage_id);
        }
        state.revision =
            Revision::new(previous.revision.value().checked_add(1).ok_or(Error::Conflict)?);
        return session::save(tx, &previous, &state, at);
    }
    let user = session::user_message(tx, turn.session_id, turn.root_user_event_id)?;
    lineage::insert(
        tx,
        &TaskLineage::from_root(turn.lineage_id, &user, turn.turn_task_id)
            .map_err(|_| Error::Conflict)?,
    )?;
    let mut next = turn.clone();
    next.lineage_resolution_required = false;
    next.revision = Revision::new(turn.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    next.updated_at = at;
    turns::save(tx, turn, &next)?;
    let previous = session::load(tx, turn.session_id)?;
    let mut state = previous.clone();
    state.active_lineage = Some(turn.lineage_id);
    if !state.open_lineages.contains(&turn.lineage_id) {
        state.open_lineages.push(turn.lineage_id);
    }
    state.revision =
        Revision::new(previous.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    session::save(tx, &previous, &state, at)
}

fn sync_goal_budget(tx: &Transaction<'_>, turn: &TurnTask, at: UtcTimestamp) -> StoreResult<()> {
    if turn.mode != AgentTaskMode::Goal {
        return Ok(());
    }
    let Some(episode) = super::mode::load(tx, turn.session_id)? else {
        return Ok(());
    };
    let Some(budget) = episode.goal_budget.clone() else {
        return Ok(());
    };
    if !episode.is_active_special() || episode.mode != AgentTaskMode::Goal {
        return Ok(());
    }
    let current = super::work::goal(tx, turn.turn_task_id)?;
    if current.budget == budget {
        return Ok(());
    }
    super::work::update_goal_budget(tx, turn.turn_task_id, budget, at)?;
    Ok(())
}
