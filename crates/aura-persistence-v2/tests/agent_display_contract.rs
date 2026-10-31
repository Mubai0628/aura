//! The desktop display contract's assistant meter is assembled from Agent facts
//! and the Context domain's frozen usage. `AgentAssistantView` moved to
//! `aura_projection_contract`, so these cases pin the compatibility boundaries the
//! move must not change: a message written with a recorded inspection still shows
//! the frozen meter and the compaction state, and a message written before either
//! optional field existed still reads with them absent.
#![allow(clippy::unwrap_used)]
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;

use aura_agent_domain::*;
use aura_context_domain::{
    ContextCompactionStateView, ContextUsageKind, PromptInspectorProjection,
};
use aura_domain_common::*;
use aura_persistence_core::event;
use aura_persistence_v2::{
    V2Store,
    agent::{desktop, history, model_context},
};
use aura_projection_contract::AgentAssistantView;
use serde_json::{Value, json};

/// The real inspector document the context preparation produces for a session,
/// bound to this fixture's session and step. Only `session_id`,
/// `active_model_step_id`, `context_usage` and `compaction.state` are read back
/// through the desktop projection's frozen meter.
fn inspector(
    step: ModelStepId,
    session: SessionId,
    compaction: ContextCompactionStateView,
) -> PromptInspectorProjection {
    let prepared = aura_context_fixtures::p06_prepared_context_fixture().expect("P06 fixture");
    let mut projection = prepared.prepared.prompt_inspector;
    projection.session_id = session;
    projection.active_model_step_id = Some(step);
    projection.compaction.state = compaction;
    projection
}

fn record_inspector(
    store: &mut V2Store,
    step: ModelStepId,
    value: &PromptInspectorProjection,
    at: UtcTimestamp,
) {
    let payload = serde_json::to_value(value).expect("inspector payload");
    store
        .database
        .unit_of_work(|tx| {
            event::append(
                tx,
                &event::EventDraft {
                    id: EventId::new(),
                    owner: &step.to_string(),
                    kind: "agent.prompt_inspector",
                    version: 1,
                    payload: &payload,
                    at,
                },
                "agent",
            )
        })
        .expect("recorded inspection");
}

fn history_of(
    store: &V2Store,
    session: SessionId,
) -> aura_projection_contract::AgentSessionProjection {
    desktop::projection(
        store.database.connection(),
        &AgentHistoryRequest {
            session_id: session,
            before_turn_task_id: None,
            within_turn: None,
            limit: 10,
        },
        RuntimeInstanceId::new(),
    )
    .expect("history projection")
}

fn seal(store: &mut V2Store, f: &support::Fixture) {
    store
        .database
        .unit_of_work(|tx| {
            let mut output = f.template.clone();
            output.model_step_id = f.step.model_step_id;
            output.tool_calls.clear();
            output.refusal = None;
            history::record_output(tx, &output, None, f.at)
        })
        .expect("sealed output");
}

/// A recorded inspection reaches the UI through the moved type unchanged, and the
/// JSON keeps the same field names and omissions the renderer already reads.
#[test]
fn assistant_view_carries_the_frozen_context_meter_and_compaction_state() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    seal(&mut store, &f);
    let recorded =
        inspector(f.step.model_step_id, f.turn.session_id, ContextCompactionStateView::Running);
    let expected = recorded.context_usage.clone().expect("fixture usage");
    record_inspector(&mut store, f.step.model_step_id, &recorded, f.at);

    let view = history_of(&store, f.turn.session_id);
    let assistant = &view.turns[0].assistant_messages[0];
    assert_eq!(assistant.model_step_id, f.step.model_step_id);
    assert!(!assistant.partial);
    assert_eq!(assistant.context_usage.as_ref(), Some(&expected));
    assert_eq!(assistant.context_usage.as_ref().expect("meter").segments.len(), 4);
    assert!(assistant.context_usage.as_ref().expect("meter").is_consistent());
    assert!(assistant.context_compaction_running);
    assert!(assistant.sealed_revision.is_some());
    // The projection over a turn repeats the same message object.
    assert_eq!(&view.turns[0].assistant_messages[0], assistant);

    let wire: Value = serde_json::to_value(assistant).expect("assistant wire");
    assert_eq!(
        wire["context_usage"]["runtime_effective_window_tokens"],
        json!(expected.runtime_effective_window_tokens)
    );
    assert_eq!(wire["context_usage"]["used_input_tokens"], json!(expected.used_input_tokens));
    assert_eq!(wire["context_usage"]["estimated"], json!(expected.estimated));
    assert_eq!(
        wire["context_usage"]["segments"].as_array().expect("segments").len(),
        ContextUsageKind::ORDER.len()
    );
    assert_eq!(wire["context_compaction_running"], json!(true));
    assert!(wire.get("sealed_revision").is_some());
}

/// The same reading when the compaction state was not running: the fact is still
/// read, and the flag is omitted exactly as `skip_serializing_if = "is_false"` says.
#[test]
fn settled_compaction_omits_the_running_flag_without_dropping_the_meter() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    seal(&mut store, &f);
    let recorded =
        inspector(f.step.model_step_id, f.turn.session_id, ContextCompactionStateView::Completed);
    let expected = recorded.context_usage.clone().expect("fixture usage");
    record_inspector(&mut store, f.step.model_step_id, &recorded, f.at);

    let view = history_of(&store, f.turn.session_id);
    let assistant = &view.turns[0].assistant_messages[0];
    assert_eq!(assistant.context_usage.as_ref(), Some(&expected));
    assert!(!assistant.context_compaction_running);
    let wire: Value = serde_json::to_value(assistant).expect("assistant wire");
    assert!(wire.get("context_usage").is_some());
    assert!(wire.get("context_compaction_running").is_none());
}

/// A message written before either optional field existed still reads: the two
/// fields are absent, and every other field keeps its actual value.
#[test]
fn legacy_message_without_the_optional_fields_still_reads() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    seal(&mut store, &f);

    let view = history_of(&store, f.turn.session_id);
    let assistant = &view.turns[0].assistant_messages[0];
    assert!(assistant.context_usage.is_none());
    assert!(!assistant.context_compaction_running);
    assert_eq!(assistant.model_step_id, f.step.model_step_id);
    assert!(assistant.text_total_bytes > 0);
    let wire: Value = serde_json::to_value(assistant).expect("assistant wire");
    assert!(wire.get("context_usage").is_none());
    assert!(wire.get("context_compaction_running").is_none());
    assert!(wire.get("stream_body_id").is_some());

    // A stored document from before the optional fields existed — the shape an old
    // client wrote, and the shape this reader must still accept — decodes with the
    // same defaults instead of failing on the unknown/missing field.
    let legacy = json!({
        "model_step_id": f.step.model_step_id,
        "model_profile_id": null,
        "context_snapshot_id": null,
        "text": "old answer",
        "reasoning": "",
        "text_total_bytes": 10,
        "reasoning_total_bytes": 0,
        "partial": false,
        "stream_sequence": null,
        "tool_call_ids": [],
        "at": f.at,
    });
    let decoded: AgentAssistantView =
        serde_json::from_value(legacy).expect("legacy assistant view");
    assert!(decoded.context_usage.is_none());
    assert!(!decoded.context_compaction_running);
    assert!(decoded.sealed_revision.is_none());
    assert!(decoded.stream_body_id.is_none());
}

/// The moved type is still the display contract's owner: a page of assistant
/// messages from a real store decodes into it, and the desktop route's own reply
/// type is the same single definition.
#[test]
fn projection_owns_the_single_assistant_view_definition() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    seal(&mut store, &f);
    let steps: Vec<_> = model_context::steps(store.database.connection(), f.turn.agent_run_id)
        .expect("steps")
        .into_iter()
        .map(|step| step.model_step_id)
        .collect();
    assert_eq!(steps, vec![f.step.model_step_id]);
    let view: aura_projection_contract::AgentTurnView = serde_json::from_value(
        serde_json::to_value(&history_of(&store, f.turn.session_id).turns[0]).expect("turn wire"),
    )
    .expect("turn decodes into the display contract type");
    assert_eq!(view.assistant_messages.len(), 1);
    let _: &AgentAssistantView = &view.assistant_messages[0];
}

/// While the context is compacted before a step, the step says so instead of looking like the
/// model thinking: since when the part running now began, and how many parts are done.
#[test]
fn a_step_waiting_on_a_compaction_says_since_when_and_how_many_parts_are_done() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::preflight(&mut store, AgentTaskMode::Normal);
    let insert = |store: &mut V2Store, state: &str, at: &str| {
        let payload = json!({"source_step_id": f.step.model_step_id, "created_at": at}).to_string();
        let id = ContextProjectionSnapshotId::new().to_string();
        store
            .database
            .unit_of_work(|tx| -> rusqlite::Result<usize> {
                tx.execute(
                    "INSERT INTO context_compactions VALUES(?1,?2,?3,1,?4)",
                    [id, f.turn.session_id.to_string(), state.to_owned(), payload],
                )
            })
            .expect("compaction row");
    };
    insert(&mut store, "completed", "2026-09-07T00:00:01Z");
    insert(&mut store, "running", "2026-09-07T00:01:00Z");
    let view = history_of(&store, f.turn.session_id);
    let assistant = view.turns[0].assistant_messages.last().expect("the step being prepared");
    assert!(assistant.partial);
    let compacting = assistant.context_compacting.as_ref().expect("the compaction under way");
    assert_eq!(compacting.since, "2026-09-07T00:01:00Z".parse().unwrap());
    assert_eq!(compacting.finished_parts, 1);
    let wire: Value = serde_json::to_value(assistant).expect("assistant wire");
    assert_eq!(wire["context_compacting"]["finished_parts"], json!(1));

    store
        .database
        .unit_of_work(|tx| -> rusqlite::Result<usize> {
            tx.execute("UPDATE context_compactions SET state='completed' WHERE state='running'", [])
        })
        .expect("compaction finished");
    let view = history_of(&store, f.turn.session_id);
    let assistant = view.turns[0].assistant_messages.last().expect("the step being prepared");
    assert!(assistant.context_compacting.is_none());
    let wire: Value = serde_json::to_value(assistant).expect("assistant wire");
    assert!(wire.get("context_compacting").is_none());
}

/// Once the compaction before a step has ended, the step's wait for the model counts from then:
/// the latest end among the attempts of the step's finished compactions. A compaction still
/// running is shown as such and does not count.
#[test]
fn a_step_after_its_compaction_says_when_the_compaction_ended() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::preflight(&mut store, AgentTaskMode::Normal);
    let insert = |store: &mut V2Store, state: &str, ends: &[&str]| {
        let attempts = ends.iter().map(|at| json!({"terminal_at": at})).collect::<Vec<_>>();
        let payload = json!({"source_step_id": f.step.model_step_id,
            "created_at": "2026-10-31T00:00:00Z", "attempts": attempts})
        .to_string();
        let id = ContextProjectionSnapshotId::new().to_string();
        store
            .database
            .unit_of_work(|tx| -> rusqlite::Result<usize> {
                tx.execute(
                    "INSERT INTO context_compactions VALUES(?1,?2,?3,1,?4)",
                    [id, f.turn.session_id.to_string(), state.to_owned(), payload],
                )
            })
            .expect("compaction row");
    };
    let latest = |store: &V2Store| {
        let view = history_of(store, f.turn.session_id);
        view.turns[0].assistant_messages.last().expect("the step").compacted_until
    };
    assert_eq!(latest(&store), None, "no compaction yet");
    insert(&mut store, "failed", &["2026-10-31T00:01:00Z"]);
    insert(&mut store, "completed", &["2026-10-31T00:02:00Z", "2026-10-31T00:03:30Z"]);
    insert(&mut store, "running", &["2026-10-31T00:09:00Z"]);
    assert_eq!(latest(&store), Some("2026-10-31T00:03:30Z".parse().unwrap()));
    let view = history_of(&store, f.turn.session_id);
    let wire: Value =
        serde_json::to_value(view.turns[0].assistant_messages.last().expect("the step"))
            .expect("assistant wire");
    assert!(wire.get("compacted_until").is_some());
}
