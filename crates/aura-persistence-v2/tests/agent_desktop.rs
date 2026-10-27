#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_execution_contract::SideEffectStateV1;
use aura_model_domain::*;
use aura_persistence_core::{PersistenceCoreError, StoreResult, artifact};
use aura_persistence_v2::{
    V2Store,
    agent::{desktop, desktop_messages, stream},
    model, tool,
};
use aura_tool_contract::*;

#[test]
fn visible_stream_deduplicates_and_retries_cannot_mix_or_execute_partial_calls() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let snapshot =
        model::step_snapshot(store.database.connection(), f.step.model_step_id).expect("snapshot");
    let mut attempt = aura_contract_schema::canonical_fixture()
        .expect("fixture")
        .models
        .transport_attempt
        .expect("transport");
    let first = attempt.id;
    store
        .database
        .unit_of_work(|tx| model::insert_transport(tx, snapshot.id, &attempt))
        .expect("attempt");
    let event = |transport_attempt_id, sequence, event| ModelStreamEventEnvelope {
        model_step_id: f.step.model_step_id,
        transport_attempt_id,
        sequence,
        received_at: f.at,
        event,
    };
    let delta = |text: &str| ModelStreamEvent::AssistantTextDelta {
        block_id: ModelContentBlockId::new(),
        text: text.into(),
    };
    let one = event(first, 1, delta("old "));
    let two = event(first, 2, delta("attempt"));
    for value in [&one, &two, &one] {
        store
            .database
            .unit_of_work(|tx| stream::append(tx, value))
            .expect("ordered or identical repeat");
    }
    let old_body =
        stream::partial(store.database.connection(), f.step.model_step_id).expect("partial");
    assert_eq!(old_body.text, "old attempt");
    assert!(old_body.body_id.is_some(), "a live body identifies its attempt");
    let conflict = event(first, 1, delta("changed"));
    assert!(store.database.unit_of_work(|tx| stream::append(tx, &conflict)).is_err());
    attempt.id = TransportAttemptId::new();
    attempt.attempt_number += 1;
    store
        .database
        .unit_of_work(|tx| model::insert_transport(tx, snapshot.id, &attempt))
        .expect("retry");
    let restarted =
        stream::partial(store.database.connection(), f.step.model_step_id).expect("no old content");
    // A retry restarts the visible body: the old attempt's bytes are gone and
    // the identity changes, so a client holding them cannot append to this one.
    assert_eq!(restarted.text, "");
    assert_eq!(restarted.reasoning, "");
    assert_eq!(restarted.sequence, None);
    assert_eq!(restarted.body_id, Some(attempt.id.to_string()));
    assert_ne!(restarted.body_id, old_body.body_id);
    for value in [
        event(attempt.id, 1, delta("new answer")),
        event(first, 3, delta("late old")),
        event(
            attempt.id,
            2,
            ModelStreamEvent::ToolCallArgumentsDelta {
                provider_call_id: "call_0".parse().expect("id"),
                delta: "{\"command\":\"must not run".into(),
            },
        ),
    ] {
        store
            .database
            .unit_of_work(|tx| stream::append(tx, &value))
            .expect("visible event or ignored input");
    }
    let view = desktop::projection(
        store.database.connection(),
        &AgentHistoryRequest {
            within_turn: None,
            session_id: f.turn.session_id,
            before_turn_task_id: None,
            limit: 5,
        },
        RuntimeInstanceId::new(),
    )
    .expect("history");
    assert_eq!(view.turns[0].assistant_messages[0].text, "new answer");
    assert!(view.turns[0].assistant_messages[0].tool_call_ids.is_empty());
    assert!(view.turns[0].assistant_messages[0].partial);
    let foreign = event(TransportAttemptId::new(), 3, delta("wrong execution"));
    assert!(store.database.unit_of_work(|tx| stream::append(tx, &foreign)).is_err());
    let mut final_output = f.template.clone();
    final_output.model_step_id = f.step.model_step_id;
    final_output.tool_calls.clear();
    final_output.assistant_blocks = vec![AssistantContentBlock {
        block_id: ModelContentBlockId::new(),
        text: "sealed final".into(),
        provider_reference: None,
    }];
    store
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::agent::history::record_output(tx, &final_output, None, f.at)
        })
        .expect("terminal commit");
    let late = event(attempt.id, 3, delta("late tail"));
    store
        .database
        .unit_of_work(|tx| stream::append(tx, &late))
        .expect("ignore late terminal delta");
    let final_view = desktop::projection(
        store.database.connection(),
        &AgentHistoryRequest {
            within_turn: None,
            session_id: f.turn.session_id,
            before_turn_task_id: None,
            limit: 5,
        },
        RuntimeInstanceId::new(),
    )
    .expect("sealed view");
    assert!(!final_view.turns[0].assistant_messages[0].partial);
    assert!(final_view.turns[0].assistant_messages[0].text.starts_with("sealed final"));
}

/// A step that is still running has a real visible body, so the process area can
/// be read page by page long before the output is sealed. The pages carry the
/// identity of the attempt they came from, because a retry restarts that body.
#[test]
fn a_running_step_is_read_page_by_page_and_a_retry_restarts_its_body() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let snapshot =
        model::step_snapshot(store.database.connection(), f.step.model_step_id).expect("snapshot");
    let mut attempt = aura_contract_schema::canonical_fixture()
        .expect("fixture")
        .models
        .transport_attempt
        .expect("transport");
    store
        .database
        .unit_of_work(|tx| model::insert_transport(tx, snapshot.id, &attempt))
        .expect("attempt");
    let event = |transport_attempt_id, sequence, event| ModelStreamEventEnvelope {
        model_step_id: f.step.model_step_id,
        transport_attempt_id,
        sequence,
        received_at: f.at,
        event,
    };
    let delta = |text: &str| ModelStreamEvent::AssistantTextDelta {
        block_id: ModelContentBlockId::new(),
        text: text.into(),
    };
    let reasoning = |text: &str| ModelStreamEvent::ReasoningDelta {
        block_id: ModelContentBlockId::new(),
        text: text.into(),
    };
    for value in [
        event(attempt.id, 1, delta("有界")),
        event(attempt.id, 2, delta("结果")),
        event(attempt.id, 3, reasoning("思考")),
    ] {
        store.database.unit_of_work(|tx| stream::append(tx, &value)).expect("visible delta");
    }
    let mut read = AgentMessageTextRequest {
        session_id: f.turn.session_id,
        source: AgentMessageTextSource::Assistant { model_step_id: f.step.model_step_id },
        offset: 0,
        limit: 4,
    };
    let first = desktop_messages::text(store.database.connection(), &read).expect("live page");
    // Four bytes land inside the second character, so the page stops at the
    // boundary before it instead of returning half a character.
    assert_eq!(first.text, "有");
    assert_eq!(first.from_offset, 0);
    assert_eq!(first.next_offset, Some(3));
    assert_eq!(first.total_bytes, 12);
    assert_eq!(first.body_id, Some(attempt.id.to_string()));
    // An offset the caller holds from a body that has since grown is snapped to
    // the boundary at or before it, and the page says where it really started.
    read.offset = 4;
    let snapped = desktop_messages::text(store.database.connection(), &read).expect("snapped page");
    assert_eq!(snapped.from_offset, 3);
    assert_eq!(snapped.text, "界");

    read.offset = 0;
    read.limit = 4096;
    let whole = desktop_messages::text(store.database.connection(), &read).expect("whole body");
    assert_eq!(whole.text, "有界结果");
    assert_eq!(whole.next_offset, None);
    read.source = AgentMessageTextSource::Reasoning { model_step_id: f.step.model_step_id };
    let thoughts = desktop_messages::text(store.database.connection(), &read).expect("reasoning");
    assert_eq!(thoughts.text, "思考");
    assert_eq!(thoughts.total_bytes, 6);
    // Text and reasoning belong to the same visible stream, so one identity
    // covers both.
    assert_eq!(thoughts.body_id, first.body_id);

    // A retry starts a new visible body: the old bytes are not a prefix of it.
    attempt.id = TransportAttemptId::new();
    attempt.attempt_number += 1;
    store
        .database
        .unit_of_work(|tx| model::insert_transport(tx, snapshot.id, &attempt))
        .expect("retry");
    read.source = AgentMessageTextSource::Assistant { model_step_id: f.step.model_step_id };
    let before = desktop_messages::text(store.database.connection(), &read).expect("empty body");
    // The new attempt owns the visible stream from its first byte: the body is
    // empty and already identified, so a client never appends old bytes to it.
    assert_eq!(before.text, "");
    assert_eq!(before.body_id, Some(attempt.id.to_string()));
    assert_ne!(before.body_id, first.body_id);
    store
        .database
        .unit_of_work(|tx| stream::append(tx, &event(attempt.id, 1, delta("新"))))
        .expect("new attempt");
    let restarted =
        desktop_messages::text(store.database.connection(), &read).expect("restarted body");
    assert_eq!(restarted.text, "新");
    assert_ne!(restarted.body_id, first.body_id);
}

#[test]
fn result_pages_keep_utf8_boundaries_and_require_both_session_and_artifact_ownership() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let batch = f.admit(&mut store, vec![("fs.read", serde_json::json!({}))]);
    let call = batch.batch.calls[0].tool_call_id;
    let artifact_id = ArtifactId::new();
    let content = "有界结果🙂\nsecond page";
    store
        .database
        .unit_of_work(|tx| -> StoreResult<()> {
            artifact::put(
                tx,
                artifact_id,
                &call.to_string(),
                "text/plain",
                content.as_bytes(),
                f.at,
            )?;
            // A deliberately private artifact with no Agent grant cannot be read
            // merely because its ToolCall is in the right session.
            tool::execution::complete_call(
                tx,
                batch.batch.tool_batch_id,
                &ToolResultV1 {
                    tool_call_id: call,
                    outcome: ToolResultOutcomeV1::NotExecuted {
                        reason: ToolNotExecutedReasonV1::PermissionDenied,
                        error: None,
                    },
                    side_effect_state: SideEffectStateV1::NotStarted,
                    attempts: vec![],
                    model_visible_result_ref: Some(artifact_id),
                    user_audit_result_ref: None,
                    internal_diagnostic_ref: None,
                    aggregate_side_effect_evidence_ref: None,
                    started_at: None,
                    completed_at: f.at,
                },
            )
        })
        .expect("private result");
    let mut request = AgentResultRequest {
        session_id: f.turn.session_id,
        tool_call_id: call,
        offset: 0,
        limit: 4,
    };
    assert!(desktop::result(store.database.connection(), &request).is_err());
    store
        .database
        .unit_of_work(|tx| {
            artifact::grant_access(
                tx,
                artifact_id,
                &call.to_string(),
                &f.turn.agent_run_id.to_string(),
            )
        })
        .expect("explicit model-visible access");
    let mut rebuilt = String::new();
    loop {
        let page = desktop::result(store.database.connection(), &request).expect("bounded page");
        assert!(page.text.len() <= request.limit as usize);
        assert_eq!(page.total_bytes, content.len() as u64);
        rebuilt.push_str(&page.text);
        match page.next_offset {
            Some(offset) => request.offset = offset,
            None => break,
        }
    }
    assert_eq!(rebuilt, content);
    request.offset = 1;
    assert!(matches!(
        desktop::result(store.database.connection(), &request),
        Err(PersistenceCoreError::Invalid("text byte cursor"))
    ));
    request.offset = u64::MAX;
    assert!(desktop::result(store.database.connection(), &request).is_err());
    request.offset = 0;
    request.session_id = SessionId::new();
    assert!(desktop::result(store.database.connection(), &request).is_err());
}

/// A page of a sealed body is a slice of it; a page of a body that can still grow snaps back
/// to the boundary before an offset that fell inside a character. Both are answered from a
/// window of the body, so the reference implementations below are written out independently
/// and the windows are also checked for staying bounded.
fn sealed_slice(content: &str, offset: u64, limit: u64) -> (String, u64, Option<u64>) {
    let start = usize::try_from(offset).expect("offset").min(content.len());
    assert!(content.is_char_boundary(start), "a sealed cursor is exact");
    let mut end = start.saturating_add(usize::try_from(limit).expect("limit")).min(content.len());
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    (content[start..end].into(), start as u64, (end < content.len()).then_some(end as u64))
}

fn live_slice(content: &str, offset: u64, limit: u64) -> (String, u64, Option<u64>) {
    let mut start = usize::try_from(offset).expect("offset").min(content.len());
    while start > 0 && !content.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = start.saturating_add(usize::try_from(limit).expect("limit")).min(content.len());
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    (content[start..end].into(), start as u64, (end < content.len()).then_some(end as u64))
}

#[test]
fn a_large_sealed_body_is_paged_through_windows_without_assembling_it() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    // A body big enough that copying it per page would be visible, with characters that
    // straddle the page boundaries so the snap is exercised.
    let body = "有界结果-é".repeat(40_000);
    let thoughts = format!("思考{}", "é".repeat(50_000));
    let mut output = f.template.clone();
    output.model_step_id = f.step.model_step_id;
    output.tool_calls.clear();
    output.assistant_blocks = vec![AssistantContentBlock {
        block_id: ModelContentBlockId::new(),
        text: body.clone(),
        provider_reference: None,
    }];
    output.reasoning_blocks = vec![ReasoningContentBlock {
        block_id: ModelContentBlockId::new(),
        text: "unused".into(),
        summary: Some(thoughts.clone()),
        provider_reference: None,
    }];
    store
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::agent::history::record_output(tx, &output, None, f.at)
        })
        .expect("sealed output");
    let db = store.database.connection();
    let sealed =
        aura_persistence_v2::agent::history::output(db, f.step.model_step_id).expect("sealed body");
    assert_eq!(aura_persistence_v2::agent::history::visible_text(&sealed), body);
    let mut read = AgentMessageTextRequest {
        session_id: f.turn.session_id,
        source: AgentMessageTextSource::Assistant { model_step_id: f.step.model_step_id },
        offset: 0,
        limit: 4096,
    };
    // Pages are exact slices of the sealed body and walking them rebuilds it.
    let mut rebuilt = String::new();
    loop {
        let page = desktop_messages::text(db, &read).expect("sealed page");
        let (text, from, next) = sealed_slice(&body, read.offset, u64::from(read.limit));
        assert_eq!(page.text, text);
        assert_eq!(page.from_offset, from);
        assert_eq!(page.next_offset, next);
        assert_eq!(page.total_bytes, body.len() as u64);
        assert_eq!(page.body_id, None, "sealed text has no growing body");
        let identity = aura_persistence_v2::agent::history::sealed_body(db, f.step.model_step_id)
            .expect("revision");
        assert_eq!(page.sealed_revision.as_deref(), Some(identity.revision.to_string().as_str()));
        rebuilt.push_str(&page.text);
        match page.next_offset {
            Some(offset) => read.offset = offset,
            None => break,
        }
    }
    assert_eq!(rebuilt, body);
    // The window copies its own size, never the body: the same page over the sealed record
    // and over the reasoning record stays bounded even though both are far larger. Offsets
    // inside a character are included, because a window snaps them back the way a live page
    // does — the sealed entry point rejects them, which is checked below.
    let projection = desktop::projection(
        db,
        &AgentHistoryRequest {
            within_turn: None,
            session_id: f.turn.session_id,
            before_turn_task_id: None,
            limit: 5,
        },
        RuntimeInstanceId::new(),
    )
    .expect("history");
    // The history projection carries metadata and a bounded preview, never the body: the
    // per-poll cost of a running conversation does not grow with the answer.
    let assistant = &projection.turns[0].assistant_messages[0];
    assert_eq!(assistant.text_total_bytes, body.len() as u64);
    assert_eq!(assistant.reasoning_total_bytes, thoughts.len() as u64);
    assert!(assistant.text.len() <= 2048);
    assert!(assistant.reasoning.len() <= 2048);
    assert!(!assistant.partial);
    assert!(assistant.sealed_revision.is_some());
    for source in [
        AgentMessageTextSource::Assistant { model_step_id: f.step.model_step_id },
        AgentMessageTextSource::Reasoning { model_step_id: f.step.model_step_id },
        AgentMessageTextSource::User { event_id: projection.turns[0].user_messages[0].event_id },
    ] {
        let page = desktop_messages::text(
            db,
            &AgentMessageTextRequest {
                session_id: f.turn.session_id,
                source,
                offset: 0,
                limit: 16,
            },
        )
        .expect("revision page");
        let expected = if matches!(source, AgentMessageTextSource::User { .. }) {
            &projection.turns[0].user_messages[0].sealed_revision
        } else {
            &assistant.sealed_revision
        };
        assert_eq!(&page.sealed_revision, expected);
    }
    let length = body.len() as u64;
    for offset in [0_u64, 1, 3, 4095, 4096, 40_001, length - 1, length, length + 64] {
        for limit in [4_u64, 4096] {
            let window =
                aura_persistence_v2::agent::history::visible_window(&sealed, offset, limit);
            let (text, from, next) = live_slice(&body, offset, limit);
            assert_eq!(window.text, text, "offset {offset} limit {limit}");
            assert_eq!(window.from, from);
            assert_eq!(window.next, next);
            assert_eq!(window.total_bytes, length);
            assert!(
                window.copied_bytes <= limit + 9,
                "a window copies the window, not the body: {} bytes for limit {limit}",
                window.copied_bytes
            );
            assert!(window.copied_bytes < window.total_bytes / 4);
        }
    }
    let reasoning = aura_persistence_v2::agent::history::reasoning_window(&sealed, 6, 4096);
    let (text, from, _) = sealed_slice(&thoughts, 6, 4096);
    assert_eq!(reasoning.text, text);
    assert_eq!(reasoning.from, from);
    assert_eq!(reasoning.total_bytes, thoughts.len() as u64);
    assert!(reasoning.copied_bytes <= 4096 + 9);
    // A sealed cursor inside a character starts the page at that character, as a live one does:
    // a reader asking for the last bytes of a body cannot know where its characters begin.
    read.offset = 1;
    let page = desktop_messages::text(db, &read).expect("snapped page");
    assert_eq!((page.from_offset, page.text.chars().next()), (0, Some('有')));
    read.source = AgentMessageTextSource::Reasoning { model_step_id: f.step.model_step_id };
    let page = desktop_messages::text(db, &read).expect("snapped page");
    assert_eq!((page.from_offset, page.text.chars().next()), (0, Some('思')));
}

#[test]
fn a_large_live_body_is_paged_through_windows_that_match_the_whole_body_reader() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let snapshot =
        model::step_snapshot(store.database.connection(), f.step.model_step_id).expect("snapshot");
    let attempt = aura_contract_schema::canonical_fixture()
        .expect("fixture")
        .models
        .transport_attempt
        .expect("transport");
    store
        .database
        .unit_of_work(|tx| model::insert_transport(tx, snapshot.id, &attempt))
        .expect("attempt");
    let event = |sequence, event| ModelStreamEventEnvelope {
        model_step_id: f.step.model_step_id,
        transport_attempt_id: attempt.id,
        sequence,
        received_at: f.at,
        event,
    };
    // Two hundred deltas, so the body is assembled from many rows and a per-page read that
    // concatenated them would touch every one of them.
    let pieces: Vec<String> =
        (0..200).map(|index| format!("第{index}段-有界结果é{}", "ab".repeat(500))).collect();
    let mut sequence = 0_u64;
    for piece in &pieces {
        sequence += 1;
        let value = event(
            sequence,
            ModelStreamEvent::AssistantTextDelta {
                block_id: ModelContentBlockId::new(),
                text: piece.clone(),
            },
        );
        store.database.unit_of_work(|tx| stream::append(tx, &value)).expect("visible delta");
    }
    let db = store.database.connection();
    let whole = stream::partial(db, f.step.model_step_id).expect("whole body");
    // A live body is its deltas concatenated as they arrived, with no separator added.
    assert_eq!(whole.text, pieces.concat());
    assert!(whole.text.len() > 100_000, "the body is large enough to matter");
    let total = whole.text.len() as u64;
    let mut read = AgentMessageTextRequest {
        session_id: f.turn.session_id,
        source: AgentMessageTextSource::Assistant { model_step_id: f.step.model_step_id },
        offset: 0,
        limit: 4096,
    };
    // Walking the pages rebuilds exactly the text the whole-body reader produces.
    let mut rebuilt = String::new();
    loop {
        let page = desktop_messages::text(db, &read).expect("live page");
        let (text, from, next) = live_slice(&whole.text, read.offset, u64::from(read.limit));
        assert_eq!(page.text, text, "offset {}", read.offset);
        assert_eq!(page.from_offset, from);
        assert_eq!(page.next_offset, next);
        assert_eq!(page.total_bytes, total);
        assert_eq!(page.body_id, whole.body_id);
        rebuilt.push_str(&page.text);
        match page.next_offset {
            Some(offset) => read.offset = offset,
            None => break,
        }
    }
    assert_eq!(rebuilt, whole.text);
    for offset in [0_u64, 3, 4095, 4096, 50_003, total - 2, total, total + 32] {
        let windows =
            stream::partial_windows(db, f.step.model_step_id, offset, 4096).expect("window");
        let (text, from, next) = live_slice(&whole.text, offset, 4096);
        assert_eq!(windows.text.text, text, "offset {offset}");
        assert_eq!(windows.text.from, from);
        assert_eq!(windows.text.next, next);
        assert_eq!(windows.text.total_bytes, total);
        assert_eq!(windows.sequence, whole.sequence);
        assert_eq!(windows.body_id, whole.body_id);
        assert!(
            windows.text.copied_bytes <= 4096 + 9,
            "a live window copies the window, not the body: {} bytes",
            windows.text.copied_bytes
        );
        assert!(windows.text.copied_bytes < total / 4);
    }
    // The preview the process area reads is bounded the same way.
    let preview =
        stream::partial_windows(db, f.step.model_step_id, 0, 2048).expect("preview window");
    assert_eq!(preview.text.text, &whole.text[..preview.text.text.len()]);
    assert_eq!(preview.text.total_bytes, total);
    assert!(preview.text.copied_bytes <= 2048 + 9);
}

/// A sealed page reads the chunks it asks for, not the whole response.
///
/// The stored index gives every chunk's length, so a page in the middle of a large body
/// becomes a chunk range: the read fetches those chunks and copies the window out of them.
/// `chunks_read` is the measured number of rows really fetched, so this is a fact about the
/// read rather than a promise about it — and the page still has to equal what the whole-body
/// reader would have returned.
#[test]
fn a_sealed_page_reads_only_the_chunks_it_needs() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    // Ten chunks of text: a page in the middle must not touch the rest of them.
    let body = "有界结果-é".repeat(60_000);
    let mut output = f.template.clone();
    output.model_step_id = f.step.model_step_id;
    output.tool_calls.clear();
    output.assistant_blocks = vec![AssistantContentBlock {
        block_id: ModelContentBlockId::new(),
        text: body.clone(),
        provider_reference: None,
    }];
    output.reasoning_blocks.clear();
    store
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::agent::history::record_output(tx, &output, None, f.at)
        })
        .expect("sealed output");
    let read = AgentMessageTextRequest {
        session_id: f.turn.session_id,
        source: AgentMessageTextSource::Assistant { model_step_id: f.step.model_step_id },
        offset: 300_006,
        limit: 4096,
    };
    let page = {
        let db = store.database.connection();
        desktop_messages::text(db, &read).expect("a page from the middle")
    };
    let (expected, from, next) = live_slice(&body, read.offset, 4096);
    assert_eq!(page.text, expected);
    assert_eq!(page.from_offset, from);
    assert_eq!(page.next_offset, next);
    assert_eq!(page.total_bytes, body.len() as u64);
    assert_eq!(page.body_id, None, "a sealed body has no growing identity");

    // The bounded read is reported by the reader itself.
    let window = {
        let db = store.database.connection();
        aura_persistence_v2::agent::history::visible_body_window(
            db,
            f.step.model_step_id,
            aura_persistence_v2::agent::history::VisibleSource::Assistant,
            read.offset,
            4096,
            &aura_persistence_v2::agent::history::sealed_body(db, f.step.model_step_id)
                .expect("sealed body identity"),
        )
        .expect("chunk read")
        .expect("the step stores a chunk index")
    };
    assert!(
        window.chunks_read <= 2,
        "a 4 KiB page must touch at most the two chunks it spans, not the body: {}",
        window.chunks_read
    );
    assert!(window.copied_bytes <= 4096 + 9);

    // A step whose chunks were never stored has no index, and the reader has to say so
    // rather than invent a body: that is the branch a store written before chunking takes,
    // and the one `text()` answers from the response itself.
    let without_index = {
        let db = store.database.connection();
        // The seal is the one this step really has, so the lookup is refused because no chunk
        // index names the step — not because the identity was made to disagree.
        let seal = aura_persistence_v2::agent::history::sealed_body(db, f.step.model_step_id)
            .expect("sealed body identity");
        aura_persistence_v2::agent::history::visible_body_window(
            db,
            f.step.model_step_id,
            aura_persistence_v2::agent::history::VisibleSource::Assistant,
            0,
            4096,
            &seal,
        )
        .expect("lookup")
    };
    assert!(
        without_index.is_some(),
        "the step whose chunks were stored is readable through its own index"
    );
    // And the same page read through `text()` on the step that does have chunks is the
    // chunked read, not a whole-body one: it returns the identical page.
    let chunked = {
        let db = store.database.connection();
        desktop_messages::text(db, &read).expect("the chunked page")
    };
    assert_eq!(chunked.text, expected, "the stored chunks reproduce the page");
    assert_eq!(chunked.total_bytes, body.len() as u64);
    assert_eq!(chunked.next_offset, next);
}

/// A page of a running thought continues that thought.
///
/// Reasoning is its own source, so a page of it must honour the offset it was asked
/// for. A reader that is told the second page starts where the first one ended has to
/// receive the bytes after that boundary — the same rule the visible text already
/// follows. Answering with the first bytes again would make the reader clear its
/// buffer and show the beginning twice.
#[test]
fn a_running_thought_is_paged_from_its_own_offset() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let snapshot =
        model::step_snapshot(store.database.connection(), f.step.model_step_id).expect("snapshot");
    let attempt = aura_contract_schema::canonical_fixture()
        .expect("fixture")
        .models
        .transport_attempt
        .expect("transport");
    store
        .database
        .unit_of_work(|tx| model::insert_transport(tx, snapshot.id, &attempt))
        .expect("attempt");
    let event = ModelStreamEventEnvelope {
        model_step_id: f.step.model_step_id,
        transport_attempt_id: attempt.id,
        sequence: 1,
        received_at: f.at,
        event: ModelStreamEvent::ReasoningDelta {
            block_id: ModelContentBlockId::new(),
            text: "第一段第二段第三段".into(),
        },
    };
    store.database.unit_of_work(|tx| stream::append(tx, &event)).expect("visible delta");
    let mut read = AgentMessageTextRequest {
        session_id: f.turn.session_id,
        source: AgentMessageTextSource::Reasoning { model_step_id: f.step.model_step_id },
        offset: 0,
        limit: 9,
    };
    let first = desktop_messages::text(store.database.connection(), &read).expect("first page");
    assert_eq!(first.text, "第一段");
    assert_eq!(first.next_offset, Some(9));
    read.offset = first.next_offset.expect("a continuation");
    let second = desktop_messages::text(store.database.connection(), &read).expect("second page");
    assert_eq!(second.from_offset, read.offset, "a page starts where it was asked to");
    assert_eq!(second.text, "第二段");
    assert_eq!(second.total_bytes, 27);
    assert_eq!(second.body_id, first.body_id);
}

/// A finished turn's history poll carries the report's length and a short prefix.
/// The stored report stays intact and is read back a page at a time.
#[test]
fn a_completed_history_page_leaves_the_final_report_out_of_the_poll() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let report = "x".repeat(1_000_000);
    store
        .database
        .unit_of_work(|tx| {
            let mut closed = f.turn.clone();
            closed
                .transition(
                    f.turn.revision,
                    TurnTaskState::Completed { final_report: report.clone() },
                    f.at,
                )
                .expect("complete");
            aura_persistence_v2::agent::turns::save_completed(tx, &f.turn, &closed)?;
            let previous = aura_persistence_v2::agent::session::load(tx, f.turn.session_id)?;
            let mut idle = previous.clone();
            idle.active_turn = None;
            idle.active_lineage = None;
            idle.revision = Revision::new(previous.revision.value() + 1);
            aura_persistence_v2::agent::session::save(tx, &previous, &idle, f.at)
        })
        .expect("seal the turn");
    let view = desktop::projection(
        store.database.connection(),
        &AgentHistoryRequest {
            within_turn: None,
            session_id: f.turn.session_id,
            before_turn_task_id: None,
            limit: 5,
        },
        RuntimeInstanceId::new(),
    )
    .expect("history");
    let encoded = serde_json::to_vec(&view).expect("projection").len();
    assert!(encoded < 64 * 1024, "history poll is {encoded} bytes");
    let summary = view.turns[0].completion.as_ref().expect("completion summary");
    assert_eq!(summary.total_bytes, 1_000_000);
    assert!(summary.preview.len() <= 2048);
    assert!(summary.preview.chars().all(|character| character == 'x'));
    match &view.turns[0].turn.state {
        TurnTaskState::Completed { final_report } => assert!(final_report.is_empty()),
        other => panic!("completed turn, got {other:?}"),
    }
    let page = desktop_messages::text(
        store.database.connection(),
        &AgentMessageTextRequest {
            session_id: f.turn.session_id,
            source: AgentMessageTextSource::Completion { turn_task_id: f.turn.turn_task_id },
            offset: 0,
            limit: 64,
        },
    )
    .expect("report page");
    assert_eq!(page.text, "x".repeat(64));
    assert_eq!(page.total_bytes, 1_000_000);
    assert_eq!(page.sealed_revision, summary.digest);
    // This fixture saves a legacy turn directly, without a completion digest.
    // It remains pageable, but cannot claim a verifiable cache identity.
    assert!(page.sealed_revision.is_none());
    assert_eq!(page.next_offset, Some(64));
    let owner = f.turn.turn_task_id.to_string();
    let mut statement = store
        .database
        .connection()
        .prepare(
            "EXPLAIN QUERY PLAN SELECT text FROM agent_visible_body \
             WHERE owner=?1 AND source='completion' AND ordinal=0",
        )
        .expect("plan");
    let plan = statement
        .query_map([&owner], |row| row.get::<_, String>(3))
        .expect("plan rows")
        .map(|row| row.expect("detail"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(plan.contains("agent_visible_body"), "{plan}");
    assert!(!plan.contains("events"), "{plan}");
}

fn seal_report(store: &mut V2Store, fixture: &support::Fixture, report: String) {
    store
        .database
        .unit_of_work(|tx| {
            let before = aura_persistence_v2::agent::turns::load(tx, fixture.turn.turn_task_id)?;
            let mut done = before.clone();
            done.transition(
                before.revision,
                TurnTaskState::Completed { final_report: report },
                fixture.at,
            )
            .expect("complete");
            aura_persistence_v2::agent::turns::save_completed(tx, &before, &done)?;
            tx.execute("DELETE FROM turn_poll", [])?;
            tx.execute("DELETE FROM agent_visible_body WHERE source='completion'", [])?;
            Ok::<_, PersistenceCoreError>(())
        })
        .expect("seal");
}

/// A 1 MiB report is published across slices. The second slice must not fail the BLOB column.
#[test]
fn legacy_poll_backfill_finishes_a_large_report_across_slices() {
    let mut poll_cursor = desktop::PollBackfillCursor::default();
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    seal_report(&mut store, &fixture, "P".repeat(1_048_576));
    for slice in 0..8 {
        let copied = store
            .database
            .unit_of_work(|tx| desktop::backfill_turn_poll(tx, 262_144, &mut poll_cursor))
            .unwrap_or_else(|error| panic!("slice {slice} failed: {error}"));
        if copied == 0 {
            break;
        }
    }
    let id = fixture.turn.turn_task_id.to_string();
    let published: bool = store
        .database
        .connection()
        .query_row("SELECT EXISTS(SELECT 1 FROM turn_poll WHERE id=?1)", [&id], |row| row.get(0))
        .expect("poll");
    assert!(published, "the poll row is missing after the slice budget");
    let body: i64 = store
        .database
        .connection()
        .query_row(
            "SELECT COALESCE(SUM(byte_len),0) FROM agent_visible_body WHERE owner=?1 AND source='completion'",
            [&id],
            |row| row.get(0),
        )
        .expect("body");
    assert_eq!(body, 1_048_576);
}

/// A capture that already holds the whole turn still writes only one body slice.
#[test]
fn poll_publication_writes_one_report_slice() {
    let mut poll_cursor = desktop::PollBackfillCursor::default();
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let id = fixture.turn.turn_task_id.to_string();
    seal_report(&mut store, &fixture, "P".repeat(1_048_576));
    store
        .database
        .unit_of_work(|tx| {
            tx.execute(
                "INSERT INTO turn_poll_capture(id,revision,state,total_len,copied,payload) \
                 SELECT id, revision, state, length(CAST(payload AS BLOB)), length(CAST(payload AS BLOB)), \
                 CAST(payload AS BLOB) FROM turn_tasks WHERE id=?1",
                [&id],
            )?;
            Ok::<_, PersistenceCoreError>(())
        })
        .expect("checkpoint");
    store
        .database
        .unit_of_work(|tx| desktop::backfill_turn_poll(tx, 262_144, &mut poll_cursor))
        .expect("one slice");
    let body: i64 = store
        .database
        .connection()
        .query_row(
            "SELECT COALESCE(SUM(length(CAST(text AS BLOB))),0) FROM agent_visible_body WHERE source='completion'",
            [],
            |row| row.get(0),
        )
        .expect("written");
    assert!(body <= 262_144, "published {body} body bytes in one call");
    assert!(body > 0);
}

/// A cut inside a multibyte character stays on a character boundary and still reassembles.
#[test]
fn poll_backfill_snaps_utf8_and_reassembles() {
    let mut poll_cursor = desktop::PollBackfillCursor::default();
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let report = "你".repeat(20);
    seal_report(&mut store, &fixture, report.clone());
    let stalled = store
        .database
        .unit_of_work(|tx| desktop::backfill_turn_poll(tx, 1, &mut poll_cursor))
        .expect("one-byte budget");
    assert_eq!(stalled, 0);
    let written: i64 = store
        .database
        .connection()
        .query_row(
            "SELECT COALESCE(SUM(byte_len),0) FROM agent_visible_body WHERE source='completion'",
            [],
            |row| row.get(0),
        )
        .expect("stalled bytes");
    assert_eq!(written, 0, "a one-byte budget must not store a partial character");
    for _ in 0..30 {
        let copied = store
            .database
            .unit_of_work(|tx| desktop::backfill_turn_poll(tx, 5, &mut poll_cursor))
            .expect("slice");
        if copied == 0 {
            break;
        }
    }
    let mut statement = store
        .database
        .connection()
        .prepare("SELECT text FROM agent_visible_body WHERE source='completion' ORDER BY ordinal")
        .expect("body");
    let text = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("rows")
        .map(|row| row.expect("text"))
        .collect::<String>();
    assert_eq!(text, report);
}

/// Closing the store keeps the copied prefix, and the next open continues it.
#[test]
fn poll_backfill_continues_after_restart() {
    let mut poll_cursor = desktop::PollBackfillCursor::default();
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("v2");
    let mut store = V2Store::create(&path).expect("store");
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let id = fixture.turn.turn_task_id.to_string();
    seal_report(&mut store, &fixture, "P".repeat(1_048_576));
    store
        .database
        .unit_of_work(|tx| desktop::backfill_turn_poll(tx, 262_144, &mut poll_cursor))
        .expect("first slice");
    drop(store);
    let mut store = V2Store::open(&path).expect("a backfill fault must not fail open");
    for _ in 0..8 {
        let copied = store
            .database
            .unit_of_work(|tx| desktop::backfill_turn_poll(tx, 262_144, &mut poll_cursor))
            .expect("continue");
        if copied == 0 {
            break;
        }
    }
    let published: bool = store
        .database
        .connection()
        .query_row("SELECT EXISTS(SELECT 1 FROM turn_poll WHERE id=?1)", [&id], |row| row.get(0))
        .expect("poll");
    assert!(published);
    let page = desktop_messages::text(
        store.database.connection(),
        &AgentMessageTextRequest {
            session_id: fixture.turn.session_id,
            source: AgentMessageTextSource::Completion { turn_task_id: fixture.turn.turn_task_id },
            offset: 0,
            limit: 32,
        },
    )
    .expect("page");
    assert_eq!(page.text, "P".repeat(32));
}

/// A revision change drops the in-progress copy and does not rewrite the authority report.
#[test]
fn poll_backfill_drops_a_stale_revision() {
    let mut poll_cursor = desktop::PollBackfillCursor::default();
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let id = fixture.turn.turn_task_id.to_string();
    seal_report(&mut store, &fixture, "P".repeat(1_048_576));
    store
        .database
        .unit_of_work(|tx| desktop::backfill_turn_poll(tx, 262_144, &mut poll_cursor))
        .expect("first slice");
    let old: String = store
        .database
        .connection()
        .query_row(
            "SELECT revision FROM agent_visible_body WHERE owner=?1 AND source='completion' LIMIT 1",
            [&id],
            |row| row.get(0),
        )
        .expect("generation");
    store
        .database
        .connection()
        .execute("UPDATE turn_tasks SET revision=revision+1 WHERE id=?1", [&id])
        .expect("bump");
    store
        .database
        .unit_of_work(|tx| desktop::backfill_turn_poll(tx, 262_144, &mut poll_cursor))
        .expect("restart");
    let stale: i64 = store
        .database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM agent_visible_body WHERE owner=?1 AND revision=?2",
            rusqlite::params![id, old],
            |row| row.get(0),
        )
        .expect("stale rows");
    assert_eq!(stale, 0);
    let report: i64 = store
        .database
        .connection()
        .query_row(
            "SELECT length(CAST(json_extract(payload,'$.state.final_report') AS BLOB)) FROM turn_tasks WHERE id=?1",
            [&id],
            |row| row.get(0),
        )
        .expect("authority");
    assert_eq!(report, 1_048_576);
}

/// The second slice of a non-report payload is a BLOB concatenation, not a type error.
#[test]
fn poll_capture_second_slice_stays_a_blob() {
    let mut poll_cursor = desktop::PollBackfillCursor::default();
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let id = fixture.turn.turn_task_id.to_string();
    let payload = serde_json::json!({ "pad": "q".repeat(600_000) }).to_string();
    store
        .database
        .connection()
        .execute(
            "UPDATE turn_tasks SET state='running', payload=?1 WHERE id=?2",
            rusqlite::params![payload, id],
        )
        .expect("inflate");
    store.database.connection().execute("DELETE FROM turn_poll", []).expect("clear poll");
    for slice in 0..2 {
        store
            .database
            .unit_of_work(|tx| desktop::backfill_turn_poll(tx, 262_144, &mut poll_cursor))
            .unwrap_or_else(|error| panic!("slice {slice} failed: {error}"));
    }
    let copied: i64 = store
        .database
        .connection()
        .query_row("SELECT length(payload) FROM turn_poll_capture WHERE id=?1", [&id], |row| {
            row.get(0)
        })
        .expect("capture");
    assert!(copied > 262_144, "second slice did not extend the blob: {copied}");
}

/// A poll fault does not roll back a stream repair, and opening still succeeds.
#[test]
fn opening_keeps_a_stream_repair_when_poll_backfill_fails() {
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("v2");
    let mut store = V2Store::create(&path).expect("store");
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let owner = fixture.step.model_step_id.to_string();
    let snapshot = model::step_snapshot(store.database.connection(), fixture.step.model_step_id)
        .expect("snapshot");
    let attempt = aura_contract_schema::canonical_fixture()
        .expect("fixture")
        .models
        .transport_attempt
        .expect("transport");
    let payload = serde_json::json!({
        "attempt": attempt.id,
        "sequence": 1,
        "text": "kept",
        "reasoning": "",
    });
    store
        .database
        .unit_of_work(|tx| {
            model::insert_transport(tx, snapshot.id, &attempt)?;
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
            )?;
            tx.execute(
                "INSERT INTO turn_poll_capture(id,revision,state,total_len,copied,payload) \
                 VALUES('missing-turn',1,'completed',8,0,zeroblob(0))",
                [],
            )?;
            Ok::<_, PersistenceCoreError>(())
        })
        .expect("plant");
    drop(store);
    let store = V2Store::open(&path).expect("open");
    let rows: i64 = store
        .database
        .connection()
        .query_row("SELECT COUNT(*) FROM agent_visible_delta WHERE owner=?1", [&owner], |row| {
            row.get(0)
        })
        .expect("delta");
    assert!(rows > 0, "the stream copy must commit even though the poll row cannot");
}

/// A 300000-byte Chinese report crosses a 64 KiB chunk with one byte of room left.
#[test]
fn multibyte_poll_backfill_crosses_chunk_boundaries() {
    let mut poll_cursor = desktop::PollBackfillCursor::default();
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let report = "你".repeat(100_000);
    seal_report(&mut store, &fixture, report.clone());
    for slice in 0..8 {
        let copied = store
            .database
            .unit_of_work(|tx| desktop::backfill_turn_poll(tx, 262_144, &mut poll_cursor))
            .unwrap_or_else(|error| panic!("slice {slice} failed: {error}"));
        if copied == 0 {
            break;
        }
    }
    let id = fixture.turn.turn_task_id.to_string();
    let found: bool = store
        .database
        .connection()
        .query_row("SELECT EXISTS(SELECT 1 FROM turn_poll WHERE id=?1)", [&id], |row| row.get(0))
        .expect("poll");
    assert!(found, "the report must publish after the short chunks");
    let mut rows = store
        .database
        .connection()
        .prepare(
            "SELECT text FROM agent_visible_body WHERE owner=?1 AND source='completion' \
             ORDER BY ordinal",
        )
        .expect("bodies");
    let rebuilt = rows
        .query_map([&id], |row| row.get::<_, String>(0))
        .expect("map")
        .map(|row| row.expect("text"))
        .collect::<String>();
    assert_eq!(rebuilt, report);
    let page = aura_persistence_v2::agent::history::completion_window(
        store.database.connection(),
        fixture.turn.turn_task_id,
        65_532,
        12,
    )
    .expect("window")
    .expect("page");
    assert!(page.text.contains('你'), "a page across the short chunk must stay valid UTF-8");
    assert_eq!(page.total_bytes, report.len() as u64);
}

/// A 4-byte character plus ASCII also closes a short chunk instead of failing.
#[test]
fn multibyte_poll_backfill_closes_a_short_chunk_for_emoji() {
    let mut poll_cursor = desktop::PollBackfillCursor::default();
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let report = "😀x".repeat(60_000);
    seal_report(&mut store, &fixture, report.clone());
    for slice in 0..8 {
        let copied = store
            .database
            .unit_of_work(|tx| desktop::backfill_turn_poll(tx, 262_144, &mut poll_cursor))
            .unwrap_or_else(|error| panic!("slice {slice} failed: {error}"));
        if copied == 0 {
            break;
        }
    }
    let id = fixture.turn.turn_task_id.to_string();
    let mut rows = store
        .database
        .connection()
        .prepare(
            "SELECT byte_len, text FROM agent_visible_body WHERE owner=?1 AND source='completion' \
             ORDER BY ordinal",
        )
        .expect("bodies");
    let pieces = rows
        .query_map([&id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
        .expect("map")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows");
    assert!(pieces.len() > 1, "the body crosses more than one chunk");
    assert!(
        pieces.iter().any(|(len, _)| *len < 65_536),
        "a chunk may stop short of 64 KiB on a character boundary"
    );
    let rebuilt = pieces.into_iter().map(|(_, text)| text).collect::<String>();
    assert_eq!(rebuilt, report);
}

/// An orphan capture must not keep a later completed turn out of turn_poll.
#[test]
fn bad_poll_capture_does_not_starve_other_valid_turns() {
    let mut poll_cursor = desktop::PollBackfillCursor::default();
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    seal_report(&mut store, &fixture, "valid".to_owned());
    store
        .database
        .unit_of_work(|tx| {
            tx.execute(
                "INSERT INTO turn_poll_capture(id,revision,state,total_len,copied,payload) \
                 VALUES('missing-turn',1,'completed',8,0,zeroblob(0))",
                [],
            )?;
            Ok::<_, PersistenceCoreError>(())
        })
        .expect("orphan");
    for _slice in 0..4 {
        let _ = store
            .database
            .unit_of_work(|tx| desktop::backfill_turn_poll(tx, 262_144, &mut poll_cursor));
    }
    let id = fixture.turn.turn_task_id.to_string();
    let found: bool = store
        .database
        .connection()
        .query_row("SELECT EXISTS(SELECT 1 FROM turn_poll WHERE id=?1)", [&id], |row| row.get(0))
        .expect("poll");
    assert!(found, "one failed capture starves later valid turns");
}
