//! "@" in a conversation not yet started lists its workspace's files: the composer has the
//! workspace and no session yet, and the file search goes through the real file route anyway.
use super::*;

fn found_paths(value: &Value) -> Vec<String> {
    value["matches"]
        .as_array()
        .expect("matches")
        .iter()
        .filter_map(|entry| entry["path"].as_str().map(str::to_owned))
        .collect()
}

#[tokio::test]
async fn a_new_conversations_file_search_needs_no_session() {
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let (_root, w, s) = workspace(&fixture, &state).await;
    success(&state, method::FS_WRITE_FILE, write_input(&w, s, "notes.txt", "a note")).await;

    let without = success(
        &state,
        method::FS_FIND_PATHS,
        json!({"workspace_id": w.workspace_id, "query": "notes", "limit": 20}),
    )
    .await;
    assert!(found_paths(&without).iter().any(|path| path == "notes.txt"), "{without}");

    let with = success(
        &state,
        method::FS_FIND_PATHS,
        json!({"workspace_id": w.workspace_id, "session_id": s, "query": "notes", "limit": 20}),
    )
    .await;
    assert_eq!(found_paths(&with), found_paths(&without));
}
