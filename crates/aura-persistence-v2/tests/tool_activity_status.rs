#![allow(clippy::unwrap_used)]
//! The projected execution status must come from the common execution lifecycle,
//! not from a per-Tool special case.
//!
//! `crates/aura-persistence-v2/src/tool/activity.rs` now reports Running for any
//! call whose Attempt has been claimed, and Queued only for a call that is still
//! waiting for scheduling. Before that change only `shell.*` and `web.*` could
//! ever leave Queued, so a file, context or Todo tool that was genuinely executing
//! stayed displayed as queued.
//!
//! What this file can verify through the shared fixture is the Queued half: a call
//! with no claimed Attempt is waiting, and the projection says so. The Running half
//! needs an admitted batch whose `tool.admission_inputs` record exists, because the
//! projection resolves the operation identity from that record — the fixture
//! admits tool batches without writing it, and `tool::admission::record` refuses a
//! snapshot that was not stored first. That half is therefore marked `#[ignore]`
//! with the exact missing precondition rather than being deleted, so it becomes a
//! regression as soon as a fixture can produce a real admission.
#[allow(dead_code)] // Shared transaction fixture also serves completion tests.
#[path = "support/agent.rs"]
mod support;
use aura_agent_domain::AgentTaskMode;
use aura_domain_common::*;
use aura_persistence_v2::{V2Store, tool};
use aura_tool_contract::ToolCallStatusV1;

fn status_of(
    store: &V2Store,
    f: &support::Fixture,
    call: ToolCallId,
) -> (ToolCallStatusV1, Option<UtcTimestamp>) {
    let page = tool::activity::page(
        store.database.connection(),
        &aura_tool_projection::AgentToolActivityRequest {
            session_id: f.turn.session_id,
            before_call_id: None,
            limit: 10,
            call_ids: Some(vec![call]),
        },
    )
    .unwrap();
    let entry = page.calls.first().expect("the requested call is visible");
    (entry.summary.status, entry.summary.started_at)
}

/// Admit one `fs.read` and return the batch and its only call identity.
fn admitted_read(
    store: &mut V2Store,
    f: &support::Fixture,
) -> (aura_persistence_v2::tool::execution::BatchExecution, ToolCallId) {
    // `fs.read` is neither Shell nor Web, so only the common lifecycle can move it
    // out of Queued.
    let batch = f.admit(
        store,
        vec![(
            "fs.read",
            serde_json::json!({
                "path": "notes.txt",
                "range": { "range_kind": "full" },
                "explicit_encoding": null,
            }),
        )],
    );
    let call = batch.batch.calls[0].tool_call_id;
    (batch, call)
}

/// A call with no claimed Attempt is genuinely waiting, and that is what Queued
/// means. This is the state that must not be confused with "Core is working on it".
#[test]
fn a_call_with_no_attempt_is_waiting_and_reports_queued() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&temp.path().join("v2")).unwrap();
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let (_batch, call) = admitted_read(&mut store, &f);

    let (status, started) = status_of(&store, &f, call);
    assert_eq!(
        status,
        ToolCallStatusV1::Queued,
        "nothing has been dispatched, so the call is waiting rather than running"
    );
    assert!(started.is_none(), "a waiting call has no start time");
}

/// Preparation alone is not execution: the call is still waiting its turn.
///
/// Backend selection, readiness checks, an approval wait and a wait for a free execution
/// slot all happen before an Attempt is reserved. Reporting Running from the preparing mark
/// made a queued call look like a running one for as long as that wait lasted, which is the
/// difference this test pins: preparing is an internal timestamp, and the projected status
/// stays Queued until a real slot is activated.
#[test]
fn preparing_a_call_is_not_executing_it() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&temp.path().join("v2")).unwrap();
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let (batch, call) = admitted_read(&mut store, &f);
    let at = f.at;
    store
        .database
        .unit_of_work(|tx| tool::preparing::mark(tx, batch.batch.tool_batch_id, call, at))
        .expect("preparation mark");

    let attempts: i64 = store
        .database
        .connection()
        .query_row("SELECT COUNT(*) FROM execution_attempts", [], |row| row.get(0))
        .expect("attempt count");
    assert_eq!(attempts, 0, "preparation is not an Attempt and claims no capacity");

    let (status, started) = status_of(&store, &f, call);
    assert_eq!(
        status,
        ToolCallStatusV1::Queued,
        "Core working on the call is not the call executing: it is still waiting"
    );
    assert!(started.is_none(), "a call that has not started executing has no start time");

    // The executing fact is what moves it, and it arrives when a real slot is activated.
    store
        .database
        .unit_of_work(|tx| {
            tool::preparing::mark_executing(tx, batch.batch.tool_batch_id, call, f.at)
        })
        .expect("executing mark");
    let (status, started) = status_of(&store, &f, call);
    assert_eq!(status, ToolCallStatusV1::Running, "an activated slot means executing");
    assert_eq!(started, Some(f.at), "and the executing mark is the start fact");
}
/// A non-Shell, non-Web tool moves out of Queued once its Attempt is claimed.
#[test]
fn any_tool_reports_running_once_its_attempt_is_claimed() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&temp.path().join("v2")).unwrap();
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let (batch, call) = admitted_read(&mut store, &f);

    let request = store
        .database
        .unit_of_work(|tx| f.begin_execution(tx, &batch, 0, None))
        .expect("begin execution");
    let recorded: i64 = store
        .database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM execution_attempts WHERE operation_id=?1",
            [request.operation.operation_id.to_string()],
            |row| row.get(0),
        )
        .expect("attempt count");
    assert_eq!(recorded, 1, "the attempt must be durable before the projection is read");
    let admitted = tool::admission::load(store.database.connection(), batch.batch.tool_batch_id)
        .expect("admission load")
        .expect("admission inputs are recorded");
    assert!(admitted.operations.contains_key(&call));

    let (status, started) = status_of(&store, &f, call);
    assert_eq!(
        status,
        ToolCallStatusV1::Running,
        "a claimed attempt means Core is preparing or performing the call"
    );
    assert!(started.is_some(), "the start time comes from the real attempt");
}
