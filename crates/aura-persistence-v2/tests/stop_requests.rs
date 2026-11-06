//! A stop, a failed step and a restart close the run's unfinished model requests: those its steps
//! made and those its compactions made, whose only record of the run is the compaction's own.
//! Another run's requests stay as they are.
#![allow(clippy::unwrap_used)]
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;

use aura_agent_domain::*;
use aura_context_domain::*;
use aura_domain_common::*;
use aura_model_domain::*;
use aura_persistence_core::StoreResult;
use aura_persistence_v2::{
    V2Store,
    agent::{history, lifecycle, recovery, turns},
    context::compaction,
    model,
};

/// A request of `snapshot` that is queued and has not been answered.
fn queued(snapshot: &ModelExecutionSnapshot) -> TransportAttemptRecord {
    let mut attempt =
        aura_contract_schema::canonical_fixture().unwrap().models.transport_attempt.unwrap();
    attempt.id = TransportAttemptId::new();
    attempt.attempt_number = 1;
    attempt.attempt_kind = TransportAttemptKind::Initial;
    attempt.provider_request_id = None;
    attempt.endpoint_id = snapshot.endpoint_id;
    attempt.endpoint_fingerprint = snapshot.endpoint_fingerprint.clone();
    attempt.request_evidence.operation_context = TransportOperationContext::AgentModelAttempt {
        model_step_id: snapshot.model_step_id,
        model_execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
    };
    attempt.state = TransportAttemptState::QueuedForEndpoint;
    attempt.first_byte_at = None;
    attempt.first_model_event_at = None;
    attempt.finished_at = None;
    attempt.raw_stream_artifact_id = None;
    attempt.failure_id = None;
    attempt
}

/// The fixture's run, its step answered, compacting its context: the compaction's request is
/// queued.
fn compacting(store: &mut V2Store, f: &support::Fixture) -> TransportAttemptId {
    store
        .database
        .unit_of_work(|tx| -> StoreResult<_> {
            let mut output = f.template.clone();
            output.model_step_id = f.step.model_step_id;
            output.tool_calls.clear();
            history::record_output(tx, &output, None, f.at)?;
            let old = turns::load_run(tx, f.turn.agent_run_id)?;
            let mut next = old.clone();
            next.state = AgentRunState::PreparingContext;
            next.revision = Revision::new(old.revision.value() + 1);
            next.next_model_ordinal = f.step.ordinal + 1;
            turns::save_run(tx, &old, &next)?;
            let step = lifecycle::prepare_step(
                tx,
                f.turn.agent_run_id,
                AgentContinuation::CompletionReminder {
                    previous_model_step_id: f.step.model_step_id,
                },
                f.at,
            )?;
            let anchors = vec![];
            let sources = b"synthetic sealed sources";
            let value = compaction::Compaction {
                id: ContextProjectionSnapshotId::new(),
                session_id: f.turn.session_id,
                agent_run_id: f.turn.agent_run_id,
                source_step_id: step.model_step_id,
                sequence: 1,
                revision: Revision::INITIAL,
                state: "running".into(),
                source_span: CompactionSourceSpan {
                    start_sequence: SessionSequence::new(1),
                    end_sequence: SessionSequence::new(1),
                    source_refs: vec![],
                },
                anchors: CompactionAnchorManifest {
                    manifest_hash: canonical_json_v1_sha256(&anchors).unwrap(),
                    anchors,
                },
                source_artifact_id: ArtifactId::new(),
                source_hash: ContentHash::sha256(sources),
                excluded_event_ids: vec![],
                retained_recent_tokens: 0,
                retention_exceptions: vec![],
                attempts: vec![],
                result: None,
                result_artifact_id: None,
                created_at: f.at,
            };
            compaction::begin(tx, &value, sources)?;
            let mut snapshot = model::step_snapshot(tx, f.step.model_step_id)?;
            snapshot.id = ModelExecutionSnapshotId::new();
            snapshot.model_step_id = ModelStepId::new();
            snapshot.context_snapshot_id = ContextProjectionSnapshotId::new();
            compaction::start_attempt(
                tx,
                value.id,
                &snapshot,
                CompactionModelAttemptRecord {
                    source_context_snapshot_id: value.id,
                    compaction_sequence: value.sequence,
                    attempt_ordinal: 1,
                    attempt_context_snapshot_id: snapshot.context_snapshot_id,
                    model_step_id: snapshot.model_step_id,
                    model_execution_snapshot_id: snapshot.id,
                    transport_attempt_id: None,
                    state: CompactionModelAttemptState::Running,
                    error_code: None,
                    started_at: f.at,
                    terminal_at: None,
                },
            )?;
            let request = queued(&snapshot);
            model::agent_transport::record_attempt(tx, &snapshot, &request)?;
            Ok(request.id)
        })
        .unwrap()
}

/// The fixture's step, its request queued.
fn requesting(store: &mut V2Store, f: &support::Fixture) -> TransportAttemptId {
    store
        .database
        .unit_of_work(|tx| -> StoreResult<_> {
            let snapshot = model::step_snapshot(tx, f.step.model_step_id)?;
            let request = queued(&snapshot);
            model::agent_transport::record_attempt(tx, &snapshot, &request)?;
            Ok(request.id)
        })
        .unwrap()
}

/// Stops `stopped`; its request is then closed as interrupted and the other run's is untouched.
fn stop_and_check(
    store: &mut V2Store,
    stopped: &support::Fixture,
    own: TransportAttemptId,
    others: TransportAttemptId,
) {
    store
        .database
        .unit_of_work(|tx| {
            recovery::interrupt(
                tx,
                stopped.turn.agent_run_id,
                "agent.user_stopped",
                true,
                stopped.at,
            )
        })
        .unwrap();
    let db = store.database.connection();
    let closed = model::agent_transport::attempt(db, own).unwrap();
    assert_eq!(closed.state, TransportAttemptState::Failed);
    assert!(closed.failure_id.is_some());
    assert_eq!(
        model::agent_transport::attempt(db, others).unwrap().state,
        TransportAttemptState::QueuedForEndpoint
    );
}

#[test]
fn a_stop_closes_its_steps_request_and_leaves_another_runs_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&dir.path().join("v2")).unwrap();
    let stopped = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let other = support::Fixture::new_in_workspace(&mut store, AgentTaskMode::Normal, "C:/other");
    let own = requesting(&mut store, &stopped);
    let others = requesting(&mut store, &other);
    stop_and_check(&mut store, &stopped, own, others);
}

#[test]
fn a_stop_closes_its_compactions_request_and_leaves_another_runs_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&dir.path().join("v2")).unwrap();
    let stopped = support::Fixture::new(&mut store, AgentTaskMode::Plan);
    let other = support::Fixture::new_in_workspace(&mut store, AgentTaskMode::Plan, "C:/other");
    let own = compacting(&mut store, &stopped);
    let others = compacting(&mut store, &other);
    stop_and_check(&mut store, &stopped, own, others);
}
