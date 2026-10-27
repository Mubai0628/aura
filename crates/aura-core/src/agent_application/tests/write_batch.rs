//! A model writes several new files into a folder none of them has created yet, all in one
//! response, as it does when it starts a project. The first write creates the folder; the others
//! still run: a folder that appears along a new file's path after the call was approved does not
//! change what was approved.
use super::*;

const FILES: [&str; 5] = [
    "game/package.json",
    "game/tsconfig.json",
    "game/server/index.mjs",
    "game/server/api.mjs",
    "game/tests/api.test.mjs",
];

const NEW_FOLDER_WRITES: Scenario = Scenario::Batch {
    calls: &[
        (
            "fs.write",
            r#"{"path":"game/package.json","text":"{}\n","expected":{"expected_kind":"absent"}}"#,
        ),
        (
            "fs.write",
            r#"{"path":"game/tsconfig.json","text":"{}\n","expected":{"expected_kind":"absent"}}"#,
        ),
        (
            "fs.write",
            r#"{"path":"game/server/index.mjs","text":"// server\n","expected":{"expected_kind":"absent"}}"#,
        ),
        (
            "fs.write",
            r#"{"path":"game/server/api.mjs","text":"// api\n","expected":{"expected_kind":"absent"}}"#,
        ),
        (
            "fs.write",
            r#"{"path":"game/tests/api.test.mjs","text":"// test\n","expected":{"expected_kind":"absent"}}"#,
        ),
    ],
};

#[tokio::test]
async fn several_new_files_in_a_new_folder_are_all_written_in_one_step() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(NEW_FOLDER_WRITES).await;
    let (runtime, session, model) =
        prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let start = super::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let requests = provider.requests.lock().expect("provider requests").clone();
    let answered = requests.last().expect("the step after the writes");
    let results = answered["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|message| message["role"] == "tool")
        .map(|message| message["content"].to_string())
        .collect::<Vec<_>>();
    assert_eq!(results.len(), FILES.len(), "{results:#?}");
    for result in &results {
        assert!(!result.contains("not started") && !result.contains("did not succeed"), "{result}");
    }
    let root = host.root.path().join("waiting-workspace");
    for path in FILES {
        assert!(root.join(path).is_file(), "{path} was not written: {results:#?}");
    }
}
