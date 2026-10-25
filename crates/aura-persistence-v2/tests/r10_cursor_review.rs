// Review regressions restored into the product suite. Expectations stay as written.
#![allow(clippy::unwrap_used)]
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;

use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_v2::{
    V2Store,
    agent::{guard, lifecycle, mode},
    context,
};

#[test]
fn special_mode_can_be_started_again_after_termination_at_a_later_time() {
    let mut failures = Vec::new();
    for selected in [AgentTaskMode::Plan, AgentTaskMode::Goal] {
        let dir = tempfile::tempdir().unwrap();
        let mut store = V2Store::create(&dir.path().join("v2")).unwrap();
        let f = support::Fixture::new(&mut store, selected);
        let episode = mode::view(store.database.connection(), f.turn.session_id).unwrap();
        store
            .database
            .unit_of_work(|tx| {
                mode::control(
                    tx,
                    CommandInvocationId::new(),
                    &AgentModeControlRequest {
                        session_id: f.turn.session_id,
                        expected_revision: episode.revision,
                        action: AgentModeControlAction::Terminate,
                    },
                    f.at,
                )
            })
            .unwrap();
        let later = "2026-09-07T00:01:00Z".parse().unwrap();
        let result = store.database.unit_of_work(|tx| {
            let mut bundle = aura_context_fixtures::p06_prepared_context_fixture()
                .unwrap()
                .prompt_bundle_snapshot;
            bundle.id = PromptBundleSnapshotId::new();
            bundle.turn_task_id = TurnTaskId::new();
            context::insert_bundle(tx, f.turn.session_id, &bundle)?;
            let user = OriginalUserMessage::new(
                EventId::new(),
                f.turn.session_id,
                "Start a new explicitly selected mode".into(),
                UserMessageOrigin::UserInput,
                later,
            )
            .unwrap();
            lifecycle::start(tx, &user, selected, None, bundle.id)
        });
        if result.is_err() {
            failures.push(format!("{selected:?}: {result:?}"));
        }
    }
    assert!(failures.is_empty(), "new modes after termination must be allowed: {failures:?}");
}

#[test]
fn mode_control_rejects_a_revision_that_has_never_existed() {
    let mut accepted = Vec::new();
    for action in [AgentModeControlAction::Terminate, AgentModeControlAction::PauseGoal] {
        let dir = tempfile::tempdir().unwrap();
        let mut store = V2Store::create(&dir.path().join("v2")).unwrap();
        let f = support::Fixture::new(&mut store, AgentTaskMode::Goal);
        let view = mode::view(store.database.connection(), f.turn.session_id).unwrap();
        let result = store.database.unit_of_work(|tx| {
            mode::control(
                tx,
                CommandInvocationId::new(),
                &AgentModeControlRequest {
                    session_id: f.turn.session_id,
                    expected_revision: Revision::new(view.revision.value() + 100),
                    action: action.clone(),
                },
                f.at,
            )
        });
        if result.is_ok() {
            accepted.push(format!("{action:?}"));
        } else {
            assert_eq!(mode::view(store.database.connection(), f.turn.session_id).unwrap(), view);
        }
    }
    assert!(accepted.is_empty(), "accepted an impossible revision: {accepted:?}");
}

#[test]
fn distinct_constraints_on_the_same_field_have_independent_correction_opportunities() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&dir.path().join("v2")).unwrap();
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let first = DeterministicFailure::schema_constraint(
        "task_lineage.reconcile.relations",
        "expire_prior forbids relations",
        "relations not empty",
        "Omit relations when expiring a prior task",
    );
    let different = DeterministicFailure::schema_constraint(
        "task_lineage.reconcile.relations",
        "cancel_existing requires a cancels_task excerpt",
        "missing cancels_task excerpt",
        "Supply a grounded cancellation excerpt",
    );
    for attempt in 0..2 {
        let result = store
            .database
            .unit_of_work(|tx| guard::record_failure(tx, f.turn.turn_task_id, &first, f.at))
            .unwrap();
        assert!(result.is_none());
        store
            .database
            .unit_of_work(|tx| {
                guard::mark_feedback_delivered(tx, f.turn.turn_task_id, ModelStepId::new(), f.at)
            })
            .unwrap();
        assert_eq!(
            guard::load(store.database.connection(), f.turn.turn_task_id).unwrap().corrections[0]
                .remaining_attempts,
            2 - attempt
        );
    }
    let result = store
        .database
        .unit_of_work(|tx| guard::record_failure(tx, f.turn.turn_task_id, &different, f.at))
        .unwrap();
    assert_eq!(
        result, None,
        "a first failure of a different constraint must not exhaust the old sequence"
    );
}
