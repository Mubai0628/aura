//! Application transitions are committed by the Agent owner, not a Driver.
use super::{facts, turns, work};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RunConfiguration {
    pub request: AgentStartRequest,
    pub tool_settings: aura_tool_contract::ToolSettingsSnapshotV1,
    /// Absent in every configuration written by older versions, which were all conversations.
    #[serde(default, skip_serializing_if = "AgentTaskPurpose::is_conversation")]
    pub purpose: AgentTaskPurpose,
}
pub fn configuration(db: &Connection, run: AgentRunId) -> StoreResult<RunConfiguration> {
    facts::latest(db, &run.to_string(), "agent.run_configuration")?.ok_or(Error::Conflict)
}
/// What the run's turn is for; a run without a recorded configuration is a conversation.
pub fn purpose(db: &Connection, run: AgentRunId) -> StoreResult<AgentTaskPurpose> {
    Ok(facts::latest::<RunConfiguration>(db, &run.to_string(), "agent.run_configuration")?
        .map(|config| config.purpose)
        .unwrap_or_default())
}
/// Freeze settings at the same safe point as the request's tool definitions.
pub fn freeze_step_tool_settings(
    tx: &Transaction<'_>,
    step: ModelStepId,
    settings: &aura_tool_contract::ToolSettingsSnapshotV1,
    at: UtcTimestamp,
) -> StoreResult<()> {
    turns::step(tx, step)?;
    settings.validate().map_err(|_| Error::Conflict)?;
    if let Some(previous) = facts::latest::<aura_tool_contract::ToolSettingsSnapshotV1>(
        tx,
        &step.to_string(),
        "agent.step_tool_settings",
    )? {
        return if previous == *settings { Ok(()) } else { Err(Error::Conflict) };
    }
    facts::append(tx, EventId::new(), &step.to_string(), "agent.step_tool_settings", settings, at)
}

pub fn step_tool_settings(
    db: &Connection,
    step: ModelStepId,
) -> StoreResult<aura_tool_contract::ToolSettingsSnapshotV1> {
    if let Some(settings) = facts::latest(db, &step.to_string(), "agent.step_tool_settings")? {
        return Ok(settings);
    }
    // Old steps predate per-step settings; their original run configuration is
    // durable. Never reinterpret an issued request using current global values.
    Ok(configuration(db, turns::step(db, step)?.agent_run_id)?.tool_settings)
}
pub fn configure(
    tx: &Transaction<'_>,
    run: AgentRunId,
    config: &RunConfiguration,
    command: CommandInvocationId,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let turn = turns::load(tx, turns::load_run(tx, run)?.turn_task_id)?;
    if config.request.session_id != turn.session_id
        || config.request.mode != turn.mode
        || facts::latest::<RunConfiguration>(tx, &run.to_string(), "agent.run_configuration")?
            .is_some()
    {
        return Err(Error::Conflict);
    }
    facts::append(tx, EventId::new(), &run.to_string(), "agent.run_configuration", config, at)?;
    facts::append(
        tx,
        EventId::new(),
        &command.to_string(),
        "agent.submission",
        &(config.request.clone(), run),
        at,
    )
}
pub fn submitted(
    db: &Connection,
    command: CommandInvocationId,
    input: &AgentStartRequest,
) -> StoreResult<Option<AgentRunId>> {
    let previous: Option<(AgentStartRequest, AgentRunId)> =
        facts::latest(db, &command.to_string(), "agent.submission")?;
    previous.map(|(old, id)| if old == *input { Ok(id) } else { Err(Error::Conflict) }).transpose()
}
pub fn cancellation_requested(db: &Connection, run: AgentRunId) -> StoreResult<bool> {
    Ok(facts::latest::<bool>(db, &run.to_string(), "agent.cancel_requested")?.unwrap_or(false))
}
pub fn request_cancel(tx: &Transaction<'_>, run: AgentRunId, at: UtcTimestamp) -> StoreResult<()> {
    let r = turns::load_run(tx, run)?;
    let t = turns::load(tx, r.turn_task_id)?;
    if t.state.terminal() || cancellation_requested(tx, run)? {
        return Ok(());
    }
    facts::append(tx, EventId::new(), &run.to_string(), "agent.cancel_requested", &true, at)
}
pub fn projection(db: &Connection, request: &AgentRunRequest) -> StoreResult<AgentRunProjection> {
    let run = turns::load_run(db, request.agent_run_id)?;
    let turn = turns::load(db, run.turn_task_id)?;
    if turn.session_id != request.session_id {
        return Err(Error::Conflict);
    }
    let pending_query = work::query(db, turn.turn_task_id)?;
    Ok(AgentRunProjection {
        recovery_episodes: super::recovery::episodes(db, run.agent_run_id)?,
        pending_approval_ids: super::approval::pending(db, run.agent_run_id)?,
        pending_query_deadline_at: match &pending_query {
            Some(query) => super::query::deadline(db, query)?,
            None => None,
        },
        pending_query,
        cancellation_requested: cancellation_requested(db, run.agent_run_id)?,
        model_steps: super::model_context::steps(db, run.agent_run_id)?,
        turn,
        run,
    })
}
pub fn after_batch(
    tx: &Transaction<'_>,
    run_id: AgentRunId,
    batch: ToolBatchId,
    group: &aura_context_domain::HistoryAtomicGroupPreimageV1,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let old = turns::load_run(tx, run_id)?;
    let turn = turns::load(tx, old.turn_task_id)?;
    if turn.state.terminal() {
        return Ok(());
    }
    if old.state != (AgentRunState::ToolsRunning { tool_batch_id: batch })
        || group.agent_run_id != run_id
        || group.tool_batch_id != batch
        || !crate::context::history(tx, turn.session_id)?.iter().any(|g| g == group)
    {
        return Err(Error::Conflict);
    }
    let mut next = old.clone();
    next.revision = Revision::new(old.revision.value() + 1);
    next.updated_at = at;
    next.state = if let Some(query) =
        work::query(tx, turn.turn_task_id)?.filter(|q| q.state == UserQueryState::Pending)
    {
        let mut t = turn.clone();
        t.transition(turn.revision, TurnTaskState::AwaitingQuery { query_id: query.query_id }, at)
            .map_err(|_| Error::Conflict)?;
        turns::save(tx, &turn, &t)?;
        AgentRunState::AwaitingQuery { query_id: query.query_id }
    } else {
        AgentRunState::PreparingContext
    };
    turns::save_run(tx, &old, &next)
}
pub use super::recovery::{interrupt, interrupt_with_diagnostic};
