#[path = "support/agent.rs"]
#[allow(dead_code)] // Shared database fixture also supports completion execution tests.
mod support;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_v2::{
    V2Store,
    agent::{lifecycle, lineage, reconcile, session, turns},
    context,
};

fn next_turn(store: &mut V2Store, old: &TurnTask, text: &str) -> TurnTask {
    store
        .database
        .unit_of_work(|tx| {
            let mut closed = old.clone();
            closed
                .transition(old.revision, TurnTaskState::Cancelled, old.updated_at)
                .expect("transition");
            turns::save(tx, old, &closed)?;
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
        .expect("next real user turn")
}
fn decision(
    turn: &TurnTask,
    prior: &TaskLineage,
    kind: TaskLineageReconcileDecision,
    relation: TaskLineageRelationKind,
    text: &str,
) -> TaskLineageReconcileInput {
    TaskLineageReconcileInput {
        current_user_event_id: turn.root_user_event_id,
        active_lineage_id: Some(prior.lineage_id),
        expected_lineage_revision: Some(prior.revision),
        decision: kind,
        relations: vec![TaskLineageRelationInput {
            earlier_user_event_id: Some(prior.root_user_event_id),
            later_user_event_id: turn.root_user_event_id,
            kind: relation,
            earlier_excerpt: None,
            later_excerpt: Some(UserMessageExcerptSelector {
                exact_text: text.into(),
                occurrence: 0,
            }),
        }],
        completion_policy_update: None,
        reason: "Relation grounded in the exact user source".into(),
    }
}
#[test]
fn idle_next_turn_owns_an_active_lineage_without_reactivating_the_prior() {
    let dir = tempfile::tempdir().expect("dir");
    let root = dir.path().join("v2");
    let mut store = V2Store::create(&root).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let original = lineage::load(store.database.connection(), f.turn.lineage_id).expect("lineage");
    let turn = next_turn(&mut store, &f.turn, "Also test cancellation");
    let state = session::load(store.database.connection(), turn.session_id).expect("state");
    let current = lineage::load(store.database.connection(), turn.lineage_id).expect("current");
    let prior = lineage::load(store.database.connection(), original.lineage_id).expect("prior");
    assert!(!turn.lineage_resolution_required);
    assert_eq!(state.active_lineage, Some(turn.lineage_id));
    assert_eq!(current.state, TaskLineageState::Active);
    assert_eq!(prior.state, TaskLineageState::InterruptedResumable);
    assert_ne!(current.lineage_id, original.lineage_id);
    let resume = TaskLineageReconcileInput {
        current_user_event_id: turn.root_user_event_id,
        active_lineage_id: Some(original.lineage_id),
        expected_lineage_revision: Some(prior.revision),
        decision: TaskLineageReconcileDecision::ResumeSuspended { lineage_id: original.lineage_id },
        relations: vec![],
        completion_policy_update: None,
        reason: "Must not reactivate".into(),
    };
    assert!(
        store
            .database
            .unit_of_work(|tx| reconcile::apply(tx, turn.turn_task_id, &resume, f.at))
            .is_err()
    );
    let expire = TaskLineageReconcileInput {
        current_user_event_id: turn.root_user_event_id,
        active_lineage_id: Some(turn.lineage_id),
        expected_lineage_revision: Some(prior.revision),
        decision: TaskLineageReconcileDecision::ExpirePrior { lineage_id: original.lineage_id },
        relations: vec![],
        completion_policy_update: None,
        reason: "Background record is no longer a current obligation".into(),
    };
    let expired = store
        .database
        .unit_of_work(|tx| reconcile::apply(tx, turn.turn_task_id, &expire, f.at))
        .expect("expire");
    assert_eq!(expired.state, TaskLineageState::Expired);
    assert_eq!(
        lineage::load(store.database.connection(), turn.lineage_id).expect("current stays").state,
        TaskLineageState::Active
    );
    assert!(
        store
            .database
            .unit_of_work(|tx| {
                let previous = lineage::load(tx, original.lineage_id)?;
                let mut resumed = previous.clone();
                resumed.state = TaskLineageState::Active;
                resumed.advance_revision(previous.revision, f.at).expect("revision");
                lineage::save(tx, &previous, &resumed)
            })
            .is_err()
    );
}
#[test]
fn mixed_source_ranges_remain_distinct_and_missing_text_rolls_back() {
    let dir = tempfile::tempdir().expect("dir");
    let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let turn = next_turn(&mut store, &f.turn, "补测取消。另写报告。");
    let current = lineage::load(store.database.connection(), turn.lineage_id).expect("current");
    let mut input = decision(
        &turn,
        &current,
        TaskLineageReconcileDecision::Mixed,
        TaskLineageRelationKind::AddsRequirement,
        "补测取消。",
    );
    assert!(
        store
            .database
            .unit_of_work(|tx| reconcile::apply(tx, turn.turn_task_id, &input, f.at))
            .is_err()
    );
    input.relations.push(TaskLineageRelationInput {
        earlier_user_event_id: None,
        later_user_event_id: turn.root_user_event_id,
        kind: TaskLineageRelationKind::ReclassifiesUnrelatedPart,
        earlier_excerpt: None,
        later_excerpt: Some(UserMessageExcerptSelector {
            exact_text: "另写报告。".into(),
            occurrence: 0,
        }),
    });
    let selected = store
        .database
        .unit_of_work(|tx| reconcile::apply(tx, turn.turn_task_id, &input, f.at))
        .expect("mixed");
    assert_eq!(selected.state, TaskLineageState::Active);
    let state = session::load(store.database.connection(), turn.session_id).expect("state");
    assert!(state.open_lineages.len() >= 2);
    let sibling = state
        .open_lineages
        .iter()
        .map(|id| lineage::load(store.database.connection(), *id).expect("open"))
        .find(|l| l.state == TaskLineageState::SuspendedByTopicSwitch)
        .expect("sibling");
    assert_eq!(sibling.sources[0].immutable_preview, "另写报告。");
    assert_eq!(sibling.root_user_event_id, turn.root_user_event_id);
}
#[test]
fn explicit_cancellation_preserves_source_history_and_cannot_be_resumed() {
    let dir = tempfile::tempdir().expect("dir");
    let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let old = lineage::load(store.database.connection(), f.turn.lineage_id).expect("lineage");
    let turn = next_turn(&mut store, &f.turn, "Cancel that task");
    let current = lineage::load(store.database.connection(), turn.lineage_id).expect("current");
    let mut input = decision(
        &turn,
        &current,
        TaskLineageReconcileDecision::CancelExisting,
        TaskLineageRelationKind::CancelsTask,
        "forged user text",
    );
    assert!(
        store
            .database
            .unit_of_work(|tx| reconcile::apply(tx, turn.turn_task_id, &input, f.at))
            .is_err()
    );
    input.relations[0].later_excerpt.as_mut().expect("selector").exact_text =
        "Cancel that task".into();
    let cancelled = store
        .database
        .unit_of_work(|tx| reconcile::apply(tx, turn.turn_task_id, &input, f.at))
        .expect("cancelled");
    assert_eq!(cancelled.state, TaskLineageState::CancelledByUser);
    assert_eq!(cancelled.sources[0].user_event_id, turn.root_user_event_id);
    assert_eq!(
        turns::load(store.database.connection(), turn.turn_task_id).expect("turn").state,
        TurnTaskState::Cancelled
    );
    assert_eq!(
        lineage::load(store.database.connection(), old.lineage_id).expect("prior").state,
        TaskLineageState::InterruptedResumable
    );
    assert!(
        store
            .database
            .unit_of_work(|tx| {
                let mut resumed = cancelled.clone();
                resumed.state = TaskLineageState::Active;
                resumed.advance_revision(cancelled.revision, f.at).expect("revision");
                lineage::save(tx, &cancelled, &resumed)
            })
            .is_err()
    );
}
