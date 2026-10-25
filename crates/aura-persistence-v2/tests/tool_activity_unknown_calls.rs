#![allow(clippy::unwrap_used)]
//! The
//! conversation asks for the activity of every call it shows at once. A call the model wrote whose
//! batch is not admitted yet, or never was because its step was interrupted, has no record; it is
//! left out of the answer instead of failing the read of every call asked with it.
#[allow(dead_code)] // Shared transaction fixture also serves completion tests.
#[path = "support/agent.rs"]
mod support;
use aura_agent_domain::AgentTaskMode;
use aura_domain_common::*;
use aura_persistence_v2::{V2Store, tool};

#[test]
fn a_call_with_no_record_is_left_out_and_the_others_are_read() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&temp.path().join("v2")).unwrap();
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let batch = f.admit(
        &mut store,
        vec![(
            "fs.read",
            serde_json::json!({
                "path": "notes.txt",
                "range": { "range_kind": "full" },
                "explicit_encoding": null,
            }),
        )],
    );
    let stored = batch.batch.calls[0].tool_call_id;
    let unstored = ToolCallId::new();

    let page = tool::activity::page(
        store.database.connection(),
        &aura_tool_projection::AgentToolActivityRequest {
            session_id: f.turn.session_id,
            before_call_id: None,
            limit: 10,
            call_ids: Some(vec![unstored, stored]),
        },
    )
    .expect("one call without a record does not fail the read");
    let read: Vec<ToolCallId> = page.calls.iter().map(|entry| entry.summary.tool_call_id).collect();
    assert_eq!(read, vec![stored], "the stored call is read; the other is simply not there");
}
