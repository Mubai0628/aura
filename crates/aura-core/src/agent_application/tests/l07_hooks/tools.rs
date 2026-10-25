//! `tool.before_call`, `tool.after_call` and the re-check of changed arguments.
use super::*;

#[tokio::test]
async fn a_before_call_hook_changes_what_is_read_and_is_recorded() {
    let (seen, runtime) = tool_result_seen(&[(
        "redirect",
        &["tool.before_call"],
        &["set-argument", "path", "other.txt"],
    )])
    .await;
    assert!(seen.contains("redirected text") && !seen.contains("original text"), "{seen}");
    let records = crate::r03_tests::success(
        &runtime,
        "agent.hooks.invocations",
        json!({"path": null, "turn_task_id": null, "limit": 10}),
    )
    .await["invocations"]
        .clone();
    assert_eq!(records.as_array().map(Vec::len), Some(1), "{records}");
    assert_eq!(records[0]["outcome"], "transformed");
    assert_eq!(records[0]["event"], "tool.before_call");
}

#[tokio::test]
async fn a_blocked_call_never_runs_and_the_model_is_told_which_hook_stopped_it() {
    let turn = turn_with(&[(
        "guard",
        &["tool.before_call"],
        &["answer", r#"{"decision":"block","reason":"不许读这个文件"}"#],
    )])
    .await;
    let done = wait(&turn.runtime, &turn.request, |p| {
        matches!(p.turn.state, TurnTaskState::Completed { .. })
    })
    .await;
    let seen = last_tool_message(&turn.requests()[1]);
    assert!(seen.contains("被 Hook guard 阻止：不许读这个文件"), "{seen}");
    assert!(!seen.contains("original text"), "the blocked read never ran: {seen}");
    // The user's row says a Hook stopped it, and its details name the Hook and the reason.
    let activity = crate::r03_tests::success(
        &turn.runtime,
        "agent.tool_activity",
        json!({"session_id": turn.request.session_id, "before_call_id": null, "limit": 10}),
    )
    .await;
    let display = &activity["calls"][0]["display"];
    assert_eq!(display["summary"], "未执行：被 Hook 阻止", "{activity}");
    assert!(
        display["details"].as_array().expect("details").iter().any(|line| line
            .as_str()
            .is_some_and(|line| line.contains("被 Hook guard 阻止：不许读这个文件"))),
        "{display}"
    );
    // The turn's totals come with its calls, for the line shown when the turn ends.
    let list = crate::r03_tests::success(
        &turn.runtime,
        "agent.hooks.invocations",
        json!({"path": null, "turn_task_id": done.turn.turn_task_id, "limit": 1}),
    )
    .await;
    assert_eq!(list["invocations"].as_array().map(Vec::len), Some(1), "{list}");
    assert_eq!(
        list["statistics"],
        json!({"calls": 1, "succeeded": 0, "failed": 0, "blocked": 1}),
        "{list}"
    );
    let all = turn.invocations().await;
    assert_eq!(all[0]["effect"], "action_blocked", "{all}");
}

#[tokio::test]
async fn a_changed_call_is_checked_again_and_a_path_outside_the_workspace_asks_first() {
    let outside = std::env::temp_dir().join("aura-l07-outside-never-read.txt");
    std::fs::write(&outside, "outside secret").expect("outside file");
    let outside_path = outside.display().to_string();
    let turn =
        turn_with(&[("escape", &["tool.before_call"], &["set-argument", "path", &outside_path])])
            .await;
    // WorkspaceReadWrite reads its own workspace without asking. The Hook's path is outside
    // it, so the changed call goes through the ordinary checks again and waits for the user.
    wait(&turn.runtime, &turn.request, |p| {
        matches!(p.run.state, AgentRunState::AwaitingApproval { .. })
    })
    .await;
    let requests = turn.provider.requests.lock().expect("requests").clone();
    let _ = std::fs::remove_file(&outside);
    assert_eq!(requests.len(), 1, "nothing was read and the model was not called again");
}

#[tokio::test]
async fn an_after_call_hook_replaces_only_what_the_model_sees() {
    let (seen, runtime) = tool_result_seen(&[(
        "summary",
        &["tool.after_call"],
        &["answer", r#"{"decision":"transform","result":"Hook 摘要"}"#],
    )])
    .await;
    assert_eq!(seen, "Hook 摘要");
    let records = crate::r03_tests::success(
        &runtime,
        "agent.hooks.invocations",
        json!({"path": null, "turn_task_id": null, "limit": 10}),
    )
    .await["invocations"]
        .clone();
    assert_eq!(records[0]["event"], "tool.after_call");
    assert_eq!(records[0]["summary"], "改写了模型看到的工具结果（7 字）");
}
