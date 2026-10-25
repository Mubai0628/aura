#![allow(clippy::unwrap_used)]
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_model_domain::NormalizedStopReason;
use aura_persistence_v2::{
    V2Store,
    agent::{guard, lifecycle, lineage, mode, reconcile, session, turns},
    context,
};

fn next_turn(store: &mut V2Store, old: &TurnTask, text: &str) -> TurnTask {
    store
        .database
        .unit_of_work(|tx| {
            let mut closed = old.clone();
            closed
                .transition(
                    old.revision,
                    TurnTaskState::Completed { final_report: "Turn finished".into() },
                    old.updated_at,
                )
                .expect("transition");
            turns::save_completed(tx, old, &closed)?;
            let previous = session::load(tx, old.session_id)?;
            let mut idle = previous.clone();
            idle.active_turn = None;
            idle.active_lineage = None;
            idle.revision = Revision::new(previous.revision.value() + 1);
            session::save(tx, &previous, &idle, old.updated_at)?;
            let mut bundle = aura_context_fixtures::p06_prepared_context_fixture()
                .expect("bundle fixture")
                .prompt_bundle_snapshot;
            bundle.id = PromptBundleSnapshotId::new();
            bundle.turn_task_id = TurnTaskId::new();
            context::insert_bundle(tx, old.session_id, &bundle)?;
            let user = OriginalUserMessage::new(
                EventId::new(),
                old.session_id,
                text.into(),
                UserMessageOrigin::UserInput,
                old.updated_at,
            )
            .expect("user");
            lifecycle::start(tx, &user, AgentTaskMode::Normal, None, bundle.id)
        })
        .expect("next turn")
}

#[test]
fn plan_and_goal_persist_across_new_turns_until_terminated() {
    for mode in [AgentTaskMode::Plan, AgentTaskMode::Goal] {
        let dir = tempfile::tempdir().expect("dir");
        let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
        let f = support::Fixture::new(&mut store, mode);
        let view = mode::view(store.database.connection(), f.turn.session_id).expect("episode");
        assert_eq!(view.mode, mode);
        let next = next_turn(&mut store, &f.turn, "Stay in the same mode");
        assert_eq!(next.mode, mode);
        let after = mode::view(store.database.connection(), next.session_id).expect("kept");
        assert_eq!(after.mode, mode);
        assert!(!after.paused);
        store
            .database
            .unit_of_work(|tx| {
                mode::control(
                    tx,
                    CommandInvocationId::new(),
                    &AgentModeControlRequest {
                        session_id: next.session_id,
                        expected_revision: after.revision,
                        action: AgentModeControlAction::Terminate,
                    },
                    f.at,
                )
            })
            .expect("terminate");
        assert_eq!(
            mode::view(store.database.connection(), next.session_id).expect("normal").mode,
            AgentTaskMode::Normal
        );
    }
}

#[test]
fn goal_pause_and_budget_edit_keep_usage() {
    let dir = tempfile::tempdir().expect("dir");
    let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Goal);
    let view = mode::view(store.database.connection(), f.turn.session_id).expect("episode");
    let paused = store
        .database
        .unit_of_work(|tx| {
            mode::control(
                tx,
                CommandInvocationId::new(),
                &AgentModeControlRequest {
                    session_id: f.turn.session_id,
                    expected_revision: view.revision,
                    action: AgentModeControlAction::PauseGoal,
                },
                f.at,
            )
        })
        .expect("pause");
    assert!(paused.paused);
    assert_eq!(paused.mode, AgentTaskMode::Goal);
    let edited = store
        .database
        .unit_of_work(|tx| {
            mode::control(
                tx,
                CommandInvocationId::new(),
                &AgentModeControlRequest {
                    session_id: f.turn.session_id,
                    expected_revision: paused.revision,
                    action: AgentModeControlAction::EditGoalBudget {
                        goal_budget: GoalBudget { max_tokens: Some(9_000), max_turns: Some(20) },
                    },
                },
                f.at,
            )
        })
        .expect("edit");
    assert_eq!(edited.goal_budget.as_ref().map(|b| b.max_tokens), Some(Some(9_000)));
    assert_eq!(
        edited.inherited_goal.as_ref().map(|g| g.consumed_tokens),
        paused.inherited_goal.as_ref().map(|g| g.consumed_tokens)
    );
    store
        .database
        .unit_of_work(|tx| {
            mode::control(
                tx,
                CommandInvocationId::new(),
                &AgentModeControlRequest {
                    session_id: f.turn.session_id,
                    expected_revision: edited.revision,
                    action: AgentModeControlAction::PauseGoal,
                },
                f.at,
            )
        })
        .expect("still goal");
}

#[test]
fn plan_rejects_pause_and_budget_edit() {
    let dir = tempfile::tempdir().expect("dir");
    let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Plan);
    let view = mode::view(store.database.connection(), f.turn.session_id).expect("episode");
    assert!(
        store
            .database
            .unit_of_work(|tx| {
                mode::control(
                    tx,
                    CommandInvocationId::new(),
                    &AgentModeControlRequest {
                        session_id: f.turn.session_id,
                        expected_revision: view.revision,
                        action: AgentModeControlAction::PauseGoal,
                    },
                    f.at,
                )
            })
            .is_err()
    );
    assert!(
        store
            .database
            .unit_of_work(|tx| {
                mode::control(
                    tx,
                    CommandInvocationId::new(),
                    &AgentModeControlRequest {
                        session_id: f.turn.session_id,
                        expected_revision: view.revision,
                        action: AgentModeControlAction::EditGoalBudget {
                            goal_budget: GoalBudget { max_tokens: Some(1_000), max_turns: None },
                        },
                    },
                    f.at,
                )
            })
            .is_err()
    );
    assert_eq!(
        mode::view(store.database.connection(), f.turn.session_id).expect("unchanged").mode,
        AgentTaskMode::Plan
    );
}

#[test]
fn paused_goal_continues_on_the_next_user_message() {
    let dir = tempfile::tempdir().expect("dir");
    let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Goal);
    let view = mode::view(store.database.connection(), f.turn.session_id).expect("episode");
    store
        .database
        .unit_of_work(|tx| {
            mode::control(
                tx,
                CommandInvocationId::new(),
                &AgentModeControlRequest {
                    session_id: f.turn.session_id,
                    expected_revision: view.revision,
                    action: AgentModeControlAction::PauseGoal,
                },
                f.at,
            )
        })
        .expect("pause");
    let next = store
        .database
        .unit_of_work(|tx| {
            let mut bundle = aura_context_fixtures::p06_prepared_context_fixture()
                .expect("bundle fixture")
                .prompt_bundle_snapshot;
            bundle.id = PromptBundleSnapshotId::new();
            bundle.turn_task_id = TurnTaskId::new();
            context::insert_bundle(tx, f.turn.session_id, &bundle)?;
            let user = OriginalUserMessage::new(
                EventId::new(),
                f.turn.session_id,
                "Continue the paused Goal".into(),
                UserMessageOrigin::UserInput,
                f.at,
            )
            .expect("user");
            lifecycle::start(tx, &user, AgentTaskMode::Normal, None, bundle.id)
        })
        .expect("next message");
    assert_eq!(next.mode, AgentTaskMode::Normal);
    let held = mode::view(store.database.connection(), next.session_id).expect("held");
    assert_eq!(held.mode, AgentTaskMode::Goal);
    assert!(held.paused);
    assert!(held.disposition_required);
}

fn start_message(
    store: &mut V2Store,
    session: SessionId,
    text: &str,
    mode: AgentTaskMode,
    budget: Option<GoalBudget>,
    at: UtcTimestamp,
) -> TurnTask {
    store
        .database
        .unit_of_work(|tx| {
            let mut bundle = aura_context_fixtures::p06_prepared_context_fixture()
                .expect("bundle fixture")
                .prompt_bundle_snapshot;
            bundle.id = PromptBundleSnapshotId::new();
            bundle.turn_task_id = TurnTaskId::new();
            context::insert_bundle(tx, session, &bundle)?;
            let user = OriginalUserMessage::new(
                EventId::new(),
                session,
                text.into(),
                UserMessageOrigin::UserInput,
                at,
            )
            .expect("user");
            lifecycle::start(tx, &user, mode, budget, bundle.id)
        })
        .expect("start message")
}

#[test]
fn paused_goal_continues_only_after_an_explicit_decision() {
    let dir = tempfile::tempdir().expect("dir");
    let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Goal);
    let view = mode::view(store.database.connection(), f.turn.session_id).expect("episode");
    store
        .database
        .unit_of_work(|tx| {
            mode::control(
                tx,
                CommandInvocationId::new(),
                &AgentModeControlRequest {
                    session_id: f.turn.session_id,
                    expected_revision: view.revision,
                    action: AgentModeControlAction::PauseGoal,
                },
                f.at,
            )
        })
        .expect("pause");
    let next = start_message(
        &mut store,
        f.turn.session_id,
        "Continue the paused Goal",
        AgentTaskMode::Normal,
        None,
        f.at,
    );
    assert_eq!(next.mode, AgentTaskMode::Normal);
    store
        .database
        .unit_of_work(|tx| {
            mode::decide(
                tx,
                next.turn_task_id,
                &ModeDispositionInput {
                    decision: ModeDispositionDecision::Continue,
                    reason: "The user asked to continue this Goal.".into(),
                },
                f.at,
            )
        })
        .expect("continue");
    let continued = turns::load(store.database.connection(), next.turn_task_id).expect("turn");
    assert_eq!(continued.mode, AgentTaskMode::Goal);
    let episode = mode::view(store.database.connection(), next.session_id).expect("episode");
    assert_eq!(episode.mode, AgentTaskMode::Goal);
    assert!(!episode.paused);
    assert!(!episode.disposition_required);
    assert!(episode.inherited_goal.is_some() || episode.goal_budget.is_some());
}

#[test]
fn unrelated_message_can_skip_a_paused_goal() {
    let dir = tempfile::tempdir().expect("dir");
    let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Goal);
    let view = mode::view(store.database.connection(), f.turn.session_id).expect("episode");
    store
        .database
        .unit_of_work(|tx| {
            mode::control(
                tx,
                CommandInvocationId::new(),
                &AgentModeControlRequest {
                    session_id: f.turn.session_id,
                    expected_revision: view.revision,
                    action: AgentModeControlAction::PauseGoal,
                },
                f.at,
            )
        })
        .expect("pause");
    let next = start_message(
        &mut store,
        f.turn.session_id,
        "Please answer a different question",
        AgentTaskMode::Normal,
        None,
        f.at,
    );
    let skipped = store
        .database
        .unit_of_work(|tx| {
            mode::decide(
                tx,
                next.turn_task_id,
                &ModeDispositionInput {
                    decision: ModeDispositionDecision::Skip,
                    reason: "The message is unrelated to the paused Goal.".into(),
                },
                f.at,
            )
        })
        .expect("skip");
    assert_eq!(skipped.mode, AgentTaskMode::Normal);
    assert_eq!(skipped.end_reason, Some(ModeEpisodeEndReason::Skipped));
    assert_eq!(
        turns::load(store.database.connection(), next.turn_task_id).expect("turn").mode,
        AgentTaskMode::Normal
    );
}

#[test]
fn explicit_goal_after_pause_starts_a_new_cycle() {
    let dir = tempfile::tempdir().expect("dir");
    let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Goal);
    let view = mode::view(store.database.connection(), f.turn.session_id).expect("episode");
    let before =
        mode::load(store.database.connection(), f.turn.session_id).expect("episode").unwrap();
    store
        .database
        .unit_of_work(|tx| {
            mode::control(
                tx,
                CommandInvocationId::new(),
                &AgentModeControlRequest {
                    session_id: f.turn.session_id,
                    expected_revision: view.revision,
                    action: AgentModeControlAction::PauseGoal,
                },
                f.at,
            )
        })
        .expect("pause");
    let later = "2026-09-07T00:01:00Z".parse().expect("time");
    let next = start_message(
        &mut store,
        f.turn.session_id,
        "Start a new Goal",
        AgentTaskMode::Goal,
        Some(GoalBudget { max_tokens: Some(50), max_turns: Some(2) }),
        later,
    );
    assert_eq!(next.mode, AgentTaskMode::Goal);
    let episode = mode::load(store.database.connection(), next.session_id).expect("load").unwrap();
    assert_eq!(episode.created_at, later);
    assert_ne!(episode.created_at, before.created_at);
    assert!(episode.inherited_goal.is_none());
    assert_eq!(episode.goal_budget.unwrap().max_tokens, Some(50));
    assert!(!episode.paused);
    let history: Vec<String> = {
        let connection = store.database.connection();
        let mut query = connection
            .prepare("SELECT payload FROM events WHERE kind='agent.mode_episode' ORDER BY sequence")
            .expect("history");
        query
            .query_map([], |row| row.get::<_, String>(0))
            .expect("rows")
            .map(|row| row.expect("payload"))
            .collect()
    };
    assert!(history.iter().any(|payload| payload.contains("\"skipped\"")));
}

#[test]
fn protocol_end_turn_reminds_once_then_stops() {
    let dir = tempfile::tempdir().expect("dir");
    let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let first = store
        .database
        .unit_of_work(|tx| {
            guard::note_end_turn(
                tx,
                f.turn.agent_run_id,
                NormalizedStopReason::EndTurn,
                false,
                f.at,
            )
        })
        .expect("first");
    assert_eq!(first, None);
    assert!(
        guard::load(store.database.connection(), f.turn.turn_task_id)
            .expect("guard")
            .completion_reminder_sent
    );
    store
        .database
        .unit_of_work(|tx| {
            guard::mark_feedback_delivered(tx, f.turn.turn_task_id, f.step.model_step_id, f.at)
        })
        .expect("feedback");
    let stopped = store
        .database
        .unit_of_work(|tx| {
            guard::note_end_turn(
                tx,
                f.turn.agent_run_id,
                NormalizedStopReason::EndTurn,
                false,
                f.at,
            )
        })
        .expect("second");
    assert_eq!(stopped, Some(TurnGuardStopReason::ProtocolNotCompleted));
    assert_eq!(
        guard::blocked(store.database.connection(), f.turn.turn_task_id).expect("blocked"),
        Some(TurnGuardStopReason::ProtocolNotCompleted)
    );
}

#[test]
fn truncated_or_refused_output_does_not_start_the_protocol_stop() {
    let dir = tempfile::tempdir().expect("dir");
    let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    for stop in [
        NormalizedStopReason::MaxOutputTokens,
        NormalizedStopReason::Refusal,
        NormalizedStopReason::ProviderIncomplete,
        NormalizedStopReason::Unknown,
    ] {
        let noted = store
            .database
            .unit_of_work(|tx| guard::note_end_turn(tx, f.turn.agent_run_id, stop, false, f.at))
            .expect("ignored");
        assert_eq!(noted, None);
    }
    let guard = guard::load(store.database.connection(), f.turn.turn_task_id).expect("guard");
    assert!(!guard.completion_reminder_sent);
    assert!(guard.stopped.is_none());
}

#[test]
fn deterministic_correction_stops_after_two_post_feedback_failures() {
    let dir = tempfile::tempdir().expect("dir");
    let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let failure = DeterministicFailure::schema_constraint(
        "limit",
        "1..=100",
        "101",
        "Submit a value in 1..=100",
    );
    store
        .database
        .unit_of_work(|tx| guard::record_failure(tx, f.turn.turn_task_id, &failure, f.at))
        .expect("first");
    store
        .database
        .unit_of_work(|tx| {
            guard::mark_feedback_delivered(tx, f.turn.turn_task_id, f.step.model_step_id, f.at)
        })
        .expect("feedback 1");
    store
        .database
        .unit_of_work(|tx| guard::record_failure(tx, f.turn.turn_task_id, &failure, f.at))
        .expect("second");
    store
        .database
        .unit_of_work(|tx| {
            guard::mark_feedback_delivered(tx, f.turn.turn_task_id, f.step.model_step_id, f.at)
        })
        .expect("feedback 2");
    let stopped = store
        .database
        .unit_of_work(|tx| guard::record_failure(tx, f.turn.turn_task_id, &failure, f.at))
        .expect("third");
    assert_eq!(stopped, Some(TurnGuardStopReason::DeterministicCorrectionExhausted));
}

#[test]
fn raising_the_same_limit_stays_in_sequence_and_an_accepted_value_starts_another() {
    let dir = tempfile::tempdir().expect("dir");
    let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let over = DeterministicFailure::schema_constraint(
        "limit",
        "1..=100",
        "101",
        "Submit a value in 1..=100",
    );
    let higher = DeterministicFailure::schema_constraint(
        "limit",
        "1..=100",
        "102",
        "Submit a value in 1..=100",
    );
    let accepted = DeterministicFailure::schema_constraint(
        "limit",
        "accepted value",
        "100",
        "100 is inside the limit and is a different constraint",
    );
    store
        .database
        .unit_of_work(|tx| guard::record_failure(tx, f.turn.turn_task_id, &over, f.at))
        .expect("101");
    store
        .database
        .unit_of_work(|tx| {
            guard::mark_feedback_delivered(tx, f.turn.turn_task_id, f.step.model_step_id, f.at)
        })
        .expect("feedback");
    let continued = store
        .database
        .unit_of_work(|tx| guard::record_failure(tx, f.turn.turn_task_id, &higher, f.at))
        .expect("102");
    assert_eq!(continued, None);
    let fresh = store
        .database
        .unit_of_work(|tx| guard::record_failure(tx, f.turn.turn_task_id, &accepted, f.at))
        .expect("100");
    assert_eq!(fresh, None);
    let guard = guard::load(store.database.connection(), f.turn.turn_task_id).expect("guard");
    assert_eq!(guard.corrections.len(), 2);
    assert_eq!(guard.corrections[0].remaining_attempts, 1);
    assert_eq!(guard.corrections[1].remaining_attempts, 2);
}

#[test]
fn reference_prior_does_not_activate_an_interrupted_lineage() {
    let dir = tempfile::tempdir().expect("dir");
    let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let original = lineage::load(store.database.connection(), f.turn.lineage_id).expect("original");
    let turn = next_turn(&mut store, &f.turn, "Use the earlier facts");
    let prior = lineage::load(store.database.connection(), original.lineage_id).expect("prior");
    let current = lineage::load(store.database.connection(), turn.lineage_id).expect("current");
    let referenced = store
        .database
        .unit_of_work(|tx| {
            reconcile::apply(
                tx,
                turn.turn_task_id,
                &TaskLineageReconcileInput {
                    current_user_event_id: turn.root_user_event_id,
                    active_lineage_id: Some(current.lineage_id),
                    expected_lineage_revision: Some(current.revision),
                    decision: TaskLineageReconcileDecision::ReferencePrior {
                        lineage_id: original.lineage_id,
                    },
                    relations: vec![],
                    completion_policy_update: None,
                    reason: "Cite readable background".into(),
                },
                f.at,
            )
        })
        .expect("reference");
    assert_eq!(referenced.lineage_id, current.lineage_id);
    assert_eq!(referenced.derived_from_lineage_id, Some(original.lineage_id));
    assert_eq!(
        lineage::load(store.database.connection(), prior.lineage_id)
            .expect("still interrupted")
            .state,
        TaskLineageState::InterruptedResumable
    );
}

/// A reference to a prior
/// task relates the new message to the prior task's own message.
#[test]
fn reference_prior_relates_the_new_message_to_the_prior_tasks_own_message() {
    let dir = tempfile::tempdir().expect("dir");
    let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let original = lineage::load(store.database.connection(), f.turn.lineage_id).expect("original");
    let turn = next_turn(&mut store, &f.turn, "继续");
    let current = lineage::load(store.database.connection(), turn.lineage_id).expect("current");
    let excerpt =
        |text: &str| UserMessageExcerptSelector { exact_text: text.into(), occurrence: 0 };
    let referenced = store
        .database
        .unit_of_work(|tx| {
            reconcile::apply(
                tx,
                turn.turn_task_id,
                &TaskLineageReconcileInput {
                    current_user_event_id: turn.root_user_event_id,
                    active_lineage_id: Some(current.lineage_id),
                    expected_lineage_revision: Some(current.revision),
                    decision: TaskLineageReconcileDecision::ReferencePrior {
                        lineage_id: original.lineage_id,
                    },
                    relations: vec![TaskLineageRelationInput {
                        earlier_user_event_id: Some(f.turn.root_user_event_id),
                        later_user_event_id: turn.root_user_event_id,
                        kind: TaskLineageRelationKind::Continues,
                        earlier_excerpt: Some(excerpt("requested work")),
                        later_excerpt: Some(excerpt("继续")),
                    }],
                    completion_policy_update: None,
                    reason: "继续之前的任务".into(),
                },
                f.at,
            )
        })
        .expect("the prior task's own message is a valid earlier message");
    assert_eq!(referenced.derived_from_lineage_id, Some(original.lineage_id));
    // The reference is the link; the current task's own relations stay among its own messages.
    assert!(
        referenced
            .relations
            .iter()
            .all(|relation| relation.earlier_user_event_id != Some(f.turn.root_user_event_id))
    );
}
