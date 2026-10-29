//! A reply's streamed deltas are what its window shows while it is written. Once the reply is
//! sealed the window reads the sealed text and nothing reads the deltas again, so they go with the
//! seal. A reply that failed or was stopped keeps them: they hold the only copy of what it showed.
#![allow(clippy::unwrap_used)]
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;

use aura_agent_domain::*;
use aura_domain_common::*;
use aura_model_domain::*;
use aura_persistence_v2::{
    V2Store,
    agent::{desktop, desktop_messages, history, stream, turns},
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

/// Streams `count` deltas one at a time, as each used to be stored; returns the text they make.
fn stream_one_by_one(
    store: &mut V2Store,
    fixture: &support::Fixture,
    attempt: TransportAttemptId,
    count: u64,
) -> String {
    let mut whole = String::new();
    for n in 1..=count {
        let piece = format!("piece {n}. ");
        let event = envelope(fixture, attempt, n, text(&piece));
        store.database.unit_of_work(|tx| stream::append(tx, &event)).unwrap();
        whole.push_str(&piece);
    }
    whole
}

/// The rows the step's stream left: its delta events of all three kinds and the derived rows.
fn streamed(store: &V2Store, step: ModelStepId) -> i64 {
    let db = store.database.connection();
    let owner = step.to_string();
    let events: i64 = db
        .query_row(
            "SELECT count(*) FROM events WHERE owner=?1 AND kind IN \
             ('agent.visible_delta','agent.visible_delta_index','agent.visible_delta_epoch')",
            [&owner],
            |row| row.get(0),
        )
        .unwrap();
    let derived: i64 = ["agent_visible_delta", "agent_visible_index", "agent_visible_coverage"]
        .iter()
        .map(|table| {
            db.query_row(&format!("SELECT count(*) FROM {table} WHERE owner=?1"), [&owner], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
        })
        .sum();
    events + derived
}

fn seal(store: &mut V2Store, fixture: &support::Fixture, sealed: &str) {
    let mut output = fixture.template.clone();
    output.model_step_id = fixture.step.model_step_id;
    output.tool_calls.clear();
    output.assistant_blocks = vec![AssistantContentBlock {
        block_id: ModelContentBlockId::new(),
        text: sealed.into(),
        provider_reference: None,
    }];
    store
        .database
        .unit_of_work(|tx| history::record_output(tx, &output, None, fixture.at))
        .unwrap();
}

fn shown(store: &V2Store, fixture: &support::Fixture) -> (String, bool) {
    let view = desktop::projection(
        store.database.connection(),
        &AgentHistoryRequest {
            within_turn: None,
            session_id: fixture.turn.session_id,
            before_turn_task_id: None,
            limit: 5,
        },
        RuntimeInstanceId::new(),
    )
    .unwrap();
    let message = &view.turns[0].assistant_messages[0];
    (message.text.clone(), message.partial)
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

fn event_delete_guard(store: &V2Store) -> i64 {
    store
        .database
        .connection()
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='trigger' \
             AND name='immutable_event_delete'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn a_completed_reply_drops_its_streamed_deltas_and_still_reads_whole() {
    let (_root, mut store, fixture, attempt) = setup();
    let whole = stream_one_by_one(&mut store, &fixture, attempt, 40);
    assert_eq!(page(&store, &fixture), whole, "the running reply reads from its deltas");
    assert!(streamed(&store, fixture.step.model_step_id) > 0);

    seal(&mut store, &fixture, &whole);

    assert_eq!(streamed(&store, fixture.step.model_step_id), 0, "the seal drops the deltas");
    assert_eq!(shown(&store, &fixture), (whole.clone(), false));
    assert_eq!(page(&store, &fixture), whole, "the sealed reply reads whole");
    // The guard was lifted for the seal only: every other delete of an event is still refused.
    assert_eq!(event_delete_guard(&store), 1);
    assert!(store.database.connection().execute("DELETE FROM events", []).is_err());
}

#[test]
fn a_stopped_reply_keeps_what_it_streamed() {
    let (_root, mut store, fixture, attempt) = setup();
    let whole = stream_one_by_one(&mut store, &fixture, attempt, 25);
    store
        .database
        .unit_of_work(|tx| {
            let previous = turns::step(tx, fixture.step.model_step_id)?;
            let mut next = previous.clone();
            next.state = ModelStepState::Cancelled;
            next.finished_at = Some(fixture.at);
            turns::save_step(tx, &previous, &next)
        })
        .unwrap();
    assert!(streamed(&store, fixture.step.model_step_id) > 0, "a stopped reply keeps its deltas");
    assert_eq!(shown(&store, &fixture), (whole.clone(), true));
    assert_eq!(page(&store, &fixture), whole);
}

/// An older version wrote an outbox copy of every event, which points at its event and has no index
/// on that column: deleting an event would read the whole outbox. While a copy is left the seal
/// keeps the deltas; the slimming that clears the outbox drops them after.
#[test]
fn a_seal_keeps_the_deltas_while_an_older_versions_outbox_copy_is_left() {
    let (_root, mut store, fixture, attempt) = setup();
    let whole = stream_one_by_one(&mut store, &fixture, attempt, 10);
    store
        .database
        .connection()
        .execute(
            "INSERT INTO projection_outbox(event_id,topic,revision,payload) \
             SELECT event_id,'old',sequence,payload FROM events",
            [],
        )
        .unwrap();
    seal(&mut store, &fixture, &whole);
    let deltas: i64 = store
        .database
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE owner=?1 AND kind='agent.visible_delta'",
            [fixture.step.model_step_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(deltas, 10, "the deltas stay");
    assert_eq!(shown(&store, &fixture), (whole, false));
}
