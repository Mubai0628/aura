use super::*;
use aura_context_domain::{AgentContextReadToolResultInput, is_tool_result_reference};

const READS: Scenario = Scenario::Sequence {
    calls: &[
        ("fs.read", r#"{"path":"small.txt","range":{"range_kind":"full"}}"#),
        ("fs.read", r#"{"path":"wide.txt","range":{"range_kind":"full"}}"#),
        ("fs.read", r#"{"path":"last.txt","range":{"range_kind":"full"}}"#),
    ],
};

#[tokio::test]
async fn a_large_arrival_preserves_adopted_results_and_supports_a_bounded_read() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(READS).await;
    let (runtime, session, profile) = waiting::prepare(&host, &provider).await;
    l10_19_compaction::resize(&runtime, profile, 64_000, 4_096).await;
    let work = host.root.path().join("waiting-workspace");
    std::fs::write(work.join("small.txt"), "Already-adopted evidence").expect("small fixture");
    std::fs::write(work.join("last.txt"), "Last file evidence").expect("last fixture");
    let full = "New result introduction\n".to_owned()
        + &"1234567890 abcdefghijklmn 测试内容 9876543210\n".repeat(6_000)
        + "Exact result tail 中文!\n";
    std::fs::write(work.join("wide.txt"), &full).expect("large fixture");

    let run = l10_15_compaction::start(&runtime, session, profile, "Read the three files").await;
    waiting::wait(&runtime, &run, |view| {
        matches!(view.turn.state, TurnTaskState::Completed { .. })
    })
    .await;
    let requests = provider.requests.lock().expect("requests");
    let texts: Vec<Vec<&str>> = requests
        .iter()
        .map(|request| {
            request["messages"]
                .as_array()
                .expect("Chat messages")
                .iter()
                .filter(|message| message["role"] == "tool")
                .map(|message| message["content"].as_str().expect("tool result text"))
                .collect()
        })
        .collect();
    assert_eq!(texts.iter().map(Vec::len).collect::<Vec<_>>(), [0, 1, 2, 3]);
    assert!(texts[1][0].contains("Already-adopted evidence"));
    assert_eq!(texts[2][0], texts[1][0]);
    assert_eq!(texts[3][..2], texts[2]);
    assert!(is_tool_result_reference(texts[2][1]));
    assert!(texts[2][1].contains("New result introduction"));
    assert!(!texts[2][1].contains("Exact result tail"));
    drop(requests);

    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let mut store = services.broker.lock().expect("V2 store");
    let tool_call_id: String = store
        .database
        .connection()
        .query_row("SELECT owner FROM events WHERE kind='context.tool_result_pages'", [], |row| {
            row.get(0)
        })
        .expect("indexed source for the receipt");
    let bytes: u64 = store
        .database
        .connection()
        .query_row(
            "SELECT SUM(byte_len) FROM agent_visible_body WHERE owner=?1 AND source='tool_result'",
            [&tool_call_id],
            |row| row.get::<_, i64>(0),
        )
        .expect("durable result length")
        .try_into()
        .expect("nonnegative durable result length");
    let input = AgentContextReadToolResultInput {
        tool_call_id: tool_call_id.parse().expect("persisted tool call ID"),
        include_input: false,
        offset_bytes: Some(bytes - 64),
        max_bytes: Some(128),
        cursor: None,
    };
    let read = |tx: &rusqlite::Transaction<'_>| {
        aura_persistence_v2::context::read::tool_result(
            tx,
            session,
            run.agent_run_id,
            &input,
            UtcTimestamp::now(),
        )
    };
    let page = store.database.unit_of_work(read).expect("read saved source");
    assert!(page.value.result.len() <= 128);
    assert!(page.value.result.contains("Exact result tail 中文!"));
    assert!(page.next_cursor.is_none());
}
