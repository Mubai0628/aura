//! Unchanged checkpoint instructions and existing messages form an intact request prefix.
//! The task's state the model reads carries what it acts on, not Core's per-step bookkeeping.
use super::*;

#[tokio::test]
async fn a_turns_steps_share_the_complete_prefix_without_bookkeeping() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Search).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    // What the scripted search finds.
    std::fs::write(
        host.root.path().join("waiting-workspace").join("search-fixture.txt"),
        concat!("unique_fs_search_agent_token", "\n"),
    )
    .expect("search fixture");
    let start = super::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let requests = provider.requests.lock().expect("provider requests").clone();
    // The last two steps come after the tool set settled: one runs the search, one answers.
    let [.., searching, answering] = requests.as_slice() else {
        panic!("a search turn takes several steps: {}", requests.len());
    };
    // The tool definitions come before the conversation in the provider's prompt too.
    assert_eq!(
        searching["tools"], answering["tools"],
        "the tool definitions changed between steps"
    );
    let messages = |request: &Value| request["messages"].as_array().expect("messages").clone();
    let (before, after) = (messages(searching), messages(answering));
    assert_eq!(before[0]["role"], "system");
    // With the web tools loaded, the instructions carry how to use them; the module named a
    // capability nothing produced before.
    let instructions = before[0]["content"].as_str().expect("instructions");
    assert!(instructions.contains("pass mode `http_only`"), "the web module is not active");
    assert!(after.starts_with(&before), "a settled step changed previously sent messages");
    let (_, state) = instructions.split_once("<Current-Task-State>").expect("task state");
    let (state, _) = state.split_once("</Current-Task-State>").expect("end of task state");
    for per_step in ["state_hash", "state_revision", "registry_snapshot_ref", "\"continuation\""] {
        assert!(!state.contains(per_step), "{per_step} reaches the model");
    }
    for kept in ["lineages", "permission_mode", "\"turn\""] {
        assert!(state.contains(kept), "{kept} is missing from the task state");
    }
}
