//! A step the provider failed for a passing reason is asked again as a
//! new step on the same continuation, a bounded number of times. Nothing of the failed step was
//! committed - its output is not history and no tool of it ran - so asking again replays nothing,
//! the way a refused context window is asked again with a smaller one.
use crate::agent::{model_context, turns};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_model_domain::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact, event};
use rusqlite::{Connection, OptionalExtension, Transaction};

const RETRIED: &str = "model.transient_retry";

/// Records that the provider failed this running step for a passing reason, and returns its run to
/// preparing a new step, which [`source`] continues where the failed one would have.
pub fn retry(
    tx: &Transaction<'_>,
    snapshot: &ModelExecutionSnapshot,
    failure: &PersistedModelFailure,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let step = turns::step(tx, snapshot.model_step_id)?;
    let run = turns::load_run(tx, step.agent_run_id)?;
    let attempt = super::agent_transport::attempt(
        tx,
        failure.failure.transport_attempt_id.ok_or(Error::Conflict)?,
    )?;
    let durable: String = tx.query_row(
        "SELECT payload FROM events WHERE owner=?1 AND kind='model.failure' AND json_extract(payload,'$.id')=?2",
        [step.model_step_id.to_string(), failure.id.to_string()], |r| r.get(0),
    )?;
    if serde_json::from_str::<PersistedModelFailure>(&durable)? != *failure
        || attempt.state != TransportAttemptState::Failed
        || attempt.failure_id != Some(failure.id)
        || !matches!(attempt.request_evidence.operation_context, TransportOperationContext::AgentModelAttempt { model_execution_snapshot_id, .. } if model_execution_snapshot_id == snapshot.id)
        || super::snapshot(tx, snapshot.id)?.as_ref() != Some(snapshot)
        || step.state != (ModelStepState::Running { execution_snapshot_id: snapshot.id })
        || run.state != (AgentRunState::ModelRunning { model_step_id: step.model_step_id })
    {
        return Err(Error::Conflict);
    }
    let evidence = ArtifactId::new();
    let payload = serde_json::json!({
        "reason": "agent.provider_transient_failure",
        "code": failure.failure.code,
        "retry": "automatic_new_step",
    });
    artifact::put(
        tx,
        evidence,
        &step.model_step_id.to_string(),
        "application/json",
        &serde_json::to_vec(&payload)?,
        at,
    )?;
    let mut failed = step.clone();
    failed.state = ModelStepState::Failed { error_artifact_id: evidence };
    failed.finished_at = Some(at);
    turns::save_step(tx, &step, &failed)?;
    let mut next = run.clone();
    next.revision = Revision::new(run.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    next.next_model_ordinal = run.next_model_ordinal.checked_add(1).ok_or(Error::Conflict)?;
    next.state = AgentRunState::PreparingContext;
    next.updated_at = at;
    turns::save_run(tx, &run, &next)?;
    event::append(
        tx,
        &event::EventDraft {
            id: EventId::new(),
            owner: &step.model_step_id.to_string(),
            kind: RETRIED,
            version: 1,
            payload: &serde_json::json!({ "failure_id": failure.id, "code": failure.failure.code }),
            at,
        },
        "model",
    )?;
    Ok(())
}

/// The failed step a run prepares again after [`retry`], when it is the run's last step.
pub fn source(db: &Connection, run_id: AgentRunId) -> StoreResult<Option<ModelStep>> {
    let run = turns::load_run(db, run_id)?;
    let Some(step) = model_context::steps(db, run_id)?.pop() else {
        return Ok(None);
    };
    if run.state != AgentRunState::PreparingContext
        || !matches!(step.state, ModelStepState::Failed { .. })
        || step.ordinal.checked_add(1) != Some(run.next_model_ordinal)
        || !retried(db, step.model_step_id)?
    {
        return Ok(None);
    }
    Ok(Some(step))
}

/// How many steps in a row before the current one were asked again after a passing failure. The
/// step whose failure is being decided is still marked running and is not counted.
pub fn streak(db: &Connection, run_id: AgentRunId) -> StoreResult<u32> {
    let mut count = 0;
    for step in model_context::steps(db, run_id)?.iter().rev() {
        match step.state {
            ModelStepState::Preparing | ModelStepState::Running { .. } => {}
            ModelStepState::Failed { .. } if retried(db, step.model_step_id)? => count += 1,
            _ => break,
        }
    }
    Ok(count)
}

/// The connections the steps of this streak failed on, the latest last: a step asked again
/// goes to a connection not among them (the provider's backups).
pub fn failed_connections(db: &Connection, run_id: AgentRunId) -> StoreResult<Vec<EndpointId>> {
    let mut failed = Vec::new();
    for step in model_context::steps(db, run_id)?.iter().rev() {
        match step.state {
            ModelStepState::Preparing | ModelStepState::Running { .. } => {}
            ModelStepState::Failed { .. } if retried(db, step.model_step_id)? => {
                let endpoint: Option<String> = db
                    .query_row(
                        "SELECT s.endpoint_id FROM model_steps m                          JOIN model_execution_snapshots s ON s.id = m.snapshot_id WHERE m.id = ?1",
                        [step.model_step_id.to_string()],
                        |row| row.get(0),
                    )
                    .optional()?;
                if let Some(endpoint) = endpoint.and_then(|value| value.parse().ok()) {
                    failed.insert(0, endpoint);
                }
            }
            _ => break,
        }
    }
    Ok(failed)
}

fn retried(db: &Connection, step: ModelStepId) -> StoreResult<bool> {
    Ok(db
        .query_row(
            "SELECT 1 FROM events WHERE owner=?1 AND kind=?2 LIMIT 1",
            [step.to_string(), RETRIED.to_owned()],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}
