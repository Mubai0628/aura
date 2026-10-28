//! A model writes several new files into a folder none of them has created yet, all in one
//! response, as it does when it starts a project. The first write creates the folder; the others
//! still run: a folder that appears along a new file's path after the call was approved does not
//! change what was approved, at whichever check before its start the call sees it.
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

const ONE_NEW_FOLDER_WRITE: Scenario = Scenario::Batch {
    calls: &[(
        "fs.write",
        r#"{"path":"game/server/api.mjs","text":"// api\n","expected":{"expected_kind":"absent"}}"#,
    )],
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
    all_written(&host, &provider);
}

/// The folder of a new file appears after the write was prepared and before it is dispatched:
/// another write of the same response created it, as calls that run together do on a busy
/// machine. The write still runs.
#[tokio::test]
async fn a_folder_that_appears_after_a_new_file_was_prepared_does_not_stop_it() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(ONE_NEW_FOLDER_WRITE).await;
    let (runtime, session, model) =
        prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    crate::agent_execution::CREATE_BEFORE_RECHECK
        .lock()
        .expect("folders due")
        .push((session, WorkspacePath::parse("game/server").expect("a workspace path")));
    let start = super::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let results = results(&provider);
    assert_eq!(results.len(), 1, "{results:#?}");
    assert!(!results[0].contains("not started"), "{}", results[0]);
    let file = host.root.path().join("waiting-workspace").join("game/server/api.mjs");
    assert!(file.is_file(), "the file was not written: {}", results[0]);
}

/// The tool results the model got back last.
fn results(provider: &Provider) -> Vec<String> {
    let requests = provider.requests.lock().expect("provider requests").clone();
    let answered = requests.last().expect("the step after the writes");
    answered["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|message| message["role"] == "tool")
        .map(|message| message["content"].to_string())
        .collect()
}

/// Every write of the response ran and its file is there.
fn all_written(host: &crate::r03_tests::Fixture, provider: &Provider) {
    let results = results(provider);
    assert_eq!(results.len(), FILES.len(), "{results:#?}");
    for result in &results {
        assert!(!result.contains("not started") && !result.contains("did not succeed"), "{result}");
    }
    let root = host.root.path().join("waiting-workspace");
    for path in FILES {
        assert!(root.join(path).is_file(), "{path} was not written: {results:#?}");
    }
}
