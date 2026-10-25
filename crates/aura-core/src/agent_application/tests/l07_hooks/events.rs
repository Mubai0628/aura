//! Turn, session, step, message and completion Hooks in the real model loop.
use super::*;

fn answer(json: &str) -> Vec<&str> {
    vec!["answer", json]
}

#[tokio::test]
async fn a_blocked_turn_never_reaches_the_model_and_says_which_hook_stopped_it() {
    let arguments = answer(r#"{"decision":"block","reason":"今天不开工"}"#);
    let turn = turn_with(&[("gate", &["turn.before_start"], &arguments)]).await;
    let done = wait(&turn.runtime, &turn.request, |p| {
        matches!(&p.turn.state, TurnTaskState::RecoveryRequired { reason } if reason == "hooks.turn_blocked")
    })
    .await;
    assert!(matches!(done.turn.state, TurnTaskState::RecoveryRequired { .. }));
    assert!(turn.requests().is_empty(), "the model was never called");
    let records = turn.invocations().await;
    assert_eq!(records[0]["event"], "turn.before_start");
    assert_eq!(records[0]["reason"], "今天不开工");
    assert_eq!(records[0]["effect"], "action_blocked");
}

#[tokio::test]
async fn injections_reach_the_model_named_and_parameters_are_only_lowered() {
    let inject = answer(r#"{"decision":"transform","inject":"本仓库禁止联网安装依赖"}"#);
    let tune = answer(
        r#"{"decision":"transform","parameters":{"temperature":0.1,"max_output_tokens":64}}"#,
    );
    let turn = turn_with(&[
        ("context-note", &["context.before_build"], &inject),
        ("tune", &["model.before_call"], &tune),
    ])
    .await;
    wait(&turn.runtime, &turn.request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. }))
        .await;
    let requests = turn.requests();
    let first = requests[0].to_string();
    assert!(first.contains("本仓库禁止联网安装依赖"), "{first}");
    assert!(
        first.contains("From Hook context-note 1.0.0 (fingerprint "),
        "the injection names its Hook"
    );
    assert_eq!(requests[0]["temperature"], json!(0.1));
    assert_eq!(requests[0]["max_tokens"], json!(64));
    // Each step asks once; the decision of a step is kept and reused, never asked twice.
    let records = turn.invocations().await;
    let context_calls = records
        .as_array()
        .expect("records")
        .iter()
        .filter(|record| record["event"] == "context.before_build")
        .count();
    assert_eq!(context_calls, requests.len(), "one context call per model step");
}

#[tokio::test]
async fn a_blocked_message_is_refused_and_a_rewritten_one_is_shown_after_the_original() {
    let block = answer(r#"{"decision":"block","reason":"不要在消息里贴密钥"}"#);
    let refused = prepared(&[("guard", &["message.before_accept"], &block)], true).await;
    let response = crate::r03_tests::request(
        &refused.runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: refused.session,
            model_profile_id: refused.model,
            text: "Check the answer and finish".into(),
            mode: AgentTaskMode::Normal,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }),
    )
    .await;
    let text = response.to_string();
    assert!(
        text.contains("hooks.message_blocked") && text.contains("不要在消息里贴密钥"),
        "{text}"
    );
    assert!(refused.provider.requests.lock().expect("requests").is_empty());

    let rewrite = answer(r#"{"decision":"transform","text":"请检查答案后结束（Hook 改写）"}"#);
    let turn = turn_with(&[("rewrite", &["message.before_accept"], &rewrite)]).await;
    wait(&turn.runtime, &turn.request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. }))
        .await;
    let first = turn.requests()[0].to_string();
    assert!(first.contains("Check the answer and finish"), "the original stays for TaskLineage");
    assert!(
        first.contains("请检查答案后结束（Hook 改写）")
            && first.contains("Follow this rewritten version")
    );
}

#[tokio::test]
async fn a_held_answer_goes_back_once_with_guidance_and_then_completes() {
    let once = vec!["block-once", "先说明你读到了什么"];
    let turn = turn_with(&[("review", &["turn.before_complete"], &once)]).await;
    wait(&turn.runtime, &turn.request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. }))
        .await;
    let requests = turn.requests();
    assert_eq!(requests.len(), 3, "read, a held answer sent back, the accepted answer");
    let third = requests[2].to_string();
    assert!(third.contains("did not accept the answer as complete"), "{third}");
    assert!(third.contains("先说明你读到了什么"));
    let records = turn.invocations().await;
    let reviews = records
        .as_array()
        .expect("records")
        .iter()
        .filter(|record| record["event"] == "turn.before_complete")
        .map(|record| record["outcome"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(reviews, ["continued", "blocked"], "newest first: blocked once, then accepted");
}

#[tokio::test]
async fn a_hook_that_never_accepts_stops_the_turn_after_three_tries() {
    let never = answer(r#"{"decision":"block","reason":"还没完成"}"#);
    let turn = turn_with(&[("never", &["turn.before_complete"], &never)]).await;
    wait(&turn.runtime, &turn.request, |p| {
        matches!(&p.turn.state, TurnTaskState::RecoveryRequired { reason } if reason == "hooks.completion_blocked")
    })
    .await;
    assert_eq!(turn.requests().len(), 4, "the read and three answers, then the turn stops");
}

#[tokio::test]
async fn session_start_and_turn_completion_are_each_notified_once() {
    let fine = vec!["continue"];
    let turn = turn_with(&[("notify", &["session.started", "turn.completed"], &fine)]).await;
    wait(&turn.runtime, &turn.request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. }))
        .await;
    // `turn.completed` runs as the worker ends; give it its moment.
    for _ in 0..50 {
        if turn.invocations().await.as_array().is_some_and(|r| r.len() == 2) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let records = turn.invocations().await;
    let mut events = records
        .as_array()
        .expect("records")
        .iter()
        .map(|record| record["event"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    events.sort();
    assert_eq!(events, ["session.started", "turn.completed"]);
}

/// How `session.resumed` shows: after Core restarts, the first turn
/// of a session that already had a turn runs `session.resumed`, never `session.started` again.
#[tokio::test]
async fn after_a_restart_the_sessions_next_turn_is_a_resumed_session() {
    let fine = vec!["continue"];
    let Prepared { host, provider: _provider, runtime, session, model } =
        prepared(&[("notify", &["session.started", "session.resumed"], &fine)], true).await;
    let done = |p: &AgentRunProjection| matches!(p.turn.state, TurnTaskState::Completed { .. });
    let first = start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: first.run.agent_run_id };
    wait(&runtime, &request, done).await;
    let runtime = host.restart(runtime).await;
    let second = start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: second.run.agent_run_id };
    wait(&runtime, &request, done).await;
    let records = crate::r03_tests::success(
        &runtime,
        "agent.hooks.invocations",
        json!({"path": null, "turn_task_id": null, "limit": 50}),
    )
    .await["invocations"]
        .clone();
    let mut events = records
        .as_array()
        .expect("records")
        .iter()
        .map(|record| record["event"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    events.sort();
    assert_eq!(events, ["session.resumed", "session.started"], "{records}");
}

#[tokio::test]
async fn a_failed_call_is_offered_to_on_error_and_only_its_view_changes() {
    let explain = answer(r#"{"decision":"transform","error":"文件不存在（Hook 说明：先列目录）"}"#);
    let turn = started(prepared(&[("explain", &["tool.on_error"], &explain)], false).await).await;
    wait(&turn.runtime, &turn.request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. }))
        .await;
    assert_eq!(last_tool_message(&turn.requests()[1]), "文件不存在（Hook 说明：先列目录）");
}

#[tokio::test]
async fn compaction_keeps_what_a_hook_asks_and_reports_when_it_is_done() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Compact).await;
    let (runtime, session, model) =
        prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let focus = answer(r#"{"decision":"transform","focus":"保留关于测试命令的结论"}"#);
    let done = vec!["continue"];
    hook(&runtime, host.root.path(), "keep", &["context.before_compact"], &focus).await;
    hook(&runtime, host.root.path(), "after", &["context.after_compact"], &done).await;
    let run = start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: run.run.agent_run_id };
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let requests = provider.requests.lock().expect("requests").clone();
    let compactions = requests
        .iter()
        .filter(|body| body.to_string().contains("Return exactly these headings in this order:"))
        .collect::<Vec<_>>();
    assert!(!compactions.is_empty(), "the scenario compacts");
    assert!(
        compactions.iter().all(|body| body.to_string().contains("保留关于测试命令的结论")),
        "every attempt of the compaction carries the Hook's focus"
    );
    let records = crate::r03_tests::success(
        &runtime,
        "agent.hooks.invocations",
        json!({"path": null, "turn_task_id": null, "limit": 50}),
    )
    .await["invocations"]
        .clone();
    let events = records
        .as_array()
        .expect("records")
        .iter()
        .map(|record| record["event"].as_str().unwrap_or_default().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(events.contains("context.before_compact") && events.contains("context.after_compact"));
}

#[tokio::test]
async fn an_after_model_hook_changes_only_what_later_steps_see_of_that_reply() {
    let note = answer(r#"{"decision":"transform","text":"（Hook 补充：先读文件再回答）"}"#);
    let turn = turn_with(&[("annotate", &["model.after_call"], &note)]).await;
    wait(&turn.runtime, &turn.request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. }))
        .await;
    let requests = turn.requests();
    let assistant = requests[1]["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .find(|message| message["role"] == "assistant")
        .expect("the reply that called the tool")
        .to_string();
    assert!(assistant.contains("（Hook 补充：先读文件再回答）"), "{assistant}");
    // Only the reply that led to tools is offered; the final answer is the user's.
    let records = turn.invocations().await;
    let offered = records
        .as_array()
        .expect("records")
        .iter()
        .filter(|record| record["event"] == "model.after_call")
        .count();
    assert_eq!(offered, 1);
}
