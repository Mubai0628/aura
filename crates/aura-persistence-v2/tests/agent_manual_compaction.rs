#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;
use aura_agent_domain::*;
use aura_context_domain::*;
use aura_domain_common::*;
use aura_persistence_v2::{
    V2Store,
    agent::{history, inbox, lineage, recovery, turns},
    context::manual_compaction as manual,
};

#[test]
fn manual_command_is_idempotent_and_waits_without_becoming_task_input() {
    for mode in [AgentTaskMode::Normal, AgentTaskMode::Plan, AgentTaskMode::Goal] {
        let root = tempfile::tempdir().expect("directory");
        let mut store = V2Store::create(&root.path().join("v2")).expect("store");
        let f = support::Fixture::new(&mut store, mode);
        let prior = lineage::load(store.database.connection(), f.turn.lineage_id).expect("lineage");
        let input = AgentCompactionRequest {
            session_id: f.turn.session_id,
            agent_run_id: f.turn.agent_run_id,
            focus: Some("保留原始验收条件\r\n/goal is not a mode command".into()),
        };
        let id = CommandInvocationId::new();
        let result = store
            .database
            .unit_of_work(|tx| manual::request(tx, id, &input, f.at))
            .expect("request");
        let record = result.request.expect("record");
        assert_eq!(record.state, ManualCompactionRequestState::Requested);
        assert_eq!(
            store
                .database
                .unit_of_work(|tx| manual::request(tx, id, &input, f.at))
                .expect("duplicate")
                .request,
            Some(record.clone())
        );
        assert_eq!(
            manual::focus(store.database.connection(), &record).expect("focus"),
            input.focus
        );
        assert!(
            store.database.unit_of_work(|tx| manual::consume(tx, &record, None, f.at)).is_err(),
            "cannot interrupt running model"
        );
        let mut changed = input.clone();
        changed.focus = Some("different".into());
        assert!(
            store.database.unit_of_work(|tx| manual::request(tx, id, &changed, f.at)).is_err(),
            "same command cannot change its content"
        );
        assert!(
            store
                .database
                .unit_of_work(|tx| manual::request(tx, CommandInvocationId::new(), &input, f.at))
                .is_err(),
            "one pending compaction"
        );
        store
            .database
            .unit_of_work(|tx| {
                inbox::submit(
                    tx,
                    CommandInvocationId::new(),
                    &AgentSubmitRequest {
                        session_id: f.turn.session_id,
                        text: "next normal turn".into(),
                        destination: AgentInputLane::NextTurn,
                        skill_ids: Vec::new(),
                        reasoning_effort: None,
                        attachments: Vec::new(),
                    },
                    f.at,
                )
            })
            .expect("queue");
        let mut output = f.template.clone();
        output.model_step_id = f.step.model_step_id;
        output.tool_calls.clear();
        store
            .database
            .unit_of_work(|tx| {
                history::record_output(tx, &output, None, f.at)?;
                let old = turns::load_run(tx, f.turn.agent_run_id)?;
                if old.state != AgentRunState::PreparingContext {
                    let mut next = old.clone();
                    next.state = AgentRunState::PreparingContext;
                    next.revision = Revision::new(old.revision.value() + 1);
                    turns::save_run(tx, &old, &next)?;
                }
                manual::consume(tx, &record, None, f.at)
            })
            .expect("no eligible history at a safe point");
        let status = manual::status(
            store.database.connection(),
            &AgentRunRequest { session_id: input.session_id, agent_run_id: input.agent_run_id },
        )
        .expect("status");
        assert_eq!(
            status.request.expect("completed").state,
            ManualCompactionRequestState::Completed
        );
        assert_eq!(
            lineage::load(store.database.connection(), f.turn.lineage_id)
                .expect("sources unchanged"),
            prior
        );
        assert_eq!(
            inbox::load(store.database.connection(), f.turn.session_id)
                .expect("queue retained")
                .messages
                .len(),
            1
        );
        assert_eq!(
            turns::load(store.database.connection(), f.turn.turn_task_id).expect("same mode").mode,
            mode
        );
    }
}

#[test]
fn interrupted_compaction_cannot_cross_sessions_or_restart_as_a_new_request() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Plan);
    let mut input = AgentCompactionRequest {
        session_id: f.turn.session_id,
        agent_run_id: f.turn.agent_run_id,
        focus: Some("x".repeat(4097)),
    };
    assert!(
        store
            .database
            .unit_of_work(|tx| manual::request(tx, CommandInvocationId::new(), &input, f.at))
            .is_err()
    );
    input.focus = None;
    let id = CommandInvocationId::new();
    store.database.unit_of_work(|tx| manual::request(tx, id, &input, f.at)).expect("request");
    assert!(
        manual::status(
            store.database.connection(),
            &AgentRunRequest { session_id: SessionId::new(), agent_run_id: input.agent_run_id }
        )
        .is_err()
    );
    store
        .database
        .unit_of_work(|tx| {
            recovery::interrupt(tx, input.agent_run_id, "agent.core_restarted", false, f.at)
        })
        .expect("restart interruption");
    let final_status = store
        .database
        .unit_of_work(|tx| manual::request(tx, id, &input, f.at))
        .expect("same receipt after interruption");
    assert_eq!(
        final_status.request.expect("record").state,
        ManualCompactionRequestState::Cancelled
    );
    assert!(
        manual::requested(store.database.connection(), input.agent_run_id)
            .expect("pending")
            .is_none()
    );
    assert!(
        store
            .database
            .unit_of_work(|tx| manual::request(tx, CommandInvocationId::new(), &input, f.at))
            .is_err(),
        "old terminal run is never revived"
    );
}

#[test]
fn failed_attempts_keep_one_manual_request_and_never_publish_partial_compaction() {
    use aura_persistence_core::StoreResult;
    use aura_persistence_v2::{agent::lifecycle, context::compaction, model};
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Plan);
    let command = CommandInvocationId::new();
    let input = AgentCompactionRequest {
        session_id: f.turn.session_id,
        agent_run_id: f.turn.agent_run_id,
        focus: None,
    };
    store
        .database
        .unit_of_work(|tx| -> StoreResult<()> {
            let record = manual::request(tx, command, &input, f.at)?.request.expect("request");
            let mut output = f.template.clone();
            output.model_step_id = f.step.model_step_id;
            output.tool_calls.clear();
            history::record_output(tx, &output, None, f.at)?;
            let old = turns::load_run(tx, input.agent_run_id)?;
            let mut next = old.clone();
            next.state = AgentRunState::PreparingContext;
            next.revision = Revision::new(old.revision.value() + 1);
            next.next_model_ordinal = f.step.ordinal + 1;
            turns::save_run(tx, &old, &next)?;
            let step = lifecycle::prepare_step(
                tx,
                input.agent_run_id,
                AgentContinuation::CompletionReminder {
                    previous_model_step_id: f.step.model_step_id,
                },
                f.at,
            )?;
            let anchors = vec![];
            let sources = b"synthetic sealed sources";
            let value = compaction::Compaction {
                id: ContextProjectionSnapshotId::new(),
                session_id: input.session_id,
                agent_run_id: input.agent_run_id,
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
                    manifest_hash: canonical_json_v1_sha256(&anchors).expect("hash"),
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
            manual::consume(tx, &record, Some(&value), f.at)?;
            for ordinal in 1..=2 {
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
                        attempt_ordinal: ordinal,
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
                compaction::finish_attempt(
                    tx,
                    value.id,
                    ordinal,
                    CompactionModelAttemptState::Failed,
                    None,
                    Some("context.invalid_compaction".into()),
                    ordinal == 1,
                    f.at,
                )?;
                let status = manual::status(
                    tx,
                    &AgentRunRequest {
                        session_id: input.session_id,
                        agent_run_id: input.agent_run_id,
                    },
                )?
                .request
                .expect("same request");
                assert_eq!(status.command_invocation_id, command);
                assert_eq!(
                    status.state,
                    if ordinal == 1 {
                        ManualCompactionRequestState::Running
                    } else {
                        ManualCompactionRequestState::Failed
                    }
                );
                assert!(
                    compaction::latest(tx, input.session_id)?.is_none(),
                    "failed/partial data never becomes the current compaction"
                );
            }
            assert_eq!(compaction::load(tx, value.id)?.attempts.len(), 2);
            Ok(())
        })
        .expect("retry lifecycle");
}
