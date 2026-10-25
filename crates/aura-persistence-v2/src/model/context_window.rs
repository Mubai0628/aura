//! Provider rejection authorizes a new preparation, never replay of a failed mex.
//! Configuration remains P05-owned; these records only describe actual trials.
mod actions;
use crate::agent::{application, model_context, session, turns};
pub use actions::{recheck_source, resolve, resolved};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_model_domain::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact, event};
use rusqlite::{Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct WindowCorrection {
    pub view: AgentContextWindowCorrection,
    pub failed_snapshot: ModelExecutionSnapshot,
    pub source_profile: ModelProfileConfig,
    pub candidate_windows: Vec<u64>,
    pub journal_command_id: CommandInvocationId,
}
pub fn latest(db: &Connection, run: AgentRunId) -> StoreResult<Option<WindowCorrection>> {
    let raw: Option<String> = db.query_row(
        "SELECT payload FROM events WHERE owner=?1 AND kind='model.context_window' ORDER BY sequence DESC LIMIT 1",
        [run.to_string()], |r| r.get(0),
    ).optional()?;
    raw.map(|v| Ok(serde_json::from_str(&v)?)).transpose()
}
pub fn view(db: &Connection, run: AgentRunId) -> StoreResult<Option<AgentContextWindowCorrection>> {
    Ok(latest(db, run)?.map(|r| r.view))
}
fn append<T: Serialize>(
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
        "model",
    )?;
    Ok(())
}
fn save(tx: &Transaction<'_>, record: &WindowCorrection, at: UtcTimestamp) -> StoreResult<()> {
    append(tx, &record.view.correction.agent_run_id.to_string(), "model.context_window", record, at)
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
fn next_revision(value: Revision) -> StoreResult<Revision> {
    Ok(Revision::new(value.value().checked_add(1).ok_or(Error::Conflict)?))
}
fn provider_error(record: &WindowCorrection) -> ArtifactId {
    match record.view.correction.resolution {
        ContextWindowResolution::P06RebuildRequired { provider_error_ref, .. }
        | ContextWindowResolution::UserCorrectionRequired { provider_error_ref, .. }
        | ContextWindowResolution::AutomaticCandidateSelected { provider_error_ref, .. }
        | ContextWindowResolution::RuntimeCorrectionApplied { provider_error_ref, .. }
        | ContextWindowResolution::AutomaticCandidatesExhausted { provider_error_ref, .. } => {
            provider_error_ref
        }
    }
}
fn manual_resolution(
    record: &WindowCorrection,
    tokens: u64,
    exhausted: bool,
) -> ContextWindowResolution {
    let snapshot = &record.failed_snapshot;
    let input_contract =
        ContextWindowTokenInputContract::new(1, None).expect("constant input contract");
    if exhausted {
        ContextWindowResolution::AutomaticCandidatesExhausted {
            failed_execution_snapshot_id: snapshot.id,
            model_profile: record.source_profile.clone(),
            provider_error_ref: provider_error(record),
            local_estimated_input_tokens: tokens,
            current_effective_window_tokens: snapshot.runtime_effective_window_tokens,
            input_contract,
        }
    } else {
        ContextWindowResolution::UserCorrectionRequired {
            failed_execution_snapshot_id: snapshot.id,
            model_profile: record.source_profile.clone(),
            provider_error_ref: provider_error(record),
            local_estimated_input_tokens: tokens,
            current_effective_window_tokens: snapshot.runtime_effective_window_tokens,
            input_contract,
        }
    }
}
fn select_candidate(record: &mut WindowCorrection) -> bool {
    let Some(candidate) =
        next_context_window_candidate(record.failed_snapshot.runtime_effective_window_tokens)
    else {
        return false;
    };
    record.candidate_windows.push(candidate);
    record.view.correction.resolution = ContextWindowResolution::AutomaticCandidateSelected {
        failed_execution_snapshot_id: record.failed_snapshot.id,
        model_profile_id: record.failed_snapshot.model_profile_id,
        provider_error_ref: provider_error(record),
        candidate_window_tokens: candidate,
        remaining_candidates: CONTEXT_WINDOW_AUTOMATIC_CANDIDATES
            .into_iter()
            .filter(|v| *v < candidate)
            .collect(),
    };
    record.view.state = AgentContextWindowState::Preparing;
    true
}
/// Only a durable, terminal, context-window rejection without partial model
/// output is eligible. Arbitrary failures and Unknown operations cannot enter.
pub fn reject(
    tx: &Transaction<'_>,
    snapshot: &ModelExecutionSnapshot,
    profile: &ModelProfileConfig,
    failure: &PersistedModelFailure,
    summary: ProviderErrorSummaryView,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let step = turns::step(tx, snapshot.model_step_id)?;
    let run = active(tx, step.agent_run_id)?;
    let attempt = super::agent_transport::attempt(
        tx,
        failure.failure.transport_attempt_id.ok_or(Error::Conflict)?,
    )?;
    let durable: String = tx.query_row(
        "SELECT payload FROM events WHERE owner=?1 AND kind='model.failure' AND json_extract(payload,'$.id')=?2",
        [step.model_step_id.to_string(), failure.id.to_string()], |r| r.get(0),
    )?;
    if failure.failure.category != ModelFailureCategory::ContextWindowExceeded
        || failure.failure.partial_output_ref.is_some()
        || serde_json::from_str::<PersistedModelFailure>(&durable)? != *failure
        || attempt.state != TransportAttemptState::Failed
        || attempt.failure_id != Some(failure.id)
        || !matches!(attempt.request_evidence.operation_context, TransportOperationContext::AgentModelAttempt {model_execution_snapshot_id, ..} if model_execution_snapshot_id == snapshot.id)
        || super::snapshot(tx, snapshot.id)?.as_ref() != Some(snapshot)
        || step.state != (ModelStepState::Running { execution_snapshot_id: snapshot.id })
        || run.state != (AgentRunState::ModelRunning { model_step_id: step.model_step_id })
        || profile.id != snapshot.model_profile_id
        || profile.endpoint_id != snapshot.endpoint_id
        || profile.provider_model_name != snapshot.model_name
    {
        return Err(Error::Conflict);
    }
    let error = failure.failure.provider_error_ref.ok_or(Error::Conflict)?;
    artifact::read(tx, error, &step.model_step_id.to_string())?;
    let context =
        crate::context::snapshot(tx, snapshot.context_snapshot_id)?.ok_or(Error::Conflict)?;
    let tokens = context.budget.estimated_input_tokens;
    let previous = trial(tx, step.model_step_id)?;
    let mut record = WindowCorrection {
        view: AgentContextWindowCorrection {
            correction: ContextWindowCorrectionView {
                agent_run_id: run.agent_run_id,
                model_step_id: step.model_step_id,
                failed_execution_snapshot_id: snapshot.id,
                model_profile_id: snapshot.model_profile_id,
                resolution: ContextWindowResolution::P06RebuildRequired {
                    failed_execution_snapshot_id: snapshot.id,
                    model_profile_id: snapshot.model_profile_id,
                    provider_error_ref: error,
                    local_estimated_input_tokens: tokens,
                    current_effective_window_tokens: snapshot.runtime_effective_window_tokens,
                },
                provider_error: Some(summary),
            },
            revision: next_revision(run.revision)?,
            state: AgentContextWindowState::Preparing,
            created_at: at,
            updated_at: at,
        },
        failed_snapshot: snapshot.clone(),
        source_profile: profile.clone(),
        candidate_windows: Vec::new(),
        journal_command_id: CommandInvocationId::new(),
    };
    if let Some(previous) = previous.filter(|r| !r.candidate_windows.is_empty()) {
        record.source_profile = previous.source_profile;
        record.candidate_windows = previous.candidate_windows;
        if !select_candidate(&mut record) {
            record.view.correction.resolution = manual_resolution(&record, tokens, true);
            record.view.state = AgentContextWindowState::Awaiting;
        }
    } else if !context_window_requires_rebuild(tokens, snapshot.runtime_effective_window_tokens) {
        record.view.correction.resolution = manual_resolution(&record, tokens, false);
        record.view.state = AgentContextWindowState::Awaiting;
    }
    let mut failed = step.clone();
    failed.state = ModelStepState::Failed { error_artifact_id: error };
    failed.finished_at = Some(at);
    turns::save_step(tx, &step, &failed)?;
    let mut next = run.clone();
    next.revision = record.view.revision;
    next.next_model_ordinal = run.next_model_ordinal.checked_add(1).ok_or(Error::Conflict)?;
    next.state = if record.view.state == AgentContextWindowState::Preparing {
        AgentRunState::PreparingContext
    } else {
        AgentRunState::AwaitingContextRemediation { model_step_id: step.model_step_id }
    };
    next.updated_at = at;
    turns::save_run(tx, &run, &next)?;
    save(tx, &record, at)
}
pub fn trial(db: &Connection, step: ModelStepId) -> StoreResult<Option<WindowCorrection>> {
    let raw: Option<String> = db.query_row(
        "SELECT payload FROM events WHERE owner=?1 AND kind='model.context_window_trial' LIMIT 1",
        [step.to_string()], |r| r.get(0),
    ).optional()?;
    raw.map(|v| Ok(serde_json::from_str(&v)?)).transpose()
}
pub fn freeze_trial(
    tx: &Transaction<'_>,
    snapshot: &ModelExecutionSnapshot,
    selected: Option<&WindowCorrection>,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let step = turns::step(tx, snapshot.model_step_id)?;
    let Some(mut record) = latest(tx, step.agent_run_id)?
        .filter(|r| r.view.state == AgentContextWindowState::Preparing)
    else {
        return Ok(());
    };
    if selected != Some(&record) {
        record.view.state = AgentContextWindowState::Cleared;
    } else {
        if record.failed_snapshot.model_step_id == step.model_step_id
            || record.failed_snapshot.id == snapshot.id
            || snapshot.model_profile_id != record.source_profile.id
            || record
                .candidate_windows
                .last()
                .is_some_and(|v| *v != snapshot.runtime_effective_window_tokens)
        {
            return Err(Error::Conflict);
        }
        append(tx, &step.model_step_id.to_string(), "model.context_window_trial", &record, at)?;
        record.view.state = AgentContextWindowState::RequestRunning;
    }
    record.view.updated_at = at;
    save(tx, &record, at)
}
pub fn finish(
    tx: &Transaction<'_>,
    run: AgentRunId,
    resolution: Option<ContextWindowResolution>,
    cancelled: bool,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let Some(mut record) = latest(tx, run)?.filter(|r| {
        matches!(
            r.view.state,
            AgentContextWindowState::Awaiting
                | AgentContextWindowState::Preparing
                | AgentContextWindowState::RequestRunning
        )
    }) else {
        return Ok(());
    };
    record.view.state = if cancelled {
        AgentContextWindowState::Cancelled
    } else if resolution.is_some() {
        AgentContextWindowState::Applied
    } else {
        AgentContextWindowState::Cleared
    };
    if let Some(resolution) = resolution {
        record.view.correction.resolution = resolution;
    }
    record.view.updated_at = at;
    save(tx, &record, at)
}
