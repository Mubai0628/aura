//! A turn stopped before it finished is sent to the model as it was - its messages and finished
//! steps - and then one note: where and why it stopped, and what of its last response had not
//! finished. Whatever the next message says, 继续 or anything else, the model reads it there.
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_model_domain::LogicalConversationItem;
use aura_persistence_core::StoreResult;
use aura_persistence_v2::{
    V2Store,
    agent::{lifecycle, model_context, recovery, turns},
    context,
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
        .expect("the next turn")
}

fn stop(store: &mut V2Store, turn: &TurnTask, reason: &str, cancelled: bool) -> TurnTask {
    store
        .database
        .unit_of_work(|tx| {
            recovery::interrupt(tx, turn.agent_run_id, reason, cancelled, turn.updated_at)?;
            turns::load(tx, turn.turn_task_id)
        })
        .expect("stopped")
}

/// What the next turn's model is sent of the conversation before its own message.
fn history(store: &V2Store, next: &TurnTask) -> Vec<LogicalConversationItem> {
    model_context::history_for_model(store.database.connection(), next.agent_run_id, true)
        .expect("history")
        .items
}

/// The last item, which for a stopped turn is its note.
fn note(items: &[LogicalConversationItem]) -> String {
    match items.last() {
        Some(LogicalConversationItem::Assistant { content: Some(text), tool_calls, .. })
            if tool_calls.is_empty() =>
        {
            text.clone()
        }
        other => panic!("the stopped turn ends with its note: {other:?}"),
    }
}

fn starts_with_its_message(items: &[LogicalConversationItem]) {
    assert!(
        matches!(&items[0], LogicalConversationItem::User { content, .. } if content == "Verify the requested work"),
        "{items:?}"
    );
}

#[test]
fn a_turn_cut_off_while_its_model_answered_ends_with_where_it_stopped() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let cut = stop(&mut store, &f.turn, "agent.core_restarted", false);
    assert!(matches!(cut.state, TurnTaskState::RecoveryRequired { .. }));
    let next = next_start(&mut store, &cut, "继续");

    let items = history(&store, &next);
    starts_with_its_message(&items);
    assert_eq!(items.len(), 2, "its message, then the note: {items:?}");
    let note = note(&items);
    assert!(note.contains("Aura restarted"), "{note}");
    assert!(note.contains("nothing else ran"), "{note}");
    // Every later request is sent the same note, so a cached prefix stays one.
    assert_eq!(history(&store, &next), items);
}

#[test]
fn a_turn_the_user_stopped_ends_with_that() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let stopped = stop(&mut store, &f.turn, "agent.cancelled", true);
    assert_eq!(stopped.state, TurnTaskState::Cancelled);
    let next = next_start(&mut store, &stopped, "Something else");

    let items = history(&store, &next);
    starts_with_its_message(&items);
    let note = note(&items);
    assert!(note.contains("The user stopped this turn"), "{note}");
    assert!(!note.contains("Aura restarted"), "{note}");
}

#[test]
fn a_cut_off_response_names_the_call_that_had_started_and_says_its_result_is_unknown() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let batch =
        f.admit(&mut store, vec![("computer.click", serde_json::json!({"app": "notepad"}))]);
    store
        .database
        .unit_of_work(|tx| f.begin_execution(tx, &batch, 0, None).map(|_| ()))
        .expect("the call started");
    let cut = stop(&mut store, &f.turn, "agent.core_restarted", false);
    assert!(matches!(cut.state, TurnTaskState::RecoveryRequired { .. }));
    let next = next_start(&mut store, &cut, "继续");

    let items = history(&store, &next);
    starts_with_its_message(&items);
    assert_eq!(items.len(), 2, "the unfinished response is in the note, not a call: {items:?}");
    let note = note(&items);
    let call = batch.batch.calls[0].tool_call_id.to_string();
    assert!(note.contains("computer.click"), "{note}");
    assert!(note.contains(&call), "{note}");
    assert!(note.contains("its result was lost"), "{note}");
    assert!(note.contains("check before doing it again"), "{note}");
}
