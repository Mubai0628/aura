use super::{CONTROL_STATE, prompt_state, write_control_state};
use aura_agent_domain::{TodoItem, TodoState, TodoStatus};
use aura_context_domain::{
    ContextCompressibility, ContextPrivacyClass, ContextSectionKind, ContextSourceRef,
    ModelEgressAuthorizationBasis, ModelEgressSourceKind,
};
use aura_context_runtime::{ContextFragment, ContextSectionInput};
use aura_domain_common::{ContentHash, EventId, Revision, TodoItemId, ToolCallId, TurnTaskId};
use aura_model_domain::LogicalConversationItem;
use aura_security_contract::NetworkDataClass;
use aura_tool_contract::{ModelVisibleToolResultOutcomeV1, ModelVisibleToolSuccessPayloadV1};

fn todo(turn: TurnTaskId, revision: u64, titles: &[&str]) -> TodoState {
    TodoState {
        turn_task_id: turn,
        revision: Revision::new(revision),
        items: titles
            .iter()
            .map(|title| TodoItem {
                id: TodoItemId::new(),
                title: (*title).to_owned(),
                status: TodoStatus::Pending,
            })
            .collect(),
    }
}

fn state(todo: &TodoState) -> serde_json::Value {
    serde_json::json!({"turn": {"revision": "3"}, "todo": todo, "permission_mode": "read_only"})
}

fn result(tool: &str, content: String, is_error: bool) -> LogicalConversationItem {
    LogicalConversationItem::ToolResult {
        source_event_id: EventId::new(),
        tool_call_id: ToolCallId::new(),
        tool_name: tool.to_owned(),
        content_hash: ContentHash::sha256(content.as_bytes()),
        content,
        is_error,
    }
}

/// A to-do tool's result as the model reads it.
fn shows(tool: &str, todo: &TodoState) -> LogicalConversationItem {
    let outcome = ModelVisibleToolResultOutcomeV1::Succeeded {
        payload: ModelVisibleToolSuccessPayloadV1::StructuredJson {
            value: serde_json::to_value(todo).expect("a to-do"),
        },
    };
    let view = serde_json::json!({"outcome": outcome, "side_effect_state": "committed"});
    result(tool, view.to_string(), false)
}

fn carries_todo(state: &serde_json::Value, conversation: &[LogicalConversationItem]) -> bool {
    prompt_state(state, conversation).get("todo").is_some()
}

#[test]
fn the_model_reads_no_per_step_bookkeeping_and_core_keeps_it() {
    let state = serde_json::json!({
        "turn": {"revision": "3"}, "run": {"revision": "9"}, "state_revision": "12",
        "state_hash": "h", "registry_snapshot_ref": {}, "continuation": {"trigger": "tool_batch"},
        "todo": null, "permission_mode": "read_only"
    });
    assert_eq!(
        prompt_state(&state, &[]),
        serde_json::json!({"turn": {"revision": "3"}, "todo": null, "permission_mode": "read_only"})
    );
    assert_eq!(state["state_hash"], "h", "the commit check still has what it verifies");
}

#[test]
fn a_todo_no_update_has_written_is_left_out() {
    let turn = TurnTaskId::new();
    let state = state(&todo(turn, 1, &[]));
    assert_eq!(
        prompt_state(&state, &[]),
        serde_json::json!({"turn": {"revision": "3"}, "permission_mode": "read_only"})
    );
}

#[test]
fn the_todo_the_newest_result_shows_is_left_out_until_it_changes() {
    let turn = TurnTaskId::new();
    let current = todo(turn, 2, &["first", "second"]);
    let conversation = [shows("todo.replace", &current)];
    assert!(!carries_todo(&state(&current), &conversation));
    // The same items at a later revision: an update the conversation does not show.
    let later = TodoState { revision: Revision::new(3), ..current.clone() };
    assert!(carries_todo(&state(&later), &conversation));
    // A newer result shows the later one; the older result no longer decides.
    let conversation = [shows("todo.replace", &current), shows("todo.update", &later)];
    assert!(!carries_todo(&state(&later), &conversation));
    assert!(carries_todo(&state(&current), &conversation));
}

#[test]
fn a_cleared_result_shows_nothing_and_a_failed_one_is_passed_over() {
    let turn = TurnTaskId::new();
    let earlier = todo(turn, 2, &["first"]);
    let current = todo(turn, 3, &["first", "second"]);
    let cleared = result("todo.update", "[cleared: todo.update result]".to_owned(), false);
    let conversation = [shows("todo.replace", &earlier), cleared];
    assert!(carries_todo(&state(&current), &conversation), "the result that showed it is gone");
    let failed = result("todo.update", r#"{"outcome":{"type":"failed"}}"#.to_owned(), true);
    let conversation = [shows("todo.replace", &current), failed];
    assert!(!carries_todo(&state(&current), &conversation), "the conflict changed nothing");
}

/// The control fragment, and only it, is written from the history the request now carries: once a
/// clearing or a compaction took the result that showed the to-do, the state carries it again.
#[test]
fn the_control_state_follows_the_history_the_request_carries() {
    let mut input = aura_context_fixtures::p06_performance_preparation_input(128_000, 20)
        .expect("a preparation input");
    let current = todo(input.request.turn_task_id, 2, &["first"]);
    let fragment = |state_kind: &str| ContextFragment {
        source_ref: ContextSourceRef::RuntimeState {
            state_kind: state_kind.to_owned(),
            state_revision: Revision::INITIAL,
        },
        source_kind: ModelEgressSourceKind::AuraCore,
        data_class: NetworkDataClass::PublicContent,
        authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
        content: "as assembled".to_owned(),
    };
    input.section_inputs.push(ContextSectionInput {
        kind: ContextSectionKind::CurrentTaskState,
        fragments: vec![fragment(CONTROL_STATE), fragment("agent.activity.v1")],
        compressibility: ContextCompressibility::NeverCompress,
        privacy_class: ContextPrivacyClass::ModelVisible,
    });
    let written = |input: &aura_context_runtime::ContextPreparationInput| {
        let section = input.section_inputs.last().expect("the task state section");
        (section.fragments[0].content.clone(), section.fragments[1].content.clone())
    };
    input.recent_history_items = vec![shows("todo.replace", &current)];
    write_control_state(&mut input, &state(&current)).expect("written");
    let (control, other) = written(&input);
    assert!(!control.contains("todo") && control.contains("read_only"), "{control}");
    assert_eq!(other, "as assembled", "another fragment of the section is left as it was");
    input.recent_history_items = vec![result("todo.replace", "[cleared]".to_owned(), false)];
    write_control_state(&mut input, &state(&current)).expect("written");
    let (control, _) = written(&input);
    assert!(control.contains(r#""todo":{"#) && control.contains("first"), "{control}");
}

#[test]
fn another_turns_todo_or_an_emptied_one_out_of_sight_is_carried() {
    let earlier_turn = TurnTaskId::new();
    let turn = TurnTaskId::new();
    let conversation = [shows("todo.replace", &todo(earlier_turn, 2, &["first"]))];
    assert!(carries_todo(&state(&todo(turn, 2, &["first"])), &conversation));
    // Emptied by an update the conversation no longer shows: its revision is still needed.
    assert!(carries_todo(&state(&todo(turn, 4, &[])), &[]));
    // A tool that is not a to-do tool shows no to-do, whatever it returns.
    let current = todo(turn, 2, &["first"]);
    assert!(carries_todo(&state(&current), &[shows("memory.read", &current)]));
}
