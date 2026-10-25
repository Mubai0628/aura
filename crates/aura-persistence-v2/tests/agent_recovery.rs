#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::StoreResult;
use aura_persistence_v2::{
    V2Store,
    agent::{history, lifecycle, lineage, mode, reconcile, recovery, related, turns, work},
    context, model,
};

fn next_start(store: &mut V2Store, source: &TurnTask, text: &str) -> TurnTask {
    store
        .database
        .unit_of_work(|tx| -> StoreResult<TurnTask> {
            let mut bundle = aura_context_fixtures::p06_prepared_context_fixture()
                .expect("bundle")
                .prompt_bundle_snapshot;
            bundle.id = PromptBundleSnapshotId::new();
            bundle.turn_task_id = TurnTaskId::new();
            context::insert_bundle(tx, source.session_id, &bundle)?;
            let user = OriginalUserMessage::new(
                EventId::new(),
                source.session_id,
                text.into(),
                UserMessageOrigin::UserInput,
                source.updated_at,
            )
            .expect("user");
            lifecycle::start(tx, &user, AgentTaskMode::Normal, None, bundle.id)
        })
        .expect("new independent execution")
}

fn stop(store: &mut V2Store, turn: &TurnTask) -> TurnTask {
    store
        .database
        .unit_of_work(|tx| {
            recovery::interrupt(tx, turn.agent_run_id, "test interruption", true, turn.updated_at)?;
            turns::load(tx, turn.turn_task_id)
        })
        .expect("stopped execution")
}

#[test]
fn recover_entry_is_rejected_and_new_turns_keep_prior_lineages_inactive() {
    for mode in [AgentTaskMode::Normal, AgentTaskMode::Plan, AgentTaskMode::Goal] {
        let root = tempfile::tempdir().expect("directory");
        let mut store = V2Store::create(&root.path().join("v2")).expect("store");
        let f = support::Fixture::new(&mut store, mode);
        let before = lineage::load(store.database.connection(), f.turn.lineage_id).expect("root");
        let terminal = stop(&mut store, &f.turn);
        let interrupted =
            lineage::load(store.database.connection(), before.lineage_id).expect("interrupted");
        assert_eq!(interrupted.state, TaskLineageState::InterruptedResumable);
        let request = AgentRelatedTurnRequest {
            session_id: terminal.session_id,
            source_turn_task_id: terminal.turn_task_id,
            expected_revision: terminal.revision,
            kind: AgentRelatedTurnKind::Recover,
            goal_budget: None,
        };
        assert!(related::validate(store.database.connection(), &request).is_err());
        let next = next_start(&mut store, &terminal, "Continue from readable background");
        // The new turn enters as Normal and waits for the intent decision instead of
        // silently restoring the interrupted mode.
        assert_eq!(next.mode, AgentTaskMode::Normal);
        let waiting =
            mode::view(store.database.connection(), terminal.session_id).expect("episode");
        assert_eq!(waiting.mode, mode);
        if mode == AgentTaskMode::Normal {
            assert!(!waiting.disposition_required);
        } else {
            assert!(waiting.disposition_required, "an interrupted {mode:?} waits for a decision");
            let decided = store
                .database
                .unit_of_work(|tx| {
                    mode::decide(
                        tx,
                        next.turn_task_id,
                        &ModeDispositionInput {
                            decision: ModeDispositionDecision::Continue,
                            reason: "The user continues the interrupted mode".into(),
                        },
                        f.at,
                    )
                })
                .expect("continue disposition");
            assert_eq!(decided.mode, mode);
            assert!(!decided.disposition_required);
            assert_eq!(
                turns::load(store.database.connection(), next.turn_task_id)
                    .expect("adopted turn")
                    .mode,
                mode
            );
            // Continue adopts the mode on this turn only: the old lineage stays
            // interrupted and the mode cycle is not a licence to reactivate it.
            assert_eq!(
                lineage::load(store.database.connection(), before.lineage_id)
                    .expect("unchanged")
                    .state,
                TaskLineageState::InterruptedResumable
            );
        }
        assert!(!next.lineage_resolution_required);
        assert_eq!(
            lineage::load(store.database.connection(), before.lineage_id).expect("unchanged").state,
            TaskLineageState::InterruptedResumable
        );
        let current = lineage::load(store.database.connection(), next.lineage_id).expect("current");
        assert_eq!(current.state, TaskLineageState::Active);
        let resume = TaskLineageReconcileInput {
            current_user_event_id: next.root_user_event_id,
            active_lineage_id: Some(before.lineage_id),
            expected_lineage_revision: Some(interrupted.revision),
            decision: TaskLineageReconcileDecision::ResumeSuspended {
                lineage_id: before.lineage_id,
            },
            relations: vec![],
            completion_policy_update: None,
            reason: "must not reactivate".into(),
        };
        assert!(
            store
                .database
                .unit_of_work(|tx| reconcile::apply(tx, next.turn_task_id, &resume, f.at))
                .is_err()
        );
        assert_eq!(
            turns::load(store.database.connection(), terminal.turn_task_id).expect("old terminal"),
            terminal
        );
    }
}

#[test]
fn goal_usage_accumulates_across_adjudicated_turns_without_recover() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Goal);
    let mut output = f.template.clone();
    output.model_step_id = f.step.model_step_id;
    output.tool_calls.clear();
    store
        .database
        .unit_of_work(|tx| history::record_output(tx, &output, None, f.at))
        .expect("real response usage");
    let terminal = stop(&mut store, &f.turn);
    let original =
        work::goal(store.database.connection(), terminal.turn_task_id).expect("original usage");
    assert!(original.consumed_tokens > 0);
    let next = next_start(&mut store, &terminal, "Continue the same Goal");
    // An interrupted Goal is not inherited silently: the new turn waits for the AI
    // decision, and only Continue re-authorizes the same budget and usage.
    assert_eq!(next.mode, AgentTaskMode::Normal);
    let waiting = mode::view(store.database.connection(), next.session_id).expect("episode");
    assert!(waiting.disposition_required);
    assert_eq!(waiting.mode, AgentTaskMode::Goal);
    let continue_goal = ModeDispositionInput {
        decision: ModeDispositionDecision::Continue,
        reason: "The user continues the same Goal within its budget".into(),
    };
    let decided = store
        .database
        .unit_of_work(|tx| mode::decide(tx, next.turn_task_id, &continue_goal, f.at))
        .expect("continue disposition");
    assert_eq!(decided.mode, AgentTaskMode::Goal);
    assert_eq!(
        turns::load(store.database.connection(), next.turn_task_id).expect("adopted turn").mode,
        AgentTaskMode::Goal
    );
    let inherited = work::goal(store.database.connection(), next.turn_task_id).expect("inherited");
    assert_eq!(inherited.budget, original.budget);
    assert_eq!(
        (inherited.consumed_tokens, inherited.consumed_turns),
        (original.consumed_tokens, original.consumed_turns)
    );
    // The decision is once: repeating it cannot re-authorize the same cycle again.
    assert!(
        store
            .database
            .unit_of_work(|tx| mode::decide(tx, next.turn_task_id, &continue_goal, f.at))
            .is_err(),
        "a decided episode cannot be decided twice"
    );
    let view = mode::view(store.database.connection(), next.session_id).expect("episode");
    assert_eq!(view.mode, AgentTaskMode::Goal);
    store
        .database
        .unit_of_work(|tx| -> StoreResult<()> {
            let step = lifecycle::prepare_step(
                tx,
                next.agent_run_id,
                AgentContinuation::Initial { user_event_id: next.root_user_event_id },
                f.at,
            )?;
            let mut snapshot = model::step_snapshot(tx, f.step.model_step_id)?;
            snapshot.id = ModelExecutionSnapshotId::new();
            snapshot.model_step_id = step.model_step_id;
            let run = turns::load_run(tx, next.agent_run_id)?;
            snapshot.prompt_bundle_snapshot_id = run.prompt_bundle_snapshot_id;
            model::insert_snapshot(tx, next.session_id, &snapshot)?;
            let mut running = step.clone();
            running.state = ModelStepState::Running { execution_snapshot_id: snapshot.id };
            turns::save_step(tx, &step, &running)?;
            let mut active = run.clone();
            active.revision = Revision::new(run.revision.value() + 1);
            active.state = AgentRunState::ModelRunning { model_step_id: step.model_step_id };
            turns::save_run(tx, &run, &active)?;
            output.model_step_id = step.model_step_id;
            history::record_output(tx, &output, None, f.at)?;
            Ok(())
        })
        .expect("another real response");
    let next_terminal = stop(&mut store, &next);
    let final_turn = next_start(&mut store, &next_terminal, "Stay on the same Goal");
    // The second interruption again waits for the decision before the usage moves.
    assert_eq!(final_turn.mode, AgentTaskMode::Normal);
    let decided_again = store
        .database
        .unit_of_work(|tx| mode::decide(tx, final_turn.turn_task_id, &continue_goal, f.at))
        .expect("continue disposition after the second interruption");
    assert_eq!(decided_again.mode, AgentTaskMode::Goal);
    let total =
        work::goal(store.database.connection(), final_turn.turn_task_id).expect("two turns");
    assert_eq!(total.consumed_tokens, 2 * original.consumed_tokens);
    assert_eq!(total.consumed_turns, 2 * original.consumed_turns);
    assert_eq!(total.budget, original.budget);
    assert_eq!(
        work::goal(store.database.connection(), terminal.turn_task_id).expect("old usage"),
        original
    );
}
