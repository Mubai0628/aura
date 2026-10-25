use aura_domain_common::{HookInvocationId, TurnTaskId, UtcTimestamp};
use aura_hooks::{
    HookEffect, HookEvent, HookInvocationListRequest, HookInvocationView, HookOutcome,
};
use aura_persistence_v2::{V2Store, hooks};
use serde_json::json;

fn view(path: &str, turn: Option<TurnTaskId>, outcome: HookOutcome) -> HookInvocationView {
    HookInvocationView {
        invocation_id: HookInvocationId::new(),
        path: path.into(),
        hook_name: "check".into(),
        hook_version: "1.0.0".into(),
        fingerprint: "f".repeat(64),
        event: HookEvent::ToolBeforeCall,
        session_id: None,
        turn_task_id: turn,
        tool_call_id: None,
        started_at: UtcTimestamp::now(),
        duration_ms: 12,
        outcome,
        failure: None,
        effect: HookEffect::None,
        reason: None,
        summary: None,
        stderr: None,
    }
}

#[test]
fn records_are_listed_newest_first_by_hook_or_turn_and_counted_per_turn() {
    let temp = tempfile::tempdir().expect("temp");
    let mut store = V2Store::create(&temp.path().join("v2")).expect("create");
    let turn = TurnTaskId::new();
    let entries = [
        view("C:/a", Some(turn), HookOutcome::Continued),
        view("C:/b", Some(turn), HookOutcome::Blocked),
        view("C:/a", Some(turn), HookOutcome::Failed),
        view("C:/a", None, HookOutcome::Transformed),
    ];
    store
        .database
        .unit_of_work(|tx| {
            for entry in &entries {
                hooks::record(
                    tx,
                    &hooks::HookInvocationRecord {
                        view: entry,
                        subject: None,
                        input: &"x".repeat(hooks::MAX_STORED_TEXT + 10),
                        output: "{\"decision\":\"continue\"}",
                    },
                )?;
            }
            Ok::<_, aura_persistence_core::PersistenceCoreError>(())
        })
        .expect("recorded");
    let db = store.database.connection();
    let request = |path: Option<&str>, turn: Option<TurnTaskId>| HookInvocationListRequest {
        path: path.map(str::to_owned),
        turn_task_id: turn,
        limit: 10,
    };
    let for_a = hooks::list(db, &request(Some("C:/a"), None)).expect("listed");
    assert_eq!(
        for_a.iter().map(|v| v.invocation_id).collect::<Vec<_>>(),
        [entries[3].invocation_id, entries[2].invocation_id, entries[0].invocation_id]
    );
    assert_eq!(hooks::list(db, &request(None, Some(turn))).expect("turn").len(), 3);
    let statistics = hooks::turn_statistics(db, turn).expect("statistics");
    assert_eq!(
        (statistics.calls, statistics.succeeded, statistics.failed, statistics.blocked),
        (3, 1, 1, 1)
    );
    let stored: i64 = db
        .query_row("SELECT max(length(input)) FROM hook_invocations", [], |row| row.get(0))
        .expect("bounded input");
    assert_eq!(stored, i64::try_from(hooks::MAX_STORED_TEXT).expect("small"));
}

#[test]
fn a_pre_event_decision_is_written_once_and_read_back_for_replay() {
    let temp = tempfile::tempdir().expect("temp");
    let mut store = V2Store::create(&temp.path().join("v2")).expect("create");
    let decision = json!({"kind":"blocked","hook":"check","reason":"不允许"});
    store
        .database
        .unit_of_work(|tx| {
            hooks::save_decision(
                tx,
                "tcl_x",
                HookEvent::ToolBeforeCall,
                &decision,
                UtcTimestamp::now(),
            )
        })
        .expect("saved");
    let db = store.database.connection();
    assert_eq!(
        hooks::decision(db, "tcl_x", HookEvent::ToolBeforeCall).expect("read"),
        Some(decision)
    );
    assert_eq!(hooks::decision(db, "tcl_x", HookEvent::ToolAfterCall).expect("other event"), None);
    assert!(
        store
            .database
            .unit_of_work(|tx| hooks::save_decision(
                tx,
                "tcl_x",
                HookEvent::ToolBeforeCall,
                &json!({}),
                UtcTimestamp::now()
            ))
            .is_err(),
        "a decision is never replaced"
    );
}
