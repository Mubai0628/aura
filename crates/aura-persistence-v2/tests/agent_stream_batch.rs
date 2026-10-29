//! Deltas that arrive together are stored as one: the text and reasoning they carry, in order,
//! under the last sequence among them. A stream read after it is the same stream.
#![allow(clippy::unwrap_used)]
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;

use aura_agent_domain::*;
use aura_domain_common::*;
use aura_model_domain::*;
use aura_persistence_v2::{
    V2Store,
    agent::{desktop_messages, stream},
    model,
};

fn setup() -> (tempfile::TempDir, V2Store, support::Fixture, TransportAttemptId) {
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let attempt = attempt(&mut store, &fixture);
    (root, store, fixture, attempt)
}

/// A new transport attempt of the fixture's step; the newest one is the one its window shows.
fn attempt(store: &mut V2Store, fixture: &support::Fixture) -> TransportAttemptId {
    let snapshot =
        model::step_snapshot(store.database.connection(), fixture.step.model_step_id).unwrap();
    let mut attempt =
        aura_contract_schema::canonical_fixture().unwrap().models.transport_attempt.unwrap();
    attempt.id = TransportAttemptId::new();
    store.database.unit_of_work(|tx| model::insert_transport(tx, snapshot.id, &attempt)).unwrap();
    attempt.id
}

fn envelope(
    fixture: &support::Fixture,
    attempt: TransportAttemptId,
    sequence: u64,
    event: ModelStreamEvent,
) -> ModelStreamEventEnvelope {
    ModelStreamEventEnvelope {
        model_step_id: fixture.step.model_step_id,
        transport_attempt_id: attempt,
        sequence,
        received_at: fixture.at,
        event,
    }
}

fn text(text: &str) -> ModelStreamEvent {
    ModelStreamEvent::AssistantTextDelta { block_id: ModelContentBlockId::new(), text: text.into() }
}

fn reasoning(text: &str) -> ModelStreamEvent {
    ModelStreamEvent::ReasoningDelta { block_id: ModelContentBlockId::new(), text: text.into() }
}

fn delta_events(store: &V2Store, step: ModelStepId) -> i64 {
    store
        .database
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE owner=?1 AND kind='agent.visible_delta'",
            [step.to_string()],
            |row| row.get(0),
        )
        .unwrap()
}

fn page(store: &V2Store, fixture: &support::Fixture) -> String {
    desktop_messages::text(
        store.database.connection(),
        &AgentMessageTextRequest {
            session_id: fixture.turn.session_id,
            source: AgentMessageTextSource::Assistant { model_step_id: fixture.step.model_step_id },
            offset: 0,
            limit: 65_536,
        },
    )
    .unwrap()
    .text
}

#[test]
fn deltas_that_arrive_together_are_stored_as_one() {
    let (_root, mut store, fixture, attempt) = setup();
    let mut batch = Vec::new();
    let (mut answer, mut thought) = (String::new(), String::new());
    for n in 0..12_u64 {
        let piece = format!("t{n} ");
        answer.push_str(&piece);
        batch.push(envelope(&fixture, attempt, 3 * n + 1, text(&piece)));
        let piece = format!("r{n} ");
        thought.push_str(&piece);
        batch.push(envelope(&fixture, attempt, 3 * n + 2, reasoning(&piece)));
        // A call's growing input between them is not stored and does not split the batch.
        batch.push(envelope(
            &fixture,
            attempt,
            3 * n + 3,
            ModelStreamEvent::ToolCallArgumentsDelta {
                provider_call_id: "call_0".parse().unwrap(),
                delta: "{\"path\":".into(),
            },
        ));
    }
    store.database.unit_of_work(|tx| stream::append_all(tx, &batch)).unwrap();
    assert_eq!(delta_events(&store, fixture.step.model_step_id), 1, "one delta for the batch");
    let body = stream::partial(store.database.connection(), fixture.step.model_step_id).unwrap();
    assert_eq!((body.text.as_str(), body.reasoning.as_str()), (answer.as_str(), thought.as_str()));
    assert_eq!(body.sequence, Some(35), "the stored delta carries the batch's last sequence");

    // The next delta continues the stream as usual.
    let next = envelope(&fixture, attempt, 36, text("next."));
    store.database.unit_of_work(|tx| stream::append(tx, &next)).unwrap();
    answer.push_str("next.");
    assert_eq!(page(&store, &fixture), answer);
    assert_eq!(delta_events(&store, fixture.step.model_step_id), 2);
}

#[test]
fn a_batch_that_reaches_a_new_attempt_keeps_only_the_new_attempts_text() {
    let (_root, mut store, fixture, first) = setup();
    for n in 1..=3 {
        let event = envelope(&fixture, first, n, text("old "));
        store.database.unit_of_work(|tx| stream::append(tx, &event)).unwrap();
    }
    let second = attempt(&mut store, &fixture);
    let batch = [
        envelope(&fixture, first, 4, text("late old ")),
        envelope(&fixture, first, 5, text("text")),
        envelope(&fixture, second, 1, text("new ")),
        envelope(&fixture, second, 2, text("answer")),
    ];
    store.database.unit_of_work(|tx| stream::append_all(tx, &batch)).unwrap();
    let body = stream::partial(store.database.connection(), fixture.step.model_step_id).unwrap();
    assert_eq!(body.text, "new answer");
    assert_eq!(body.body_id, Some(second.to_string()));
}
