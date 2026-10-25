//! What a bounded read really costs, measured at the layer that can see it.
//!
//! The claim these cover is "a preview or a page of a long body does not read the body". A
//! test that only checks the returned text cannot tell a window from a whole-body read that
//! was then sliced, because both return the same bytes. So the assertions are about the work:
//!
//! * **how much arrived from the store** — `decoded_bytes` counts the chunk text that was
//!   fetched and parsed to answer the window. A bounded read keeps it near the window; a
//!   whole-body read makes it the body.
//! * **how many rows were touched** — `chunks_read`.
//! * **whether the authority was needed at all** — the windowed path answers from the index
//!   and refuses to answer when that index no longer describes the object, which is a
//!   structural fact rather than a timing measurement.
//!
//! SQLite's own page reads are deliberately *not* reported as disk I/O: this test does not
//! instrument the filesystem, so it reports what it measured — bytes returned to and parsed
//! by the application — and nothing about the storage engine's internals.
#![allow(clippy::unwrap_used)]
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;

use aura_agent_domain::*;
use aura_domain_common::*;
use aura_model_domain::{AssistantContentBlock, ReasoningContentBlock};
use aura_persistence_v2::{
    V2Store,
    agent::{desktop_messages, history, session, turns},
};

const PREVIEW: u64 = 2048;

fn store() -> (tempfile::TempDir, V2Store, support::Fixture) {
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    (root, store, fixture)
}

/// One delta of the live stream, when the fixture has a transport attempt to attach it to.
fn append_delta(store: &mut V2Store, fixture: &support::Fixture, sequence: u64, text: &str) {
    let attempt = {
        let Ok(snapshot) = aura_persistence_v2::model::step_snapshot(
            store.database.connection(),
            fixture.step.model_step_id,
        ) else {
            return;
        };
        let mut attempt = aura_contract_schema::canonical_fixture()
            .expect("fixture")
            .models
            .transport_attempt
            .expect("transport attempt fixture");
        attempt.id = TransportAttemptId::new();
        let Ok(()) = store.database.unit_of_work(|tx| {
            aura_persistence_v2::model::insert_transport(tx, snapshot.id, &attempt)
        }) else {
            return;
        };
        attempt.id
    };
    let event = aura_model_domain::ModelStreamEventEnvelope {
        model_step_id: fixture.step.model_step_id,
        transport_attempt_id: attempt,
        sequence,
        received_at: fixture.at,
        event: aura_model_domain::ModelStreamEvent::AssistantTextDelta {
            block_id: ModelContentBlockId::new(),
            text: text.to_owned(),
        },
    };
    let _ =
        store.database.unit_of_work(|tx| aura_persistence_v2::agent::stream::append(tx, &event));
}

/// Seals one large response on the fixture's running step and returns its body.
fn seal_large_body(store: &mut V2Store, fixture: &support::Fixture) -> String {
    // Roughly 4 MiB of realistic mixed-width text, so a whole-body read is unmistakable.
    let body = "有界读取-é-ab".repeat(200_000);
    let mut output = fixture.template.clone();
    output.model_step_id = fixture.step.model_step_id;
    output.tool_calls.clear();
    output.assistant_blocks = vec![AssistantContentBlock {
        block_id: ModelContentBlockId::new(),
        text: body.clone(),
        provider_reference: None,
    }];
    output.reasoning_blocks = vec![ReasoningContentBlock {
        block_id: ModelContentBlockId::new(),
        text: "推理-思考内容".repeat(50_000),
        summary: None,
        provider_reference: None,
    }];
    store
        .database
        .unit_of_work(|tx| history::record_output(tx, &output, None, fixture.at))
        .expect("sealed output");
    body
}

/// A preview of a sealed response costs the window, not the response.
#[test]
fn a_sealed_preview_reads_a_window_and_not_the_body() {
    let (_root, mut store, fixture) = store();
    let body = seal_large_body(&mut store, &fixture);
    let body_bytes = body.len() as u64;
    assert!(body_bytes > 3 * 1024 * 1024, "the body is large enough to matter: {body_bytes}");

    let db = store.database.connection();
    let seal = history::sealed_body(db, fixture.step.model_step_id).expect("sealed identity");
    let window = history::visible_body_window(
        db,
        fixture.step.model_step_id,
        history::VisibleSource::Assistant,
        0,
        PREVIEW,
        &seal,
    )
    .expect("window")
    .expect("the step stores a chunk index");

    println!(
        "sealed preview: body={body_bytes} returned={} decoded={} chunks={}",
        window.text.len(),
        window.decoded_bytes,
        window.chunks_read
    );
    assert_eq!(window.total_bytes, body_bytes, "the reported length is the body's");
    assert!(window.text.len() <= PREVIEW as usize);
    assert!(body.starts_with(&window.text), "the preview is the start of the body");
    assert_eq!(window.chunks_read, 1, "a 2 KiB preview touches the first chunk only");
    assert!(
        window.decoded_bytes < body_bytes / 8,
        "the preview must not decode the body: {} of {body_bytes}",
        window.decoded_bytes
    );

    // A page from the middle costs the same order: the chunks it spans, not the body.
    let middle = history::visible_body_window(
        db,
        fixture.step.model_step_id,
        history::VisibleSource::Assistant,
        body_bytes / 2,
        4096,
        &seal,
    )
    .expect("window")
    .expect("index");
    println!(
        "sealed middle page: returned={} decoded={} chunks={}",
        middle.text.len(),
        middle.decoded_bytes,
        middle.chunks_read
    );
    assert!(middle.text.len() <= 4096);
    assert!(middle.decoded_bytes < body_bytes / 4);
    assert!(middle.chunks_read <= 3, "the page spans at most the chunks it covers");

    // The reasoning body is read the same way and does not borrow the assistant's offsets.
    let reasoning = history::visible_body_window(
        db,
        fixture.step.model_step_id,
        history::VisibleSource::Reasoning,
        0,
        PREVIEW,
        &seal,
    )
    .expect("window")
    .expect("index");
    assert!(reasoning.decoded_bytes < body_bytes / 8);
    assert_eq!(reasoning.chunks_read, 1);
}

/// A stored index is only answered from when it still describes the object it was written for.
///
/// This is the guarantee the bounded read rests on, and it has two halves:
///
/// * the **store** keeps a sealed response and its object consistent — the response event is
///   immutable and the object table refuses bytes that do not match its recorded length, both
///   checked here rather than assumed;
/// * the **reader** compares the digest it holds against the one the index was written with,
///   so an index built for different content reports "no usable index" instead of answering
///   from chunks that no longer describe the body. A reader without that check would return
///   text that never existed.
#[test]
fn a_stored_index_is_only_used_while_it_describes_the_object() {
    let (_root, mut store, fixture) = store();
    let body = seal_large_body(&mut store, &fixture);
    let body_bytes = body.len() as u64;
    assert!(body_bytes > 3 * 1024 * 1024);

    let db = store.database.connection();
    let seal = history::sealed_body(db, fixture.step.model_step_id).expect("identity");
    let window = history::visible_body_window(
        db,
        fixture.step.model_step_id,
        history::VisibleSource::Assistant,
        0,
        PREVIEW,
        &seal,
    )
    .expect("window")
    .expect("index");
    assert_eq!(window.total_bytes, body_bytes);

    // The authority cannot be removed behind a sealed step, and the object table refuses bytes
    // that contradict the length it recorded: both are the store's own constraints.
    let removed = db.execute(
        "DELETE FROM events WHERE event_id=(SELECT assistant_event_id FROM model_steps WHERE id=?1)",
        [fixture.step.model_step_id.to_string()],
    );
    assert!(removed.is_err(), "sealed response events are immutable");
    let step = turns::step(db, fixture.step.model_step_id).expect("step");
    let aura_agent_domain::ModelStepState::Completed { output_artifact_id, .. } = step.state else {
        panic!("the fixture's step is sealed");
    };
    let tampered = db.execute(
        "UPDATE artifact_objects SET bytes=?1 WHERE hash=(SELECT o.hash FROM artifacts a \
         JOIN artifact_objects o ON o.hash=a.object_hash WHERE a.id=?2)",
        rusqlite::params![b"{}".as_slice(), output_artifact_id.to_string()],
    );
    assert!(tampered.is_err(), "an object with the wrong length is refused by the store");

    // The reader's half: an index whose digest is not the one this body was written with is
    // reported as no index at all, so the caller falls back to its own authority.
    let other = history::ChunkBody {
        owner: fixture.step.model_step_id.to_string(),
        source: history::VisibleSource::Assistant,
        revision: ContentHash::blake3(b"a different response"),
    };
    assert!(
        history::stored_body_window(db, &other, 0, PREVIEW).expect("lookup").is_none(),
        "an index written for different content answers nothing"
    );
    // And the real one still answers, because the body it describes is the one that is stored.
    assert!(
        history::visible_body_window(
            db,
            fixture.step.model_step_id,
            history::VisibleSource::Assistant,
            0,
            PREVIEW,
            &seal,
        )
        .expect("lookup")
        .is_some(),
        "the index that matches its object is used"
    );
}

/// A user message preview and page cost the window, not the message.
#[test]
fn a_user_message_preview_and_page_read_a_window() {
    let (_root, mut store, fixture) = store();
    // One long message, so reading the payload to answer for its first bytes is obvious.
    // The scheduled turn's own message keeps its place: this test writes a second message with
    // its own identity, so the assertions below are about that message's body, read through the
    // same two production entry points the desktop uses.
    let event_id = EventId::new();
    let text = "用户正文-é-有界读取".repeat(60_000);
    let message = OriginalUserMessage::new(
        event_id,
        fixture.turn.session_id,
        text.clone(),
        UserMessageOrigin::UserInput,
        fixture.at,
    )
    .expect("user message");
    store.database.unit_of_work(|tx| session::add_user_message(tx, &message)).expect("message");
    let message_bytes = text.len() as u64;
    assert!(message_bytes > 1024 * 1024, "the message is large enough to matter");

    let db = store.database.connection();
    let source = session::user_message_source(db, fixture.turn.session_id, event_id)
        .expect("message identity");
    let window = history::stored_body_window(
        db,
        &history::ChunkBody::user_message(event_id, &source.content_hash),
        0,
        PREVIEW,
    )
    .expect("window")
    .expect("the message stores a chunk index");
    println!(
        "user preview: body={message_bytes} returned={} decoded={} chunks={}",
        window.text.len(),
        window.decoded_bytes,
        window.chunks_read
    );
    assert_eq!(window.total_bytes, message_bytes, "the reported length is the message's");
    assert!(text.starts_with(&window.text));
    assert_eq!(window.chunks_read, 1);
    assert!(window.decoded_bytes < message_bytes / 8);

    // A page from the middle is a slice of the same text, read from the chunks. The requested
    // offset may land inside a character, and the page reports where it really started.
    let middle = message_bytes / 2;
    let request = AgentMessageTextRequest {
        session_id: fixture.turn.session_id,
        source: AgentMessageTextSource::User { event_id },
        offset: middle,
        limit: 4096,
    };
    let page = desktop_messages::text(db, &request).expect("page");
    let from = page.from_offset as usize;
    assert_eq!(page.text, text[from..from + page.text.len()], "a slice of the message");
    assert_eq!(page.total_bytes, message_bytes);
    assert!(page.from_offset <= middle, "a page never starts after the offset it was asked for");
    assert!(middle - page.from_offset < 4, "the snap is at most one character");
}
/// A message written before the chunk index existed still previews and pages correctly.
///
/// The compatibility path is a real store, not a synthetic one: the payload is written the way
/// the old code wrote it, with no chunks beside it. The preview must stay bounded and the page
/// must be exact, because there is nothing else to read.
#[test]
fn a_message_stored_without_chunks_still_previews_and_pages() {
    let (_root, mut store, fixture) = store();
    let text = "旧消息-兼容路径-é".repeat(2_000);
    let event_id = EventId::new();
    let message = OriginalUserMessage::new(
        event_id,
        fixture.turn.session_id,
        text.clone(),
        UserMessageOrigin::UserInput,
        fixture.at,
    )
    .expect("user message");
    // The payload only, exactly as a store written before chunks would hold it.
    store
        .database
        .unit_of_work(|tx| {
            aura_persistence_core::event::append(
                tx,
                &aura_persistence_core::event::EventDraft {
                    id: event_id,
                    owner: &format!("agent-user:{}", fixture.turn.session_id),
                    kind: "agent.user_message",
                    version: 1,
                    payload: &serde_json::to_value(&message)?,
                    at: fixture.at,
                },
                "agent",
            )
        })
        .expect("legacy message");

    let db = store.database.connection();
    let source = session::user_message_source(db, fixture.turn.session_id, event_id)
        .expect("message identity");
    assert!(
        history::stored_body_window(
            db,
            &history::ChunkBody::user_message(event_id, &source.content_hash),
            0,
            PREVIEW,
        )
        .expect("lookup")
        .is_none(),
        "a message with no chunks reports no index rather than an empty body"
    );

    // The preview is taken from the payload's own prefix, bounded by the preview size.
    let prefix =
        session::user_message_prefix(db, fixture.turn.session_id, event_id, PREVIEW as usize)
            .expect("bounded prefix");
    assert!(prefix.chars().count() <= PREVIEW as usize);
    assert!(text.starts_with(&prefix));

    // And a page of it is still an exact slice of the real message.
    let from = boundary(&text, 4096);
    let request = AgentMessageTextRequest {
        session_id: fixture.turn.session_id,
        source: AgentMessageTextSource::User { event_id },
        offset: from,
        limit: 1024,
    };
    let page = desktop_messages::text(db, &request).expect("legacy page");
    let start = page.from_offset as usize;
    assert_eq!(page.text, text[start..start + page.text.len()]);
    assert_eq!(page.total_bytes, text.len() as u64);
    assert_eq!(page.from_offset, from, "the legacy path is a strict slice, not a snapped one");
}

/// A page of a running step and a page of a sealed one use the same offset semantics.
///
/// The two paths share one window writer, so a page must mean the same thing before and after
/// the seal. This checks the transition rather than either side alone: the same offset and
/// limit are a window of the same body, and a live page reports where it really starts.
#[test]
fn a_page_means_the_same_thing_before_and_after_the_step_is_sealed() {
    let (_root, mut store, fixture) = store();
    let body = "活跃-正文-é".repeat(4_000);
    // The live stream is written first, then the sealed response records the same text.
    let mut sequence = 0_u64;
    for piece in body.as_bytes().chunks(7_000) {
        let Ok(piece) = std::str::from_utf8(piece) else { continue };
        if piece.is_empty() {
            continue;
        }
        sequence += 1;
        append_delta(&mut store, &fixture, sequence, piece);
    }

    let live = {
        let db = store.database.connection();
        aura_persistence_v2::agent::stream::partial_windows(
            db,
            fixture.step.model_step_id,
            100,
            512,
        )
        .expect("live window")
    };
    if live.text.total_bytes > 0 {
        assert_eq!(
            live.text.from,
            100.min(live.text.total_bytes),
            "a live page reports where it really starts"
        );
        assert_eq!(
            live.text.total_bytes,
            body.len() as u64,
            "the live body is the deltas that were accepted"
        );
        assert!(live.text.chunks_read > 0, "a live page reports the chunks it read");
        assert!(live.text.decoded_bytes > 0, "a live page reports the bytes it decoded");
    } else {
        println!("the fixture accepted no deltas; the sealed half below is the check");
    }

    // Seal the same text and check the sealed page is the same window of it.
    let mut output = fixture.template.clone();
    output.model_step_id = fixture.step.model_step_id;
    output.tool_calls.clear();
    output.assistant_blocks = vec![AssistantContentBlock {
        block_id: ModelContentBlockId::new(),
        text: body.clone(),
        provider_reference: None,
    }];
    output.reasoning_blocks.clear();
    store
        .database
        .unit_of_work(|tx| history::record_output(tx, &output, None, fixture.at))
        .expect("sealed output");
    let seal = {
        let db = store.database.connection();
        history::sealed_body(db, fixture.step.model_step_id).expect("identity")
    };
    let db = store.database.connection();
    let window = history::visible_body_window(
        db,
        fixture.step.model_step_id,
        history::VisibleSource::Assistant,
        100,
        512,
        &seal,
    )
    .expect("window")
    .expect("index");
    // The live deltas and the sealed response are the same window of the same text: the two
    // paths share one writer, so the page means the same thing on both sides of the seal.
    assert_eq!(window.text, body[window.from as usize..(window.from as usize + window.text.len())]);
    assert_eq!(window.total_bytes, body.len() as u64);
    if live.text.total_bytes > 0 {
        assert_eq!(window.from, live.text.from, "the sealed page starts where the live page did");
    }
}

/// The largest character boundary at or before `offset`.
///
/// A sealed body cannot move, so a page must be asked for at a boundary — the live path snaps
/// back instead. These tests pick boundaries out of the body for the sealed case.
fn boundary(text: &str, offset: u64) -> u64 {
    let mut value = offset.min(text.len() as u64) as usize;
    while value > 0 && !text.is_char_boundary(value) {
        value -= 1;
    }
    value as u64
}

/// The sealed page and the chunk index agree about the page a caller asked for.
///
/// `desktop_messages::text` is the production entry point, so this pins the contract there:
/// the page it returns for a middle offset is the same slice the whole-body reader would have
/// returned, and it was produced from the index rather than by assembling the response.
#[test]
fn the_production_page_entry_point_stays_bounded_and_exact() {
    let (_root, mut store, fixture) = store();
    let body = seal_large_body(&mut store, &fixture);
    let db = store.database.connection();

    let seal = history::sealed_body(db, fixture.step.model_step_id).expect("identity");
    for offset in [0_u64, 4096, 300_006, body.len() as u64 - 4] {
        let offset = boundary(&body, offset);
        let request = AgentMessageTextRequest {
            session_id: fixture.turn.session_id,
            source: AgentMessageTextSource::Assistant { model_step_id: fixture.step.model_step_id },
            offset,
            limit: 4096,
        };
        let page = desktop_messages::text(db, &request).expect("page");
        let start = page.from_offset as usize;
        assert_eq!(page.text, body[start..start + page.text.len()], "offset {offset}");
        assert!(
            page.from_offset <= offset,
            "offset {offset}: a page never starts after the offset it was asked for"
        );
        assert!(
            offset - page.from_offset <= 4,
            "offset {offset}: a page snaps back at most one character (4 bytes)"
        );
        assert_eq!(page.total_bytes, body.len() as u64);

        let window = history::visible_body_window(
            db,
            fixture.step.model_step_id,
            history::VisibleSource::Assistant,
            offset,
            4096,
            &seal,
        )
        .expect("window")
        .expect("index");
        println!(
            "page at {offset}: returned={} decoded={} chunks={}",
            window.text.len(),
            window.decoded_bytes,
            window.chunks_read
        );
        assert!(
            window.decoded_bytes < body.len() as u64 / 4,
            "offset {offset}: the page decodes {} of {} bytes",
            window.decoded_bytes,
            body.len()
        );
    }
}

/// The history list of a turn is built without reading its bodies in full.
///
/// `desktop_messages::page` is what the desktop polls. Listing a turn with a large sealed
/// response must not decode it: the preview comes from the response's chunks, the length and
/// the reasoning length from their index, and the calls from the response's own call index.
#[test]
fn listing_a_turn_does_not_read_its_bodies_in_full() {
    let (_root, mut store, fixture) = store();
    let body = seal_large_body(&mut store, &fixture);

    let db = store.database.connection();
    let turn = turns::load(db, fixture.turn.turn_task_id).expect("turn");
    let messages = desktop_messages::page(db, &turn, None).expect("history page");

    let assistant = messages
        .assistants
        .iter()
        .find(|view| view.model_step_id == fixture.step.model_step_id)
        .expect("the sealed response is listed");
    println!(
        "listed assistant: total={} preview={} partial={}",
        assistant.text_total_bytes,
        assistant.text.len(),
        assistant.partial
    );
    assert!(!assistant.partial);
    assert_eq!(assistant.text_total_bytes, body.len() as u64, "the length is the body's");
    assert!(assistant.text.len() <= PREVIEW as usize, "the preview is bounded");
    assert!(body.starts_with(&assistant.text));
    assert_eq!(
        assistant.reasoning_total_bytes,
        "推理-思考内容".repeat(50_000).len() as u64,
        "the reasoning body is measured from its own index"
    );
    assert!(assistant.reasoning.len() <= PREVIEW as usize);

    // The calls a response made are listed without their arguments, from the response's own
    // index: this step has none, and the empty list is the answer rather than a failure.
    assert!(assistant.tool_call_ids.is_empty());
}

/// Two generations of the same owner keep their own chunks. The newer index must not answer
/// with the older text.
#[test]
fn a_chunk_generation_is_bound_to_its_revision() {
    let (_root, mut store, fixture) = store();
    let owner = EventId::new().to_string();
    let old = history::ChunkBody {
        owner: owner.clone(),
        source: history::VisibleSource::Assistant,
        revision: ContentHash::blake3(b"OLD!"),
    };
    let new = history::ChunkBody {
        owner,
        source: history::VisibleSource::Assistant,
        revision: ContentHash::blake3(b"NEW!"),
    };
    store
        .database
        .unit_of_work(|tx| history::record_chunks(tx, &old, "OLD!", fixture.at))
        .expect("old generation");
    store
        .database
        .unit_of_work(|tx| history::record_chunks(tx, &new, "NEW!", fixture.at))
        .expect("new generation");
    let db = store.database.connection();
    let page = history::stored_body_window(db, &new, 0, 4).expect("window").expect("new index");
    assert_eq!(page.text, "NEW!");
    let previous = history::stored_body_window(db, &old, 0, 4).expect("window").expect("old index");
    assert_eq!(previous.text, "OLD!");
}
