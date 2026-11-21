#![allow(clippy::unwrap_used)]
//! The
//! conversation asks for the activity of every call it shows at once. A call the model wrote whose
//! batch is not admitted yet, or never was because its step was interrupted, has no record; it is
//! left out of the answer instead of failing the read of every call asked with it.
use crate::support;
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

#[test]
fn rejected_shell_and_neighbors_survive_mixed_history_and_reopen() {
    use aura_execution_contract::SideEffectStateV1;
    use aura_tool_contract::{ToolNotExecutedReasonV1, ToolResultOutcomeV1, ToolResultV1};
    use aura_tool_projection::{AgentToolActivityRequest, ToolResultOutcomeSummaryView};
    use serde_json::json;

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("v2");
    let mut store = V2Store::create(&root).unwrap();
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let batch = f.admit(
        &mut store,
        vec![
            ("fs.read", json!({"path":"notes.txt"})),
            (
                "shell.powershell",
                json!({"script":"exit 0","max_output_bytes":6500,"max_output_tokens":7000}),
            ),
            ("shell.cmd", json!({"script":"exit 1","max_output_bytes":"4096"})),
        ],
    );
    let ids: Vec<_> = batch.batch.calls.iter().map(|c| c.tool_call_id).collect();
    for (id, field) in ids[1..].iter().zip(["max_output_tokens", "max_output_bytes"]) {
        let error = ErrorEnvelope::new(
            "tool.schema.arguments_invalid".parse().unwrap(),
            ErrorCategory::Validation,
            format!("Invalid argument /{field}"),
            false,
            CorrelationId::new(),
            None,
        );
        let result = ToolResultV1 {
            tool_call_id: *id,
            outcome: ToolResultOutcomeV1::NotExecuted {
                reason: ToolNotExecutedReasonV1::SchemaValidationFailed,
                error: Some(error),
            },
            side_effect_state: SideEffectStateV1::NotStarted,
            attempts: vec![],
            model_visible_result_ref: None,
            user_audit_result_ref: None,
            internal_diagnostic_ref: None,
            aggregate_side_effect_evidence_ref: None,
            started_at: None,
            completed_at: f.at,
        };
        store
            .database
            .unit_of_work(|tx| {
                tool::execution::complete_call(tx, batch.batch.tool_batch_id, &result)
            })
            .unwrap();
    }
    let check = |store: &V2Store| {
        for call_ids in [Some(ids.clone()), None] {
            let page = tool::activity::page(
                store.database.connection(),
                &AgentToolActivityRequest {
                    session_id: f.turn.session_id,
                    before_call_id: None,
                    limit: 10,
                    call_ids,
                },
            )
            .expect("a rejected shell must not poison the page");
            assert_eq!(page.calls.len(), 3);
            let file = page.calls.iter().find(|c| c.summary.tool_call_id == ids[0]).unwrap();
            assert_eq!(file.display.target.as_deref(), Some("notes.txt"));
            for (id, field) in ids[1..].iter().zip(["max_output_tokens", "max_output_bytes"]) {
                let row = page.calls.iter().find(|c| c.summary.tool_call_id == *id).unwrap();
                assert!(matches!(
                    row.summary.outcome,
                    Some(ToolResultOutcomeSummaryView::NotExecuted {
                        reason: ToolNotExecutedReasonV1::SchemaValidationFailed
                    })
                ));
                assert_eq!(row.summary.side_effect_state, Some(SideEffectStateV1::NotStarted));
                assert!(!row.shell_query_available && !row.summary.can_cancel);
                assert!(row.display.details.iter().any(|line| line.contains(field)));
                let error = aura_persistence_v2::execution::shell::query(
                    store.database.connection(),
                    f.turn.session_id,
                    &aura_windows_user_contract::ShellQueryInput {
                        tool_call_id: *id,
                        stdout_offset: 0,
                        stderr_offset: 0,
                        max_bytes: 64,
                    },
                )
                .expect_err("a refused call has no process details");
                assert!(error.to_string().contains("rejected before execution"));
            }
        }
    };
    check(&store);
    drop(store);
    check(&V2Store::open(&root).unwrap());
}

#[test]
fn unreadable_row_is_distinct_from_absent_and_its_cursor_still_advances() {
    use aura_tool_projection::AgentToolActivityRequest;
    use serde_json::json;
    let temp = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&temp.path().join("v2")).unwrap();
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let batch = f.admit(
        &mut store,
        vec![("fs.read", json!({"path":"old"})), ("fs.list", json!({"path":"."}))],
    );
    let older = batch.batch.calls[0].tool_call_id;
    let unreadable = batch.batch.calls[1].tool_call_id;
    let absent = ToolCallId::new();
    let db = store.database.connection();
    // A stored call that this reader cannot decode is different from an unadmitted model call.
    db.execute("UPDATE tool_calls SET payload='{}' WHERE id=?1", [unreadable.to_string()]).unwrap();
    let mut request = AgentToolActivityRequest {
        session_id: f.turn.session_id,
        before_call_id: None,
        limit: 3,
        call_ids: Some(vec![unreadable, absent, older]),
    };
    let page = tool::activity::page(db, &request).expect("read the healthy neighbor");
    assert_eq!(page.calls.len(), 1);
    assert_eq!(page.calls[0].summary.tool_call_id, older);
    assert_eq!(serde_json::to_value(&page).unwrap()["unreadable_call_ids"], json!([unreadable]));
    request.call_ids = None;
    request.limit = 1;
    let first = tool::activity::page(db, &request).unwrap();
    assert!(first.calls.is_empty());
    assert_eq!(first.next_before_call_id, Some(unreadable));
    request.before_call_id = first.next_before_call_id;
    assert_eq!(tool::activity::page(db, &request).unwrap().calls[0].summary.tool_call_id, older);
}
