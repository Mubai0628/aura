//! `fs.delete` recycles without a second question where the mode allows
//! the deletion, leaves what the bin cannot take, and deleting for good — or from a Shell — asks.
//! The tests' Recycle Bin is a folder beside the file (see `agent_execution::deletion`).
#![allow(clippy::unwrap_used)]
use super::*;
use aura_agent_domain::AgentTaskMode;

async fn run(mode: PermissionMode, calls: &'static [(&'static str, &'static str)]) -> Run {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Sequence { calls }).await;
    let (runtime, session, model) = waiting::prepare_in_mode(&host, &provider, mode).await;
    let root = host.root.path().join("waiting-workspace");
    for name in ["old.txt", "not-recyclable.txt", "keep.txt"] {
        std::fs::write(root.join(name), name).unwrap();
    }
    let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    Run { _host: host, root, runtime, provider, request }
}

struct Run {
    _host: crate::r03_tests::Fixture,
    root: std::path::PathBuf,
    runtime: crate::RuntimeState,
    provider: Provider,
    request: AgentRunRequest,
}

fn finished(view: &AgentRunProjection) -> bool {
    view.turn.state.terminal()
}

fn asking(view: &AgentRunProjection) -> bool {
    matches!(view.run.state, AgentRunState::AwaitingApproval { .. })
}

/// What the model was told about its first call.
fn told(provider: &Provider) -> String {
    let requests = provider.requests.lock().unwrap().clone();
    requests
        .iter()
        .flat_map(|request| request["messages"].as_array().cloned().unwrap_or_default())
        .find(|message| message["role"] == "tool")
        .and_then(|message| message["content"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[tokio::test]
async fn a_deletion_goes_to_the_recycle_bin_without_a_second_question() {
    let run =
        run(PermissionMode::WorkspaceReadWrite, &[("fs.delete", r#"{"path":"old.txt"}"#)]).await;
    waiting::wait(&run.runtime, &run.request, finished).await;
    assert!(!run.root.join("old.txt").exists());
    assert!(run.root.join(".test-recycle-bin").join("old.txt").exists());
    assert!(run.root.join("keep.txt").exists());
    let text = told(&run.provider);
    assert!(text.contains("recycled"), "{text}");
}

#[tokio::test]
async fn what_the_recycle_bin_cannot_take_stays_where_it_is_and_the_model_is_told() {
    let run = run(
        PermissionMode::WorkspaceReadWrite,
        &[("fs.delete", r#"{"path":"not-recyclable.txt"}"#)],
    )
    .await;
    waiting::wait(&run.runtime, &run.request, finished).await;
    assert!(run.root.join("not-recyclable.txt").exists());
    let text = told(&run.provider);
    assert!(text.contains("not_recyclable") && text.contains("permanent"), "{text}");
}

#[tokio::test]
async fn deleting_for_good_asks_the_user_even_in_full_access() {
    let run =
        run(PermissionMode::FullAccess, &[("fs.delete", r#"{"path":"old.txt","permanent":true}"#)])
            .await;
    waiting::wait(&run.runtime, &run.request, asking).await;
    assert!(run.root.join("old.txt").exists(), "nothing is deleted before the user answers");
}

#[tokio::test]
async fn a_shell_command_that_deletes_asks_in_a_workspace_the_model_may_write() {
    let run = run(
        PermissionMode::WorkspaceReadWrite,
        &[("shell.powershell", r#"{"script":"Remove-Item old.txt"}"#)],
    )
    .await;
    waiting::wait(&run.runtime, &run.request, asking).await;
    assert!(run.root.join("old.txt").exists());
}
