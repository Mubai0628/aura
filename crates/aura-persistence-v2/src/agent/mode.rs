//! Session Plan/Goal cycle. New turns inherit the mode; they do not resume a
//! stopped lineage identity.
use super::{facts, session, turns, work};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, Transaction};

const KIND: &str = "agent.mode_episode";

fn owner(session: SessionId) -> String {
    format!("agent-mode:{session}")
}

pub fn load(db: &Connection, session: SessionId) -> StoreResult<Option<SessionModeEpisode>> {
    let Some(value) = facts::latest::<SessionModeEpisode>(db, &owner(session), KIND)? else {
        return Ok(None);
    };
    if value.session_id != session {
        return Err(Error::Conflict);
    }
    Ok(Some(value))
}

pub fn view(db: &Connection, session: SessionId) -> StoreResult<AgentModeEpisodeView> {
    Ok(load(db, session)?.as_ref().map(AgentModeEpisodeView::from).unwrap_or(
        AgentModeEpisodeView {
            session_id: session,
            revision: Revision::INITIAL,
            mode: AgentTaskMode::Normal,
            paused: false,
            goal_budget: None,
            inherited_goal: None,
            disposition_required: false,
            end_reason: None,
        },
    ))
}

pub fn save(
    tx: &Transaction<'_>,
    previous: Option<&SessionModeEpisode>,
    next: &SessionModeEpisode,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let loaded = load(tx, next.session_id)?;
    if loaded.as_ref() != previous
        || (next.paused && next.mode != AgentTaskMode::Goal)
        || (next.mode != AgentTaskMode::Goal && next.goal_budget.is_some())
        || next.revision.value()
            != previous
                .map(|p| p.revision.value())
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(Error::Conflict)?
    {
        return Err(Error::Conflict);
    }
    if let Some(previous) = previous {
        if previous.session_id != next.session_id {
            return Err(Error::Conflict);
        }
        // An ended episode keeps its created_at. A later episode is a new identity.
        if previous.ended_reason.is_some() {
            if previous.created_at == next.created_at || next.ended_reason.is_some() {
                return Err(Error::Conflict);
            }
        } else if previous.created_at != next.created_at {
            return Err(Error::Conflict);
        }
    }
    facts::append(tx, EventId::new(), &owner(next.session_id), KIND, next, at)
}

pub fn bind_start(
    tx: &Transaction<'_>,
    session: SessionId,
    requested: AgentTaskMode,
    requested_budget: Option<GoalBudget>,
    turn: TurnTaskId,
    at: UtcTimestamp,
) -> StoreResult<(AgentTaskMode, Option<GoalBudget>, Option<GoalState>)> {
    let previous = load(tx, session)?;
    if let Some(episode) = &previous
        && episode.is_active_special()
    {
        if needs_disposition(tx, episode)? {
            if requested != AgentTaskMode::Normal {
                end(tx, session, ModeEpisodeEndReason::Skipped, at)?;
                return open_episode(tx, session, requested, requested_budget, turn, at);
            }
            let mut next = episode.clone();
            next.revision =
                Revision::new(episode.revision.value().checked_add(1).ok_or(Error::Conflict)?);
            next.disposition_required = true;
            next.started_turn_task_id = Some(turn);
            next.updated_at = at;
            save(tx, previous.as_ref(), &next, at)?;
            return Ok((AgentTaskMode::Normal, None, None));
        }
        if requested != AgentTaskMode::Normal && requested != episode.mode {
            return Err(Error::Invalid("agent.mode_switch_not_allowed"));
        }
        let mut next = episode.clone();
        next.revision =
            Revision::new(episode.revision.value().checked_add(1).ok_or(Error::Conflict)?);
        next.disposition_required = false;
        next.started_turn_task_id = Some(turn);
        next.updated_at = at;
        let mode = next.mode;
        let budget = next.goal_budget.clone();
        let inherited = next.inherited_goal.clone();
        save(tx, previous.as_ref(), &next, at)?;
        return Ok((mode, budget, inherited));
    }
    if requested == AgentTaskMode::Normal {
        return Ok((AgentTaskMode::Normal, None, None));
    }
    open_episode(tx, session, requested, requested_budget, turn, at)
}

fn needs_disposition(tx: &Transaction<'_>, episode: &SessionModeEpisode) -> StoreResult<bool> {
    if episode.paused || episode.disposition_required {
        return Ok(true);
    }
    let Some(turn_id) = episode.started_turn_task_id else {
        return Ok(false);
    };
    Ok(matches!(
        turns::load(tx, turn_id)?.state,
        TurnTaskState::Cancelled
            | TurnTaskState::Failed { .. }
            | TurnTaskState::RecoveryRequired { .. }
    ))
}

fn open_episode(
    tx: &Transaction<'_>,
    session: SessionId,
    requested: AgentTaskMode,
    requested_budget: Option<GoalBudget>,
    turn: TurnTaskId,
    at: UtcTimestamp,
) -> StoreResult<(AgentTaskMode, Option<GoalBudget>, Option<GoalState>)> {
    let previous = load(tx, session)?;
    let revision = previous
        .as_ref()
        .map(|episode| episode.revision.value())
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(Error::Conflict)?;
    let episode = SessionModeEpisode {
        session_id: session,
        revision: Revision::new(revision),
        mode: requested,
        paused: false,
        goal_budget: if requested == AgentTaskMode::Goal { requested_budget } else { None },
        inherited_goal: None,
        started_turn_task_id: Some(turn),
        ended_reason: None,
        created_at: at,
        updated_at: at,
        disposition_required: false,
    };
    save(tx, previous.as_ref(), &episode, at)?;
    Ok((requested, episode.goal_budget.clone(), None))
}

pub fn snapshot_goal(tx: &Transaction<'_>, turn: &TurnTask, at: UtcTimestamp) -> StoreResult<()> {
    if turn.mode != AgentTaskMode::Goal {
        return Ok(());
    }
    let Some(previous) = load(tx, turn.session_id)? else {
        return Ok(());
    };
    if !previous.is_active_special() || previous.mode != AgentTaskMode::Goal {
        return Ok(());
    }
    let goal = work::goal(tx, turn.turn_task_id)?;
    let mut next = previous.clone();
    next.revision = Revision::new(previous.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    next.inherited_goal = Some(goal);
    next.updated_at = at;
    save(tx, Some(&previous), &next, at)
}

pub fn end(
    tx: &Transaction<'_>,
    session: SessionId,
    reason: ModeEpisodeEndReason,
    at: UtcTimestamp,
) -> StoreResult<SessionModeEpisode> {
    let previous = load(tx, session)?.ok_or(Error::Conflict)?;
    if previous.ended_reason.is_some() {
        return Err(Error::Invalid("task_lineage.disposition_not_allowed"));
    }
    let mut next = previous.clone();
    next.revision = Revision::new(previous.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    next.paused = false;
    next.disposition_required = false;
    next.ended_reason = Some(reason);
    next.updated_at = at;
    save(tx, Some(&previous), &next, at)?;
    Ok(next)
}

pub fn remembered(db: &Connection, command: CommandInvocationId) -> StoreResult<bool> {
    Ok(facts::latest::<(AgentModeControlRequest, AgentModeEpisodeView)>(
        db,
        &command.to_string(),
        "agent.mode_command",
    )?
    .is_some())
}

pub fn control(
    tx: &Transaction<'_>,
    command: CommandInvocationId,
    request: &AgentModeControlRequest,
    at: UtcTimestamp,
) -> StoreResult<AgentModeEpisodeView> {
    if let Some((previous, view)) = facts::latest::<(AgentModeControlRequest, AgentModeEpisodeView)>(
        tx,
        &command.to_string(),
        "agent.mode_command",
    )? {
        return if previous == *request { Ok(view) } else { Err(Error::Conflict) };
    }
    let previous = load(tx, request.session_id)?
        .ok_or(Error::Invalid("task_lineage.disposition_not_allowed"))?;
    if !previous.is_active_special() {
        return Err(Error::Invalid("task_lineage.disposition_not_allowed"));
    }
    if previous.revision != request.expected_revision {
        return Err(Error::Invalid("task_lineage.revision_stale"));
    }
    match &request.action {
        AgentModeControlAction::Terminate => {
            interrupt_active(tx, request.session_id, "agent.mode_terminated", at)?;
            end(tx, request.session_id, ModeEpisodeEndReason::UserTerminated, at)?;
        }
        AgentModeControlAction::PauseGoal => {
            if previous.mode != AgentTaskMode::Goal {
                return Err(Error::Invalid("task_lineage.disposition_not_allowed"));
            }
            interrupt_active(tx, request.session_id, "agent.mode_paused", at)?;
            let previous = load(tx, request.session_id)?.ok_or(Error::Conflict)?;
            let mut next = previous.clone();
            next.revision =
                Revision::new(previous.revision.value().checked_add(1).ok_or(Error::Conflict)?);
            next.paused = true;
            next.updated_at = at;
            save(tx, Some(&previous), &next, at)?;
        }
        AgentModeControlAction::EditGoalBudget { goal_budget } => {
            if previous.mode != AgentTaskMode::Goal {
                return Err(Error::Invalid("task_lineage.disposition_not_allowed"));
            }
            let goal_budget = goal_budget.clone();
            let used_tokens =
                previous.inherited_goal.as_ref().map(|goal| goal.consumed_tokens).unwrap_or(0);
            let used_turns =
                previous.inherited_goal.as_ref().map(|goal| goal.consumed_turns).unwrap_or(0);
            if goal_budget.max_tokens.is_some_and(|max| max < used_tokens)
                || goal_budget.max_turns.is_some_and(|max| max < used_turns)
            {
                return Err(Error::Invalid("agent.goal_budget_below_usage"));
            }
            let mut next = previous.clone();
            next.revision =
                Revision::new(previous.revision.value().checked_add(1).ok_or(Error::Conflict)?);
            next.goal_budget = Some(goal_budget.clone());
            next.updated_at = at;
            if let Some(turn_id) = session::load(tx, request.session_id)?.active_turn {
                let turn = turns::load(tx, turn_id)?;
                let run = turns::load_run(tx, turn.agent_run_id)?;
                let live = turn.mode == AgentTaskMode::Goal
                    && !turn.state.terminal()
                    && !matches!(
                        run.state,
                        AgentRunState::ModelRunning { .. } | AgentRunState::ToolsRunning { .. }
                    );
                if live {
                    let current = work::goal(tx, turn.turn_task_id)?;
                    if goal_budget.max_tokens.is_some_and(|max| max < current.consumed_tokens)
                        || goal_budget.max_turns.is_some_and(|max| max < current.consumed_turns)
                    {
                        return Err(Error::Invalid("agent.goal_budget_below_usage"));
                    }
                    work::update_goal_budget(tx, turn.turn_task_id, goal_budget.clone(), at)?;
                }
            }
            save(tx, Some(&previous), &next, at)?;
        }
    }
    let view = self::view(tx, request.session_id)?;
    facts::append(
        tx,
        EventId::new(),
        &command.to_string(),
        "agent.mode_command",
        &(request, &view),
        at,
    )?;
    Ok(view)
}

pub fn decide(
    tx: &Transaction<'_>,
    turn_id: TurnTaskId,
    input: &ModeDispositionInput,
    at: UtcTimestamp,
) -> StoreResult<AgentModeEpisodeView> {
    let reason = input.reason.trim();
    if reason.is_empty() || reason.chars().count() > 500 {
        return Err(Error::Invalid("agent.mode_disposition_reason_required"));
    }
    let turn = turns::load(tx, turn_id)?;
    if turn.state.terminal() {
        return Err(Error::Conflict);
    }
    let previous =
        load(tx, turn.session_id)?.ok_or(Error::Invalid("task_lineage.disposition_not_allowed"))?;
    if !previous.disposition_required
        || !previous.is_active_special()
        || previous.started_turn_task_id != Some(turn_id)
    {
        return Err(Error::Invalid("task_lineage.disposition_not_allowed"));
    }
    let judgment = ModeJudgmentRecord {
        episode_created_at: previous.created_at,
        decision: input.decision,
        excluded_model_step_ids: judgment_steps(tx, &turn)?,
    };
    match input.decision {
        ModeDispositionDecision::Continue => {
            turns::adopt_mode(tx, &turn, previous.mode, at)?;
            let mut next = previous.clone();
            next.disposition_required = false;
            next.paused = false;
            next.revision =
                Revision::new(previous.revision.value().checked_add(1).ok_or(Error::Conflict)?);
            next.updated_at = at;
            save(tx, Some(&previous), &next, at)?;
            if previous.mode == AgentTaskMode::Goal {
                super::work::initialize_goal(
                    tx,
                    &GoalState {
                        turn_task_id: turn_id,
                        revision: Revision::INITIAL,
                        budget: previous.goal_budget.clone().unwrap_or_default(),
                        consumed_tokens: 0,
                        consumed_turns: 0,
                        latest_check_model_step: None,
                        completed: false,
                    },
                    at,
                )?;
                if let Some(inherited) = &previous.inherited_goal {
                    facts::append(
                        tx,
                        EventId::new(),
                        &turn_id.to_string(),
                        "agent.goal_inherited_usage",
                        inherited,
                        at,
                    )?;
                }
            }
        }
        ModeDispositionDecision::Skip => {
            end(tx, turn.session_id, ModeEpisodeEndReason::Skipped, at)?;
        }
    }
    facts::append(tx, EventId::new(), &turn_id.to_string(), "agent.mode_judgment", &judgment, at)?;
    self::view(tx, turn.session_id)
}

fn judgment_steps(tx: &Transaction<'_>, turn: &TurnTask) -> StoreResult<Vec<ModelStepId>> {
    let mut excluded = super::model_context::steps(tx, turn.agent_run_id)?
        .into_iter()
        .map(|step| step.model_step_id)
        .collect::<Vec<_>>();
    if let AgentRunState::ModelRunning { model_step_id } =
        turns::load_run(tx, turn.agent_run_id)?.state
        && !excluded.contains(&model_step_id)
    {
        excluded.push(model_step_id);
    }
    Ok(excluded)
}

fn interrupt_active(
    tx: &Transaction<'_>,
    session: SessionId,
    reason: &str,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let Some(turn_id) = session::load(tx, session)?.active_turn else {
        return Ok(());
    };
    let turn = turns::load(tx, turn_id)?;
    if turn.state.terminal() {
        return Ok(());
    }
    super::recovery::interrupt(tx, turn.agent_run_id, reason, true, at)
}
