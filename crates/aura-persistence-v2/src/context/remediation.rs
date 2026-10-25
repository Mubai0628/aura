//! A durable preflight wait and an authenticated recheck. Configuration changes
//! stay with their existing owners; this Store never trims or rewrites sources.
use crate::agent::{application, model_context, session, turns};
use aura_agent_domain::*;
use aura_context_domain::*;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact, event};
use rusqlite::{Connection, OptionalExtension, Transaction};

fn latest(db: &Connection, run: AgentRunId) -> StoreResult<Option<AgentContextRemediation>> {
    let raw: Option<String> = db.query_row(
        "SELECT payload FROM events WHERE owner=?1 AND kind='context.remediation' ORDER BY sequence DESC LIMIT 1",
        [run.to_string()], |r| r.get(0),
    ).optional()?;
    raw.map(|v| Ok(serde_json::from_str(&v)?)).transpose()
}
fn append<T: serde::Serialize>(
    tx: &Transaction<'_>,
    owner: &str,
    kind: &str,
    value: &T,
    at: UtcTimestamp,
) -> StoreResult<()> {
    event::append(
        tx,
        &event::EventDraft {
            id: EventId::new(),
            owner,
            kind,
            version: 1,
            payload: &serde_json::to_value(value)?,
            at,
        },
        "context",
    )?;
    Ok(())
}
fn active(db: &Connection, id: AgentRunId) -> StoreResult<AgentRun> {
    let run = turns::load_run(db, id)?;
    let turn = turns::load(db, run.turn_task_id)?;
    if turn.state.terminal()
        || application::cancellation_requested(db, id)?
        || session::load(db, turn.session_id)?.active_turn != Some(turn.turn_task_id)
    {
        return Err(Error::Conflict);
    }
    Ok(run)
}
pub fn view(db: &Connection, input: &AgentRunRequest) -> StoreResult<AgentContextRemediationView> {
    let run = turns::load_run(db, input.agent_run_id)?;
    if turns::load(db, run.turn_task_id)?.session_id != input.session_id {
        return Err(Error::Conflict);
    }
    Ok(AgentContextRemediationView {
        session_id: input.session_id,
        agent_run_id: input.agent_run_id,
        remediation: latest(db, input.agent_run_id)?,
        provider_correction: crate::model::context_window::view(db, input.agent_run_id)?,
    })
}
pub fn require(
    tx: &Transaction<'_>,
    step_id: ModelStepId,
    plan: ContextRemediationPlan,
    budget: ContextBudgetSnapshot,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let step = turns::step(tx, step_id)?;
    let run = active(tx, step.agent_run_id)?;
    if run.state != AgentRunState::PreparingContext
        || step.state != ModelStepState::Preparing
        || step.ordinal != run.next_model_ordinal
    {
        return Err(Error::Conflict);
    }
    let record = AgentContextRemediation {
        model_step_id: step_id,
        revision: Revision::new(run.revision.value().checked_add(1).ok_or(Error::Conflict)?),
        plan,
        budget,
        state: AgentContextRemediationState::Awaiting,
        created_at: at,
        updated_at: at,
    };
    let error_id = ArtifactId::new();
    artifact::put(
        tx,
        error_id,
        &step_id.to_string(),
        "application/json",
        &serde_json::to_vec(&record)?,
        at,
    )?;
    let mut failed = step.clone();
    failed.state = ModelStepState::Failed { error_artifact_id: error_id };
    failed.finished_at = Some(at);
    turns::save_step(tx, &step, &failed)?;
    let mut waiting = run.clone();
    waiting.revision = record.revision;
    waiting.next_model_ordinal = run.next_model_ordinal.checked_add(1).ok_or(Error::Conflict)?;
    waiting.state = AgentRunState::AwaitingContextRemediation { model_step_id: step_id };
    waiting.updated_at = at;
    turns::save_run(tx, &run, &waiting)?;
    append(tx, &run.agent_run_id.to_string(), "context.remediation", &record, at)
}
pub fn rechecked(
    db: &Connection,
    command: CommandInvocationId,
    input: &AgentContextRecheckRequest,
) -> StoreResult<bool> {
    let raw: Option<String> = db.query_row(
        "SELECT payload FROM events WHERE owner=?1 AND kind='context.remediation_recheck' LIMIT 1",
        [command.to_string()], |r| r.get(0),
    ).optional()?;
    match raw {
        Some(raw) if serde_json::from_str::<AgentContextRecheckRequest>(&raw)? == *input => {
            Ok(true)
        }
        Some(_) => Err(Error::Conflict),
        None => Ok(false),
    }
}
pub fn recheck(
    tx: &Transaction<'_>,
    command: CommandInvocationId,
    input: &AgentContextRecheckRequest,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if rechecked(tx, command, input)? {
        return Ok(());
    }
    let mut record = view(
        tx,
        &AgentRunRequest { session_id: input.session_id, agent_run_id: input.agent_run_id },
    )?
    .remediation
    .ok_or(Error::Conflict)?;
    let run = active(tx, input.agent_run_id)?;
    if record.state != AgentContextRemediationState::Awaiting
        || record.revision != input.expected_revision
        || record.model_step_id != input.source_model_step_id
        || run.revision != record.revision
        || run.state
            != (AgentRunState::AwaitingContextRemediation { model_step_id: record.model_step_id })
    {
        return Err(Error::Conflict);
    }
    let mut preparing = run.clone();
    preparing.revision = Revision::new(run.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    preparing.state = AgentRunState::PreparingContext;
    preparing.updated_at = at;
    turns::save_run(tx, &run, &preparing)?;
    record.state = AgentContextRemediationState::Rechecking;
    record.updated_at = at;
    append(tx, &input.agent_run_id.to_string(), "context.remediation", &record, at)?;
    append(tx, &command.to_string(), "context.remediation_recheck", input, at)
}
/// Reuse a failed preflight's input, or delegate to the separate Model Store
/// authorizer for a durable context-window rejection. Unknown is ineligible.
pub fn recheck_source(db: &Connection, run_id: AgentRunId) -> StoreResult<Option<ModelStep>> {
    let run = turns::load_run(db, run_id)?;
    let Some(record) =
        latest(db, run_id)?.filter(|v| v.state == AgentContextRemediationState::Rechecking)
    else {
        return crate::model::context_window::recheck_source(db, run_id);
    };
    let step = turns::step(db, record.model_step_id)?;
    let has_snapshot: bool = db.query_row(
        "SELECT snapshot_id IS NOT NULL FROM model_steps WHERE id=?1",
        [step.model_step_id.to_string()],
        |r| r.get(0),
    )?;
    if run.state != AgentRunState::PreparingContext
        || step.agent_run_id != run_id
        || has_snapshot
        || !matches!(step.state, ModelStepState::Failed { .. })
        || step.ordinal.checked_add(1) != Some(run.next_model_ordinal)
        || model_context::steps(db, run_id)?.last() != Some(&step)
    {
        return Ok(None);
    }
    Ok(Some(step))
}
pub fn clear(
    tx: &Transaction<'_>,
    run_id: AgentRunId,
    cancelled: bool,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let Some(mut record) = latest(tx, run_id)?.filter(|r| {
        matches!(
            r.state,
            AgentContextRemediationState::Awaiting | AgentContextRemediationState::Rechecking
        )
    }) else {
        return Ok(());
    };
    if !cancelled {
        let run = turns::load_run(tx, run_id)?;
        if record.state != AgentContextRemediationState::Rechecking
            || !matches!(run.state, AgentRunState::ModelRunning { .. })
        {
            return Err(Error::Conflict);
        }
    }
    record.state = if cancelled {
        AgentContextRemediationState::Cancelled
    } else {
        AgentContextRemediationState::Cleared
    };
    record.updated_at = at;
    append(tx, &run_id.to_string(), "context.remediation", &record, at)
}
