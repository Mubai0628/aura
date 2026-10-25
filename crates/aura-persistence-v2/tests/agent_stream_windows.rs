//! The live visible body of a step that is still running: what a page of it returns, and
//! what a client may conclude from that page.
//!
//! These are the regressions for two defects that both made a running step's text disagree
//! with the text the whole-body reader reports:
//!
//! * a page whose window landed in the assistant text advanced both sources for every
//!   indexed delta, so the offsets moved twice and the page returned bytes from the wrong
//!   place — and reported a body longer than the one that exists;
//! * a stream stored before the delta index existed lost its prefix as soon as one indexed
//!   delta was appended, because "any index row exists" was read as "the index is complete".
//!
//! A page and the whole-body reader must agree on the text, the total length and the next
//! offset for every legal window, so the assertions compare the two rather than a literal.
#![allow(clippy::unwrap_used)]
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;

use aura_agent_domain::*;
use aura_domain_common::*;
use aura_model_domain::{ModelStreamEvent, ModelStreamEventEnvelope};
use aura_persistence_v2::{
    V2Store,
    agent::{desktop_messages, stream},
    model,
};

fn setup() -> (tempfile::TempDir, V2Store, support::Fixture, TransportAttemptId) {
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let snapshot = model::step_snapshot(store.database.connection(), fixture.step.model_step_id)
        .expect("step snapshot");
    let attempt = aura_contract_schema::canonical_fixture()
        .unwrap()
        .models
        .transport_attempt
        .expect("transport attempt fixture");
    store
        .database
        .unit_of_work(|tx| model::insert_transport(tx, snapshot.id, &attempt))
        .expect("transport attempt");
    (root, store, fixture, attempt.id)
}

/// Appends one delta of the live stream, as the receiver really delivers them.
fn append(
    store: &mut V2Store,
    fixture: &support::Fixture,
    attempt: TransportAttemptId,
    sequence: u64,
    text: &str,
    reasoning: bool,
) {
    let block_id = ModelContentBlockId::new();
    let event = ModelStreamEventEnvelope {
        model_step_id: fixture.step.model_step_id,
        transport_attempt_id: attempt,
        sequence,
        received_at: fixture.at,
        event: if reasoning {
            ModelStreamEvent::ReasoningDelta { block_id, text: text.into() }
        } else {
            ModelStreamEvent::AssistantTextDelta { block_id, text: text.into() }
        },
    };
    store.database.unit_of_work(|tx| stream::append(tx, &event)).expect("append delta");
}

/// The text and reasoning of a step are two bodies that interleave in one stream, and a page
/// of one of them must be a window of *that* body. Reading a delta for the other source used
/// to advance this source's cursor a second time, which returned text from an earlier offset
/// and a total longer than the body.
#[test]
fn an_interleaved_live_stream_pages_the_same_bytes_as_the_whole_body() {
    let (_root, mut store, fixture, attempt) = setup();
    append(&mut store, &fixture, attempt, 1, &"A".repeat(30), false);
    append(&mut store, &fixture, attempt, 2, &"R".repeat(42), true);
    append(&mut store, &fixture, attempt, 3, "BBBBB", false);
    append(&mut store, &fixture, attempt, 4, "012345678901234567890123456789", false);

    let whole = stream::partial(store.database.connection(), fixture.step.model_step_id)
        .expect("whole body");
    assert_eq!(
        whole.text,
        format!("{}{}{}", "A".repeat(30), "BBBBB", "012345678901234567890123456789")
    );
    assert_eq!(whole.text.len(), 65, "the body is the assistant deltas of one attempt");
    assert_eq!(whole.reasoning, "R".repeat(42));

    // Every legal window of the assistant body has to agree with the body itself: the same
    // bytes, the same total, and a next offset that continues from where the page stopped.
    for offset in (0..=whole.text.len() as u64).step_by(3) {
        for limit in [4_u64, 7, 16, 1024] {
            let page = stream::partial_windows(
                store.database.connection(),
                fixture.step.model_step_id,
                offset,
                limit,
            )
            .expect("page");
            assert_eq!(
                page.text.total_bytes,
                whole.text.len() as u64,
                "offset={offset} limit={limit}: a page reports the real body length"
            );
            let end = whole.text.len() as u64;
            let remaining = end.saturating_sub(offset);
            let want = if remaining < limit { remaining } else { limit };
            assert_eq!(
                page.text.text.len() as u64,
                want,
                "offset={offset} limit={limit}: the page spans its limit or the body's end"
            );
            assert_eq!(
                page.text.text,
                whole.text[offset as usize..(offset + want) as usize],
                "offset={offset} limit={limit}: the page is a window of the body"
            );
            assert_eq!(
                page.text.next,
                (offset + want < end).then_some(offset + want),
                "offset={offset} limit={limit}: the next offset continues from the page"
            );
        }
    }

    // The specific window the review reported: offset 40 over a 65-byte body.
    let window =
        stream::partial_windows(store.database.connection(), fixture.step.model_step_id, 40, 4)
            .expect("page");
    assert_eq!(window.text.text, "5678");
    assert_eq!(window.text.total_bytes, 65);
}

/// A reasoning page keeps its own offsets, so the two bodies are paginated independently.
#[test]
fn a_reasoning_page_is_a_window_of_the_reasoning_body() {
    let (_root, mut store, fixture, attempt) = setup();
    append(&mut store, &fixture, attempt, 1, &"A".repeat(30), false);
    append(&mut store, &fixture, attempt, 2, "第一段第二段第三段", true);
    append(&mut store, &fixture, attempt, 3, "tail", false);
    append(&mut store, &fixture, attempt, 4, "more reasoning", true);

    let whole = stream::partial(store.database.connection(), fixture.step.model_step_id)
        .expect("whole body");
    let page =
        stream::partial_windows(store.database.connection(), fixture.step.model_step_id, 9, 9)
            .expect("page");
    assert_eq!(page.reasoning.text, "第二段");
    assert_eq!(page.reasoning.total_bytes, whole.reasoning.len() as u64);

    let request = AgentMessageTextRequest {
        session_id: fixture.turn.session_id,
        source: AgentMessageTextSource::Reasoning { model_step_id: fixture.step.model_step_id },
        offset: 9,
        limit: 9,
    };
    let served = desktop_messages::text(store.database.connection(), &request).expect("text page");
    assert_eq!(served.from_offset, 9);
    assert_eq!(served.text, "第二段");
}

/// A stream that predates the delta index must keep everything it already holds. Appending
/// one indexed delta used to make the page reader believe the index was complete, and the
/// prefix that had no index row disappeared from the page while the whole-body reader still
/// reported it.
#[test]
fn an_older_stream_keeps_its_prefix_after_an_indexed_append() {
    let (_root, mut store, fixture, attempt) = setup();
    let payload = serde_json::json!({
        "attempt": attempt,
        "sequence": 1,
        "text": "legacy-prefix/",
        "reasoning": "",
    });
    store
        .database
        .unit_of_work(|tx| {
            aura_persistence_core::event::append(
                tx,
                &aura_persistence_core::event::EventDraft {
                    id: EventId::new(),
                    owner: &fixture.step.model_step_id.to_string(),
                    kind: "agent.visible_delta",
                    version: 1,
                    payload: &payload,
                    at: fixture.at,
                },
                "agent",
            )
        })
        .expect("a delta written before the index existed");

    append(&mut store, &fixture, attempt, 2, "new-suffix", false);

    let whole = stream::partial(store.database.connection(), fixture.step.model_step_id)
        .expect("whole body");
    assert_eq!(whole.text, "legacy-prefix/new-suffix");
    let page =
        stream::partial_windows(store.database.connection(), fixture.step.model_step_id, 0, 1024)
            .expect("page");
    assert_eq!(
        page.text.text, "legacy-prefix/new-suffix",
        "a partially indexed old stream must retain its prefix"
    );
    assert_eq!(page.text.total_bytes, whole.text.len() as u64);

    // The backfill is what makes the indexed path correct from here on: every delta of this
    // attempt now has exactly one index row, so pages and the whole body cannot disagree.
    let indexed: i64 = store
        .database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM events WHERE owner=?1 AND kind='agent.visible_delta_index' \
             AND json_extract(payload,'$.attempt')=?2",
            rusqlite::params![fixture.step.model_step_id.to_string(), attempt.to_string()],
            |row| row.get(0),
        )
        .expect("index count");
    assert_eq!(indexed, 2, "one index row per delta of the attempt, including the backfilled one");
}

/// A single source with no interleaving is the case the old walk happened to get right, so it
/// stays covered: the fix must not have moved the simple path.
#[test]
fn a_single_source_stream_pages_exactly() {
    let (_root, mut store, fixture, attempt) = setup();
    append(&mut store, &fixture, attempt, 1, "0123456789", false);
    append(&mut store, &fixture, attempt, 2, "abcdefghij", false);

    let whole = stream::partial(store.database.connection(), fixture.step.model_step_id)
        .expect("whole body");
    assert_eq!(whole.text, "0123456789abcdefghij");
    assert!(whole.reasoning.is_empty());
    for offset in 0..=whole.text.len() as u64 {
        let page = stream::partial_windows(
            store.database.connection(),
            fixture.step.model_step_id,
            offset,
            4,
        )
        .expect("page");
        let remaining = (whole.text.len() as u64).saturating_sub(offset);
        let want = remaining.min(4);
        assert_eq!(page.text.text, whole.text[offset as usize..(offset + want) as usize]);
        assert_eq!(page.text.total_bytes, 20);
    }
}

/// A page request that lands inside a multi-byte character snaps back to the boundary
/// instead of returning half of one, and says where the text it returned really starts.
#[test]
fn a_page_boundary_snaps_to_a_character_for_both_sources() {
    let (_root, mut store, fixture, attempt) = setup();
    append(&mut store, &fixture, attempt, 1, "中文内容测试", false);
    append(&mut store, &fixture, attempt, 2, "思考内容测试", true);

    let whole = stream::partial(store.database.connection(), fixture.step.model_step_id)
        .expect("whole body");
    let inside = 1_u64;
    let page =
        stream::partial_windows(store.database.connection(), fixture.step.model_step_id, inside, 6)
            .expect("page");
    assert_eq!(page.text.from, 0, "a page never starts inside a character");
    assert!(whole.text.starts_with(&page.text.text));
    assert_eq!(page.text.total_bytes, whole.text.len() as u64);
    assert_eq!(page.reasoning.total_bytes, whole.reasoning.len() as u64);
}

/// A delta stored before the byte index existed is measured in UTF-8 bytes, including when a
/// character-count index was already written for it.
#[test]
fn a_legacy_multibyte_delta_is_paged_in_bytes_even_with_a_character_index() {
    let (_root, mut store, fixture, attempt) = setup();
    let prefix = "中文🙂";
    let owner = fixture.step.model_step_id.to_string();
    let payload = serde_json::json!({
        "attempt": attempt,
        "sequence": 1,
        "text": prefix,
        "reasoning": "",
    });
    store
        .database
        .unit_of_work(|tx| {
            aura_persistence_core::event::append(
                tx,
                &aura_persistence_core::event::EventDraft {
                    id: EventId::new(),
                    owner: &owner,
                    kind: "agent.visible_delta",
                    version: 1,
                    payload: &payload,
                    at: fixture.at,
                },
                "agent",
            )
        })
        .expect("legacy delta");
    let wrong = serde_json::json!({
        "attempt": attempt,
        "sequence": 1,
        "text_len": prefix.chars().count() as u64,
        "reasoning_len": 0,
    });
    store
        .database
        .unit_of_work(|tx| {
            aura_persistence_core::event::append(
                tx,
                &aura_persistence_core::event::EventDraft {
                    id: EventId::new(),
                    owner: &owner,
                    kind: "agent.visible_delta_index",
                    version: 1,
                    payload: &wrong,
                    at: fixture.at,
                },
                "agent",
            )
        })
        .expect("character index");
    append(&mut store, &fixture, attempt, 2, "tail", false);
    let whole =
        stream::partial(store.database.connection(), fixture.step.model_step_id).expect("whole");
    let page =
        stream::partial_windows(store.database.connection(), fixture.step.model_step_id, 6, 8)
            .expect("page");
    assert_eq!(whole.text.len(), 14);
    assert_eq!(page.text.text, &whole.text[6..]);
    assert_eq!(page.text.total_bytes, whole.text.len() as u64);
    assert_eq!(page.text.text, "🙂tail");
}

/// A page of a live stream locates the window on the cumulative index, not by scanning
/// every event for this owner and then filtering JSON.
#[test]
fn a_live_window_is_located_on_the_visible_index() {
    let (_root, store, _fixture, _attempt) = setup();
    let connection = store.database.connection();
    let plan = explain(connection, stream::TEXT_WINDOW_SQL);
    assert!(
        plan.contains("agent_visible_index_text"),
        "the production text window must seek the text index: {plan}"
    );
    assert!(!plan.contains("events"), "{plan}");
    let narrow = window_steps(connection, 100);
    let wide = window_steps(connection, 1_000);
    assert!(
        wide < narrow.saturating_mul(4),
        "a 4-byte tail of 1000 deltas took {wide} steps after {narrow} for 100"
    );
    for (label, offset) in [("head", 0_i64), ("middle", 200_i64)] {
        let narrow_page = window_steps_at(connection, 100, offset, stream::TEXT_WINDOW_SQL);
        let wide_page = window_steps_at(connection, 1_000, offset * 10, stream::TEXT_WINDOW_SQL);
        assert!(
            wide_page < narrow_page.saturating_mul(4),
            "{label} text page of 1000 deltas took {wide_page} steps after {narrow_page} for 100"
        );
        let narrow_reasoning =
            window_steps_at(connection, 100, offset, stream::REASONING_WINDOW_SQL);
        let wide_reasoning =
            window_steps_at(connection, 1_000, offset * 10, stream::REASONING_WINDOW_SQL);
        assert!(
            wide_reasoning < narrow_reasoning.saturating_mul(4),
            "{label} reasoning page of 1000 deltas took {wide_reasoning} steps after {narrow_reasoning} for 100"
        );
    }
    let delta_plan = explain(connection, stream::DELTA_TEXT_SQL);
    assert!(
        delta_plan.contains("agent_visible_delta"),
        "a delta page must read the delta row: {delta_plan}"
    );
    assert!(!delta_plan.contains("events"), "{delta_plan}");
}

fn explain(connection: &rusqlite::Connection, sql: &str) -> String {
    let mut statement = connection.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    let count = statement.parameter_count();
    let mut rows = statement
        .query(rusqlite::params_from_iter((0..count).map(|index| {
            if index < 2 {
                rusqlite::types::Value::Text("owner".to_owned())
            } else {
                rusqlite::types::Value::Integer(1)
            }
        })))
        .unwrap();
    let mut plan = Vec::new();
    while let Some(row) = rows.next().unwrap() {
        plan.push(row.get::<_, String>(3).unwrap());
    }
    plan.join("\n")
}

fn window_steps(connection: &rusqlite::Connection, rows: i64) -> i32 {
    let owner = format!("vm-{rows}");
    let attempt = "attempt";
    for sequence in 1..=rows {
        let start = (sequence - 1) * 4;
        connection
            .execute(
                "INSERT INTO agent_visible_index(owner,attempt,sequence,text_start,text_end,text_len,reasoning_start,reasoning_end,reasoning_len) \
                 VALUES(?1,?2,?3,?4,?5,4,?4,?5,4)",
                rusqlite::params![owner, attempt, sequence, start, start + 4],
            )
            .unwrap();
    }
    window_steps_at(connection, rows, rows * 4 - 4, stream::TEXT_WINDOW_SQL)
}

fn window_steps_at(connection: &rusqlite::Connection, rows: i64, from: i64, sql: &str) -> i32 {
    let owner = format!("vm-{rows}");
    let attempt = "attempt";
    let mut statement = connection.prepare(sql).unwrap();
    let found: Vec<i64> = statement
        .query_map(rusqlite::params![owner, attempt, from, from + 4], |row| row.get(0))
        .unwrap()
        .map(|row| row.unwrap())
        .collect();
    assert_eq!(found.len(), 1, "a 4-byte window selects one row at {from} of {rows}");
    statement.get_status(rusqlite::StatementStatus::VmStep)
}

/// Deleting the locatable rows and appending again must not hide the prefix.
#[test]
fn an_append_after_a_deleted_index_keeps_the_prefix_on_the_page() {
    let (_root, mut store, fixture, attempt) = setup();
    append(&mut store, &fixture, attempt, 1, "PREFIX", false);
    store
        .database
        .connection()
        .execute("DELETE FROM agent_visible_index", [])
        .expect("drop derived index");
    append(&mut store, &fixture, attempt, 2, "TAIL", false);
    let whole =
        stream::partial(store.database.connection(), fixture.step.model_step_id).expect("whole");
    let page =
        stream::partial_windows(store.database.connection(), fixture.step.model_step_id, 0, 20)
            .expect("page");
    assert_eq!(whole.text, "PREFIXTAIL");
    assert_eq!(page.text.text, whole.text);
    assert_eq!(page.text.total_bytes, 10);
}

/// A stream that has only the original delta, and no index event, is repaired on open.
#[test]
fn opening_repairs_a_legacy_stream_that_has_no_index_events() {
    let (root, mut store, fixture, attempt) = setup();
    let owner = fixture.step.model_step_id.to_string();
    let payload = serde_json::json!({
        "attempt": attempt,
        "sequence": 1,
        "text": "legacy-prefix",
        "reasoning": "",
    });
    store
        .database
        .unit_of_work(|tx| {
            aura_persistence_core::event::append(
                tx,
                &aura_persistence_core::event::EventDraft {
                    id: EventId::new(),
                    owner: &owner,
                    kind: "agent.visible_delta",
                    version: 1,
                    payload: &payload,
                    at: fixture.at,
                },
                "agent",
            )
        })
        .expect("legacy delta");
    drop(store);
    let store = V2Store::open(&root.path().join("v2")).expect("reopen");
    let count: i64 = store
        .database
        .connection()
        .query_row("SELECT COUNT(*) FROM agent_visible_index WHERE owner=?1", [&owner], |row| {
            row.get(0)
        })
        .expect("count");
    assert!(count > 0, "open repairs a delta that never had an index event");
    let page =
        stream::partial_windows(store.database.connection(), fixture.step.model_step_id, 0, 20)
            .expect("page");
    assert_eq!(page.text.text, "legacy-prefix");
}

/// A missing middle index row is not a complete generation. The page keeps the
/// source text, and a later append does not certify the hole.
#[test]
fn a_missing_middle_index_row_does_not_drop_text() {
    let (_root, mut store, fixture, attempt) = setup();
    for (sequence, text) in [(1, "AAA"), (2, "BBB"), (3, "CCC")] {
        append(&mut store, &fixture, attempt, sequence, text, false);
    }
    store
        .database
        .connection()
        .execute("DELETE FROM agent_visible_index WHERE sequence=2", [])
        .expect("drop middle row");
    let before =
        stream::partial_windows(store.database.connection(), fixture.step.model_step_id, 0, 30)
            .expect("page");
    assert_eq!(before.text.text, "AAABBBCCC");
    append(&mut store, &fixture, attempt, 4, "DDD", false);
    let after =
        stream::partial_windows(store.database.connection(), fixture.step.model_step_id, 0, 40)
            .expect("page after append");
    let whole =
        stream::partial(store.database.connection(), fixture.step.model_step_id).expect("whole");
    assert_eq!(after.text.text, whole.text);
    assert_eq!(after.text.text, "AAABBBCCCDDD");
}

/// The first unfinished object cannot spend more than the byte budget.
#[test]
fn repair_stops_inside_the_first_large_delta() {
    let (_root, mut store, fixture, attempt) = setup();
    let owner = fixture.step.model_step_id.to_string();
    let payload = serde_json::json!({
        "attempt": attempt,
        "sequence": 1,
        "text": "x".repeat(1_048_576),
        "reasoning": "",
    });
    store
        .database
        .unit_of_work(|tx| {
            aura_persistence_core::event::append(
                tx,
                &aura_persistence_core::event::EventDraft {
                    id: EventId::new(),
                    owner: &owner,
                    kind: "agent.visible_delta",
                    version: 1,
                    payload: &payload,
                    at: fixture.at,
                },
                "agent",
            )
        })
        .expect("large delta");
    let repaired = store
        .database
        .unit_of_work(|tx| stream::repair_outstanding(tx, fixture.at, 262_144))
        .expect("repair");
    let copied: i64 = store
        .database
        .connection()
        .query_row(
            "SELECT COALESCE(SUM(length(CAST(text AS BLOB))+length(CAST(reasoning AS BLOB))),0) \
             FROM agent_visible_delta",
            [],
            |row| row.get(0),
        )
        .expect("copied");
    assert_eq!(repaired, 1);
    assert!(copied > 0 && copied <= 262_144, "copied {copied} bytes");
}

/// One budgeted repair returns a slice. The source JSON is not handed back whole, twice.
#[test]
fn repair_slice_does_not_return_the_source_json_twice() {
    let (_root, mut store, fixture, attempt) = setup();
    let owner = fixture.step.model_step_id.to_string();
    let payload = serde_json::json!({
        "attempt": attempt,
        "sequence": 1,
        "text": "x".repeat(1_048_576),
        "reasoning": "",
    });
    store
        .database
        .unit_of_work(|tx| {
            aura_persistence_core::event::append(
                tx,
                &aura_persistence_core::event::EventDraft {
                    id: EventId::new(),
                    owner: &owner,
                    kind: "agent.visible_delta",
                    version: 1,
                    payload: &payload,
                    at: fixture.at,
                },
                "agent",
            )
        })
        .expect("large delta");
    for slice in 0..2 {
        let db = store.database.connection();
        FULL_SOURCE_ROW.with(|flag| flag.set(false));
        db.trace_v2(rusqlite::trace::TraceEventCodes::SQLITE_TRACE_ROW, Some(note_source_row));
        let tx = db.unchecked_transaction().unwrap();
        let result = stream::repair_outstanding(&tx, fixture.at, 262_144);
        result.expect("repair");
        tx.commit().unwrap();
        db.trace_v2(rusqlite::trace::TraceEventCodes::SQLITE_TRACE_ROW, None);
        assert!(
            !FULL_SOURCE_ROW.with(|flag| flag.get()),
            "slice {slice} selected the source payload instead of a bounded substr"
        );
        let copied: i64 = store
            .database
            .connection()
            .query_row(
                "SELECT COALESCE(SUM(length(CAST(text AS BLOB))),0) FROM agent_visible_delta",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(copied <= (slice + 1) * 262_144, "copied {copied}");
    }
}

thread_local! {
    static FULL_SOURCE_ROW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn note_source_row(event: rusqlite::trace::TraceEvent<'_>) {
    let rusqlite::trace::TraceEvent::Row(statement) = event else {
        return;
    };
    if row_returns_source_text(&statement.sql()) {
        FULL_SOURCE_ROW.with(|flag| flag.set(true));
    }
}

/// A result column that is the event payload, or a text/reasoning extract outside
/// `substr` / `length`, is the read that returned the whole JSON twice.
fn row_returns_source_text(sql: &str) -> bool {
    let compact = sql.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase();
    if compact.contains("select payload ") || compact.contains("select payload,") {
        return true;
    }
    for needle in [
        "json_extract(payload,'$.text')",
        "json_extract(payload, '$.text')",
        "json_extract(payload,'$.reasoning')",
        "json_extract(payload, '$.reasoning')",
    ] {
        let mut rest = compact.as_str();
        while let Some(index) = rest.find(needle) {
            let prefix = &rest[..index];
            let bounded = prefix.ends_with("substr(cast(") || prefix.ends_with("length(cast(");
            if !bounded {
                return true;
            }
            rest = &rest[index + needle.len()..];
        }
    }
    false
}
