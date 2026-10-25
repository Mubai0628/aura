//! Every read of a conversation wrote its execution-store mirror again, a transaction queued
//! behind every other write even when nothing had changed; it is written only when it differs.
//! And a conversation's tool rows were read holding the store's writer; they are read on its
//! reader, so a long write does not hold them up.
use super::*;
use std::time::Duration;

#[tokio::test]
async fn reading_a_conversation_writes_its_mirror_only_when_it_changed() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let root = host.root.path().join("mirror-workspace");
    std::fs::create_dir(&root).expect("mirror workspace");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"Mirror"}),
    )
    .await;
    let created = crate::r03_tests::success(
        &runtime,
        "session.create",
        json!({"workspace_id":created["workspace"]["workspace_id"],"title":"读"}),
    )
    .await;
    let session: SessionId =
        serde_json::from_value(created["session_id"].clone()).expect("session identity");
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let written = || services.agent_required().store().counters().writes_committed;
    let history = json!(AgentHistoryRequest {
        session_id: session,
        before_turn_task_id: None,
        within_turn: None,
        limit: 10,
    });
    // The first read mirrors the new conversation.
    crate::r03_tests::success(&runtime, "agent.history", history.clone()).await;
    let mirrored = written();
    for _ in 0..3 {
        crate::r03_tests::success(&runtime, "agent.history", history.clone()).await;
        crate::r03_tests::success(&runtime, "agent.inbox", json!({"session_id": session})).await;
    }
    assert_eq!(written(), mirrored, "reading an unchanged conversation writes nothing");

    // A new title still reaches the mirror with the next read.
    crate::r03_tests::success(
        &runtime,
        "session.rename",
        json!({"session_id": session, "expected_title": "读", "title": "改名"}),
    )
    .await;
    crate::r03_tests::success(&runtime, "agent.history", history).await;
    assert_eq!(written(), mirrored + 1, "a changed title is mirrored once");
    let title: String = services
        .broker
        .lock()
        .expect("V2 store")
        .database
        .connection()
        .query_row("SELECT title FROM sessions WHERE id=?1", [session.to_string()], |row| {
            row.get(0)
        })
        .expect("the mirror holds the conversation");
    assert_eq!(title, "改名");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_conversations_tool_rows_are_read_while_the_store_is_being_written() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let root = host.root.path().join("activity-workspace");
    std::fs::create_dir(&root).expect("activity workspace");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"Activity"}),
    )
    .await;
    let created = crate::r03_tests::success(
        &runtime,
        "session.create",
        json!({"workspace_id":created["workspace"]["workspace_id"],"title":"工具"}),
    )
    .await;
    let session: SessionId =
        serde_json::from_value(created["session_id"].clone()).expect("session identity");
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    // The store's writer, held by a plain thread the way a long write holds it.
    let journal = Arc::clone(&services.broker);
    let (held, holding) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let writer = std::thread::spawn(move || {
        let _lock = journal.lock().expect("the store's writer");
        held.send(()).expect("held");
        let _ = released.recv_timeout(Duration::from_secs(20));
    });
    holding.recv_timeout(Duration::from_secs(5)).expect("the writer is really held");
    let page = tokio::time::timeout(
        Duration::from_secs(5),
        crate::r03_tests::success(
            &runtime,
            "agent.tool_activity",
            json!({"session_id": session, "before_call_id": null, "limit": 10, "call_ids": null}),
        ),
    )
    .await;
    release.send(()).expect("release");
    writer.join().expect("writer thread");
    let page = page.expect("the tool rows answer while the store's writer is held");
    assert_eq!(page["calls"], json!([]));
}
