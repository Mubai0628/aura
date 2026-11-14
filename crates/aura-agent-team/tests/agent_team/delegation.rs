//! Starting a sub-agent, its reports, waiting for them, sending it more and stopping it, over a
//! real execution store.
#![allow(clippy::unwrap_used)]
use crate::support;

use aura_agent_team::{AgentsWaitInput, TurnEnd};
use aura_domain_common::{SessionId, TurnTaskId};
use serde_json::json;
use std::{sync::atomic::Ordering, time::Duration};
use support::{FakeHost, answered, call, task};

#[tokio::test]
async fn a_sub_agent_starts_on_its_task_sheet_with_read_only_tools_only() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let spawned = host
        .team()
        .call(call(
            parent,
            "agents.spawn",
            json!({"name": "读登录流程", "task": task("找出登录流程")}),
        ))
        .await
        .unwrap();
    assert_eq!(spawned["agent"], "A1");
    assert_eq!(spawned["state"], "running");
    // No model was named and the conversation's own is not allowed: the first allowed one.
    assert_eq!(spawned["model"], "Flash");
    // Not narrowed: every tool a sub-agent may have, in the mode the settings give one sharing
    // the workspace.
    assert!(spawned.get("tools").is_none(), "{spawned}");
    assert_eq!(spawned["mode"], "read_only");
    assert_eq!(
        host.opened_mode.lock().unwrap()[0],
        (host.child(0), aura_security_contract::PermissionMode::ReadOnly)
    );
    let (child, text) = host.last_message();
    assert!(text.contains("## 工作区\n你和主 Agent 共用这个工作区"), "{text}");
    assert!(text.contains("你的权限档位是「只读」"), "{text}");
    assert_eq!(child, host.child(0));
    assert!(text.contains("## 目标\n找出登录流程"), "{text}");
    assert!(text.contains("## 做到什么算完"), "{text}");
    // Its commands run in a group of its own, with a port range other sub-agents do not share.
    assert!(text.contains("AURA_PORT_RANGE"), "{text}");
}

#[tokio::test]
async fn the_same_spawn_call_answers_with_the_sub_agent_it_started() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let once = call(parent, "agents.spawn", json!({"name": "调研", "task": task("看看构建脚本")}));
    let first = host.team().call(once.clone()).await.unwrap();
    let again = host.team().call(once).await.unwrap();
    assert_eq!(first, again);
    assert_eq!(host.opened.lock().unwrap().len(), 1);
    assert_eq!(host.count("SELECT count(*) FROM agent_team_members"), 1);
}

#[tokio::test]
async fn a_task_sheet_naming_another_conversation_is_refused_before_anything_starts() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    for named in
        ["读一下 conv_0123456789abcdef 里说过的方案", "看 ses_01JC3Z8Q4K7M2N5P9R6T0V1W3X 的记录"]
    {
        let refused = host
            .team()
            .call(call(parent, "agents.spawn", json!({"name": "调研", "task": task(named)})))
            .await
            .unwrap_err();
        assert!(refused.contains("agents.task_names_conversation"), "{refused}");
    }
    assert!(host.opened.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_report_reaches_the_main_agent_once_through_wait() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let team = host.team();
    team.call(call(parent, "agents.spawn", json!({"name": "调研", "task": task("找出登录流程")})))
        .await
        .unwrap();
    let child = host.child(0);
    answered(&team, child, "在 src/login.rs").await;
    // A report not yet given to the main agent ends the wait before it starts.
    tokio::time::timeout(Duration::from_secs(5), team.prewait(parent, &AgentsWaitInput::default()))
        .await
        .unwrap()
        .unwrap();
    let waited = team.call(call(parent, "agents.wait", json!({}))).await.unwrap();
    assert_eq!(waited["timed_out"], false);
    assert_eq!(waited["reports"][0]["agent"], "A1");
    assert_eq!(waited["reports"][0]["report"]["summary"], "在 src/login.rs");
    // Given once: the next wait has nothing new, and nobody is left working.
    tokio::time::timeout(Duration::from_secs(5), team.prewait(parent, &AgentsWaitInput::default()))
        .await
        .unwrap()
        .unwrap();
    let again = team.call(call(parent, "agents.wait", json!({}))).await.unwrap();
    assert_eq!(again["timed_out"], true);
    assert_eq!(again["reports"], json!([]));
    // After a final report it makes no more calls until it is sent more.
    let closed = team
        .call(call(child, "agent.report", json!({"status": "progress", "summary": "又看了一眼"})))
        .await
        .unwrap_err();
    assert!(closed.contains("agent.report_closed"), "{closed}");
}

#[tokio::test(start_paused = true)]
async fn a_wait_wakes_when_a_report_comes_in_and_ends_when_its_time_is_up() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let team = host.team();
    team.call(call(parent, "agents.spawn", json!({"name": "调研", "task": task("找出登录流程")})))
        .await
        .unwrap();
    let child = host.child(0);
    let short = AgentsWaitInput { agents: vec!["A1".into()], timeout_seconds: Some(3) };
    let started = tokio::time::Instant::now();
    team.prewait(parent, &short).await.unwrap();
    assert!(started.elapsed() >= Duration::from_secs(3));
    let timed_out =
        team.call(call(parent, "agents.wait", json!({"agents": ["A1"]}))).await.unwrap();
    assert_eq!(timed_out["timed_out"], true);
    assert_eq!(timed_out["working"], json!(["A1"]));
    let long = AgentsWaitInput { agents: vec![], timeout_seconds: Some(600) };
    let started = tokio::time::Instant::now();
    let report = async {
        tokio::time::sleep(Duration::from_secs(20)).await;
        team.call(call(
            child,
            "agent.report",
            json!({"status": "progress", "summary": "读完一半"}),
        ))
        .await
        .unwrap();
    };
    let ((), waited) = tokio::join!(report, team.prewait(parent, &long));
    waited.unwrap();
    assert!(started.elapsed() < Duration::from_secs(60), "{:?}", started.elapsed());
}

#[tokio::test]
async fn sending_more_puts_a_reported_sub_agent_back_to_work_and_a_stopped_one_refuses() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let team = host.team();
    team.call(call(parent, "agents.spawn", json!({"name": "调研", "task": task("找出登录流程")})))
        .await
        .unwrap();
    let child = host.child(0);
    host.working.lock().unwrap().remove(&child);
    team.call(call(
        child,
        "agent.report",
        json!({"status": "waiting", "summary": "两处都像", "question": "看哪一处？"}),
    ))
    .await
    .unwrap();
    let sent = team
        .call(call(parent, "agents.send", json!({"agent": "a1", "text": "看 src/login.rs 那处"})))
        .await
        .unwrap();
    assert_eq!(sent["delivered"], "new_turn");
    assert_eq!(sent["state"], "running");
    assert_eq!(host.last_message(), (child, "来自主 Agent：看 src/login.rs 那处".to_owned()));
    // Back at work: it may report again.
    team.call(call(child, "agent.report", json!({"status": "progress", "summary": "在看"})))
        .await
        .unwrap();
    let stopped = team.call(call(parent, "agents.stop", json!({"agent": "A1"}))).await.unwrap();
    assert_eq!(stopped["state"], "stopped");
    assert_eq!(*host.halted.lock().unwrap(), vec![child]);
    assert_eq!(
        host.stop_clause(child).as_deref(),
        Some("the main agent stopped it with agents.stop")
    );
    let refused = team
        .call(call(parent, "agents.send", json!({"agent": "A1", "text": "继续"})))
        .await
        .unwrap_err();
    assert!(refused.contains("agents.stopped"), "{refused}");
    let report = team
        .call(call(child, "agent.report", json!({"status": "progress", "summary": "好了"})))
        .await
        .unwrap_err();
    assert!(report.contains("agent.report_stopped"), "{report}");
}

#[tokio::test]
async fn three_sub_agents_work_at_once_and_a_fourth_waits_for_a_slot() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let team = host.team();
    for n in 1..=3 {
        let spawned = team
            .call(call(
                parent,
                "agents.spawn",
                json!({"name": format!("任务{n}"), "task": task("读代码")}),
            ))
            .await
            .unwrap();
        assert_eq!(spawned["agent"], format!("A{n}"));
    }
    let refused = team
        .call(call(parent, "agents.spawn", json!({"name": "任务4", "task": task("读代码")})))
        .await
        .unwrap_err();
    assert!(refused.contains("agents.no_slot"), "{refused}");
    answered(&team, host.child(1), "好了").await;
    let fourth = team
        .call(call(parent, "agents.spawn", json!({"name": "任务4", "task": task("读代码")})))
        .await
        .unwrap();
    assert_eq!(fourth["agent"], "A4");
}

#[tokio::test]
async fn a_turn_that_ends_without_a_final_report_reports_its_answer() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let team = host.team();
    team.call(call(parent, "agents.spawn", json!({"name": "调研", "task": task("找出登录流程")})))
        .await
        .unwrap();
    let child = host.child(0);
    let answer = "登录在 src/login.rs。".repeat(10);
    let end = TurnEnd {
        turn_task_id: TurnTaskId::new(),
        outcome: "completed".into(),
        answer: Some(answer.clone()),
        reason: None,
    };
    team.turn_ended(child, end.clone(), None).await.unwrap();
    // The same end recorded again writes nothing more.
    team.turn_ended(child, end, None).await.unwrap();
    let waited = team.call(call(parent, "agents.wait", json!({}))).await.unwrap();
    assert_eq!(waited["reports"].as_array().unwrap().len(), 1);
    assert_eq!(waited["reports"][0]["by_turn_end"], true);
    assert_eq!(waited["reports"][0]["report"]["status"], "ready");
    let page = team
        .call(call(parent, "agents.result", json!({"agent": "A1", "offset": 0, "limit": 20})))
        .await
        .unwrap();
    assert_eq!(page["answer"], answer.chars().take(20).collect::<String>());
    assert_eq!(page["next_offset"], 20);
    assert_eq!(page["report"]["report"]["status"], "ready");
}

/// Done, stuck or failed is said in the turn's final answer, which is the report; agent.report
/// only tells progress or asks a question.
#[tokio::test]
async fn a_sub_agent_says_it_is_done_in_its_final_answer_not_with_agent_report() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let team = host.team();
    team.call(call(parent, "agents.spawn", json!({"name": "调研", "task": task("找出登录流程")})))
        .await
        .unwrap();
    let child = host.child(0);
    for status in ["ready", "blocked", "failed"] {
        let refused = team
            .call(call(child, "agent.report", json!({"status": status, "summary": "好了"})))
            .await
            .unwrap_err();
        assert!(refused.contains("agent.report_final_answer"), "{status}: {refused}");
    }
    let status = team.call(call(parent, "agents.status", json!({}))).await.unwrap();
    assert_eq!(status["agents"][0]["state"], "running");
}

/// A long final answer reaches the main agent as its start and its length; agents.result reads
/// on from there.
#[tokio::test]
async fn a_long_final_answer_reaches_the_main_agent_as_its_start_and_its_length() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let team = host.team();
    team.call(call(parent, "agents.spawn", json!({"name": "调研", "task": task("找出登录流程")})))
        .await
        .unwrap();
    let child = host.child(0);
    let answer = format!("{}{}", "甲".repeat(8_000), "乙".repeat(1_500));
    answered(&team, child, &answer).await;
    let waited = team.call(call(parent, "agents.wait", json!({}))).await.unwrap();
    let summary = waited["reports"][0]["report"]["summary"].as_str().unwrap();
    assert!(summary.starts_with(&"甲".repeat(8_000)), "the answer's start");
    assert!(!summary.contains('乙'), "only its start");
    let tail: String = summary.chars().skip(8_000).collect();
    for part in ["9500", "agents.result", "8000"] {
        assert!(tail.contains(part), "{part}: {tail}");
    }
    let page = team
        .call(call(parent, "agents.result", json!({"agent": "A1", "offset": 8000, "limit": 10})))
        .await
        .unwrap();
    assert_eq!(page["answer"], "乙".repeat(10));
    assert_eq!(page["answer_chars"], 9_500);
    assert!(page["report"].is_null(), "the report came with the first page: {page}");
}

#[tokio::test]
async fn stopping_the_turn_that_started_them_stops_the_running_sub_agents() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let team = host.team();
    for name in ["一", "二"] {
        team.call(call(parent, "agents.spawn", json!({"name": name, "task": task("读代码")})))
            .await
            .unwrap();
    }
    answered(&team, host.child(1), "好了").await;
    team.stop_running(parent).await.unwrap();
    assert_eq!(*host.halted.lock().unwrap(), vec![host.child(0)]);
    let note = host.stop_clause(host.child(0)).unwrap();
    assert!(note.contains("the user stopped the main agent's turn"), "{note}");
    let status = team.call(call(parent, "agents.status", json!({}))).await.unwrap();
    assert_eq!(status["agents"][0]["state"], "stopped");
    assert_eq!(status["agents"][1]["state"], "reported");
}

#[tokio::test]
async fn a_sub_agent_cannot_start_or_steer_sub_agents_and_a_conversation_cannot_report() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let team = host.team();
    team.call(call(parent, "agents.spawn", json!({"name": "调研", "task": task("读代码")})))
        .await
        .unwrap();
    let child = host.child(0);
    for (tool, arguments) in [
        ("agents.spawn", json!({"name": "再派一个", "task": task("读代码")})),
        ("agents.status", json!({})),
        ("agents.stop", json!({"agent": "A1"})),
    ] {
        let refused = team.call(call(child, tool, arguments)).await.unwrap_err();
        assert!(refused.contains("agents.not_for_subagents"), "{tool}: {refused}");
    }
    let refused = team
        .call(call(parent, "agent.report", json!({"status": "progress", "summary": "好了"})))
        .await
        .unwrap_err();
    assert!(refused.contains("agent.report_not_subagent"), "{refused}");
}

#[tokio::test]
async fn a_sub_agent_that_could_not_start_is_stopped() {
    let host = FakeHost::new();
    host.fail_delivery.store(true, Ordering::SeqCst);
    let parent = SessionId::new();
    let refused = host
        .team()
        .call(call(parent, "agents.spawn", json!({"name": "调研", "task": task("读代码")})))
        .await
        .unwrap_err();
    assert!(refused.contains("agents.start_failed"), "{refused}");
    assert_eq!(host.count("SELECT count(*) FROM agent_team_members WHERE state='stopped'"), 1);
}
