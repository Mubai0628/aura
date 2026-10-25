#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;
use aura_agent_domain::*;
use aura_context_domain::*;
use aura_domain_common::*;
use aura_persistence_core::StoreResult;
use aura_persistence_v2::{
    V2Store,
    agent::{continuation, inbox, lifecycle, lineage, recovery, turns},
    context::remediation,
};

fn require(store: &mut V2Store, step: &ModelStep, at: UtcTimestamp) {
    let mut budget = aura_context_fixtures::p06_prepared_context_fixture()
        .expect("budget")
        .prepared
        .context
        .snapshot
        .budget;
    budget.estimated_input_tokens = budget.request_input_budget_tokens + 1;
    let plan = ContextRemediationPlan {
        overflow: ContextOverflowDiagnosticView {
            request_input_budget_tokens: budget.request_input_budget_tokens,
            estimated_input_tokens: budget.estimated_input_tokens,
            non_compressible_tokens: budget.estimated_input_tokens,
            compressible_history_tokens: 0,
            user_reducible_tokens: 0,
            overflow_tokens: 1,
            error_code: "context.non_compressible_overflow".into(),
            remediation: vec!["reduce_user_managed_context".into()],
        },
        actions: vec!["reduce_user_managed_context".into()],
    };
    store
        .database
        .unit_of_work(|tx| remediation::require(tx, step.model_step_id, plan, budget, at))
        .expect("preflight wait");
}
fn input(store: &V2Store, turn: &TurnTask) -> AgentContextRecheckRequest {
    let record = remediation::view(
        store.database.connection(),
        &AgentRunRequest { session_id: turn.session_id, agent_run_id: turn.agent_run_id },
    )
    .expect("view")
    .remediation
    .expect("diagnostic");
    AgentContextRecheckRequest {
        session_id: turn.session_id,
        agent_run_id: turn.agent_run_id,
        source_model_step_id: record.model_step_id,
        expected_revision: record.revision,
    }
}

#[test]
fn every_mode_waits_without_consuming_queue_and_rechecks_a_new_preflight() {
    for mode in [AgentTaskMode::Normal, AgentTaskMode::Plan, AgentTaskMode::Goal] {
        let dir = tempfile::tempdir().expect("directory");
        let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
        let f = support::Fixture::preflight(&mut store, mode);
        let original =
            lineage::load(store.database.connection(), f.turn.lineage_id).expect("lineage");
        store
            .database
            .unit_of_work(|tx| {
                inbox::submit(
                    tx,
                    CommandInvocationId::new(),
                    &AgentSubmitRequest {
                        session_id: f.turn.session_id,
                        text: "/goal is queued text".into(),
                        destination: AgentInputLane::NextTurn,
                        skill_ids: Vec::new(),
                        reasoning_effort: None,
                        attachments: Vec::new(),
                    },
                    f.at,
                )
            })
            .expect("queue");
        require(&mut store, &f.step, f.at);
        let failed = turns::step(store.database.connection(), f.step.model_step_id)
            .expect("failed preflight");
        assert!(matches!(failed.state, ModelStepState::Failed { .. }));
        let request = input(&store, &f.turn);
        let command = CommandInvocationId::new();
        store
            .database
            .unit_of_work(|tx| remediation::recheck(tx, command, &request, f.at))
            .expect("explicit recheck");
        let next = store
            .database
            .unit_of_work(|tx| -> StoreResult<_> {
                lifecycle::prepare_step(
                    tx,
                    f.turn.agent_run_id,
                    continuation::next(tx, f.turn.agent_run_id)?,
                    f.at,
                )
            })
            .expect("fresh preparation");
        assert_ne!(next.model_step_id, f.step.model_step_id);
        assert_eq!(next.ordinal, 2);
        assert_eq!(next.continuation, f.step.continuation);
        require(&mut store, &next, f.at);
        let waiting =
            turns::load_run(store.database.connection(), f.turn.agent_run_id).expect("wait again");
        assert_eq!(
            waiting.state,
            AgentRunState::AwaitingContextRemediation { model_step_id: next.model_step_id }
        );
        store
            .database
            .unit_of_work(|tx| remediation::recheck(tx, command, &request, f.at))
            .expect("same command does not reapply");
        assert_eq!(
            turns::load_run(store.database.connection(), f.turn.agent_run_id).expect("unchanged"),
            waiting
        );
        assert!(
            store
                .database
                .unit_of_work(|tx| remediation::recheck(
                    tx,
                    CommandInvocationId::new(),
                    &request,
                    f.at
                ))
                .is_err(),
            "stale preparation cannot wake current wait"
        );
        assert_eq!(
            turns::step(store.database.connection(), failed.model_step_id).expect("old step"),
            failed
        );
        assert_eq!(
            lineage::load(store.database.connection(), f.turn.lineage_id).expect("sources"),
            original
        );
        assert_eq!(
            turns::load(store.database.connection(), f.turn.turn_task_id).expect("turn").mode,
            mode
        );
        assert_eq!(
            inbox::load(store.database.connection(), f.turn.session_id)
                .expect("queue")
                .messages
                .len(),
            1
        );
    }
}

#[test]
fn cross_session_recheck_is_rejected_and_cancel_never_revives() {
    let dir = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
    let f = support::Fixture::preflight(&mut store, AgentTaskMode::Goal);
    require(&mut store, &f.step, f.at);
    let request = input(&store, &f.turn);
    let mut foreign = request.clone();
    foreign.session_id = SessionId::new();
    assert!(
        store
            .database
            .unit_of_work(|tx| remediation::recheck(tx, CommandInvocationId::new(), &foreign, f.at))
            .is_err()
    );
    store
        .database
        .unit_of_work(|tx| {
            recovery::interrupt(tx, f.turn.agent_run_id, "agent.cancelled", true, f.at)
        })
        .expect("stop");
    assert!(
        store
            .database
            .unit_of_work(|tx| remediation::recheck(tx, CommandInvocationId::new(), &request, f.at))
            .is_err()
    );
    let view = remediation::view(
        store.database.connection(),
        &AgentRunRequest { session_id: f.turn.session_id, agent_run_id: f.turn.agent_run_id },
    )
    .expect("history");
    assert_eq!(view.remediation.expect("cancelled").state, AgentContextRemediationState::Cancelled);
    assert_eq!(
        turns::load_run(store.database.connection(), f.turn.agent_run_id).expect("stopped").state,
        AgentRunState::Cancelled
    );
}

#[test]
fn successful_recheck_clears_only_with_a_new_model_snapshot_and_keeps_failed_step() {
    let dir = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
    let f = support::Fixture::preflight(&mut store, AgentTaskMode::Normal);
    require(&mut store, &f.step, f.at);
    let failed = turns::step(store.database.connection(), f.step.model_step_id).expect("failed");
    let request = input(&store, &f.turn);
    store
        .database
        .unit_of_work(|tx| remediation::recheck(tx, CommandInvocationId::new(), &request, f.at))
        .expect("recheck");
    assert!(
        store
            .database
            .unit_of_work(|tx| remediation::clear(tx, f.turn.agent_run_id, false, f.at))
            .is_err(),
        "cannot clear before successful preparation"
    );
    store
        .database
        .unit_of_work(|tx| -> StoreResult<()> {
            let step = lifecycle::prepare_step(
                tx,
                f.turn.agent_run_id,
                continuation::next(tx, f.turn.agent_run_id)?,
                f.at,
            )?;
            let run = turns::load_run(tx, f.turn.agent_run_id)?;
            let mut snapshot = aura_contract_schema::canonical_fixture()
                .expect("fixture")
                .models
                .execution_snapshot
                .expect("model snapshot");
            snapshot.model_step_id = step.model_step_id;
            snapshot.prompt_bundle_snapshot_id = run.prompt_bundle_snapshot_id;
            aura_persistence_v2::model::insert_snapshot(tx, f.turn.session_id, &snapshot)?;
            let mut running_step = step.clone();
            running_step.state = ModelStepState::Running { execution_snapshot_id: snapshot.id };
            turns::save_step(tx, &step, &running_step)?;
            let mut running = run.clone();
            running.revision = Revision::new(run.revision.value() + 1);
            running.state = AgentRunState::ModelRunning { model_step_id: step.model_step_id };
            turns::save_run(tx, &run, &running)?;
            remediation::clear(tx, f.turn.agent_run_id, false, f.at)
        })
        .expect("new model snapshot and clear atomically");
    let view = remediation::view(
        store.database.connection(),
        &AgentRunRequest { session_id: f.turn.session_id, agent_run_id: f.turn.agent_run_id },
    )
    .expect("cleared");
    assert_eq!(view.remediation.expect("record").state, AgentContextRemediationState::Cleared);
    assert_eq!(
        turns::step(store.database.connection(), f.step.model_step_id).expect("old immutable step"),
        failed
    );
}

#[test]
fn restart_keeps_the_wait_but_never_automatically_runs_the_recheck() {
    use aura_persistence_v2::agent::application::{self, RunConfiguration};
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("v2");
    let mut store = V2Store::create(&path).expect("store");
    let f = support::Fixture::preflight(&mut store, AgentTaskMode::Plan);
    store
        .database
        .unit_of_work(|tx| {
            application::configure(
                tx,
                f.turn.agent_run_id,
                &RunConfiguration {
                    request: AgentStartRequest {
                        session_id: f.turn.session_id,
                        model_profile_id: ModelProfileId::new(),
                        text: "synthetic context wait".into(),
                        mode: AgentTaskMode::Plan,
                        goal_budget: None,
                        explicit_backend_id: None,
                        skill_ids: Vec::new(),
                        reasoning_effort: None,
                        attachments: Vec::new(),
                    },
                    tool_settings: aura_tool_contract::ToolSettingsSnapshotV1::default(),
                    purpose: AgentTaskPurpose::Conversation,
                },
                CommandInvocationId::new(),
                f.at,
            )
        })
        .expect("application run");
    require(&mut store, &f.step, f.at);
    let original = turns::load_run(store.database.connection(), f.turn.agent_run_id).expect("wait");
    drop(store);
    let mut store = V2Store::open(&path).expect("reopen");
    store.database.unit_of_work(|tx| recovery::startup(tx, f.at)).expect("startup");
    assert_eq!(
        turns::load_run(store.database.connection(), f.turn.agent_run_id).expect("still waiting"),
        original
    );
    let request = input(&store, &f.turn);
    store
        .database
        .unit_of_work(|tx| remediation::recheck(tx, CommandInvocationId::new(), &request, f.at))
        .expect("explicit recheck");
    store
        .database
        .unit_of_work(|tx| recovery::startup(tx, f.at))
        .expect("crash before new preflight");
    assert!(matches!(
        turns::load_run(store.database.connection(), f.turn.agent_run_id)
            .expect("interrupted")
            .state,
        AgentRunState::RecoveryRequired { .. }
    ));
    assert!(
        store
            .database
            .unit_of_work(|tx| remediation::recheck(tx, CommandInvocationId::new(), &request, f.at))
            .is_err()
    );
}
