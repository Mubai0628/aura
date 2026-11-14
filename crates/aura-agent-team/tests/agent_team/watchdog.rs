//! The watchdog's rules over running sub-agents: near the token budget the main agent is told
//! once; past the budget, past the time for one task (not counting the wait for an approval), or
//! long without progress, the sub-agent's turn is stopped and a report says why, so its main
//! agent hears of it and the user sees it listed. Over a real execution store.
#![allow(clippy::unwrap_used)]
use crate::support;

use aura_agent_team::{AgentTeamLimits, Progress, TeamHost, Watchdog, limits};
use aura_domain_common::{ApprovalRequestId, SessionId, TurnTaskId};
use serde_json::{Value, json};
use std::time::Duration;
use support::{FakeHost, call, task};

const MINUTE: Duration = Duration::from_secs(60);

async fn choose(host: &FakeHost, chosen: AgentTeamLimits) {
    let (_, revision) = host.with_store(limits::limits).unwrap();
    host.write("test.limits", move |tx| limits::set_limits(tx, &chosen, revision)).await.unwrap();
}

/// A1 of a new conversation, on a turn of its own.
async fn started(host: &FakeHost) -> (SessionId, SessionId, TurnTaskId) {
    let parent = SessionId::new();
    host.team()
        .call(call(parent, "agents.spawn", json!({"name": "调研", "task": task("读代码")})))
        .await
        .unwrap();
    let child = host.child(0);
    let turn = TurnTaskId::new();
    host.show(child, working(turn, 0));
    (parent, child, turn)
}

fn working(turn: TurnTaskId, tokens: u64) -> Progress {
    Progress { working: true, turn: Some(turn), tokens, ..Progress::default() }
}

/// What the main agent's next `agents.wait` hands it.
async fn told(host: &FakeHost, parent: SessionId) -> Vec<Value> {
    let waited = host.team().call(call(parent, "agents.wait", json!({}))).await.unwrap();
    waited["reports"].as_array().cloned().unwrap()
}

async fn listed(host: &FakeHost, parent: SessionId) -> Value {
    serde_json::to_value(host.team().list(parent).await.unwrap()).unwrap()["members"][0].clone()
}

fn summary(report: &Value) -> String {
    report["report"]["summary"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn near_the_token_budget_the_main_agent_is_told_once_and_the_sub_agent_works_on() {
    let host = FakeHost::new();
    choose(&host, AgentTeamLimits { token_budget: Some(100_000), ..AgentTeamLimits::DEFAULT })
        .await;
    let (parent, child, turn) = started(&host).await;
    let dog = Watchdog::default();
    host.show(child, working(turn, 79_000));
    host.team().watch(&dog, MINUTE).await.unwrap();
    assert!(told(&host, parent).await.is_empty());
    host.show(child, working(turn, 81_000));
    host.team().watch(&dog, MINUTE).await.unwrap();
    host.team().watch(&dog, MINUTE).await.unwrap();
    let reports = told(&host, parent).await;
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0]["report"]["status"], "progress");
    let said = summary(&reports[0]);
    assert!(said.contains("80%") && said.contains("agents.send"), "{said}");
    assert!(host.halted.lock().unwrap().is_empty());
    assert_eq!(listed(&host, parent).await["state"], "running");
}

#[tokio::test]
async fn past_the_token_budget_the_sub_agent_is_stopped_once_and_listed_for_the_user() {
    let host = FakeHost::new();
    choose(&host, AgentTeamLimits { token_budget: Some(100_000), ..AgentTeamLimits::DEFAULT })
        .await;
    let (parent, child, turn) = started(&host).await;
    let dog = Watchdog::default();
    host.show(child, working(turn, 100_000));
    host.team().watch(&dog, MINUTE).await.unwrap();
    assert_eq!(*host.halted.lock().unwrap(), vec![child]);
    let note = host.stop_clause(child).unwrap();
    assert!(note.contains("watchdog") && note.contains("used 100000 of 100000"), "{note}");
    let reports = told(&host, parent).await;
    let last = reports.last().unwrap();
    assert_eq!(last["report"]["status"], "failed");
    let said = summary(last);
    assert!(said.contains("预算") && said.contains("100000"), "{said}");
    let member = listed(&host, parent).await;
    assert_eq!((&member["state"], &member["last_report"]), (&json!("reported"), &json!("failed")));
    // Stopped once: the next pass leaves it as it is.
    host.team().watch(&dog, MINUTE).await.unwrap();
    assert_eq!(host.halted.lock().unwrap().len(), 1);
    assert!(told(&host, parent).await.is_empty());
}

#[tokio::test]
async fn the_time_for_one_task_counts_its_work_and_not_its_wait_for_an_approval() {
    let host = FakeHost::new();
    choose(&host, AgentTeamLimits { minutes_per_task: 5, ..AgentTeamLimits::DEFAULT }).await;
    let (parent, child, turn) = started(&host).await;
    let dog = Watchdog::default();
    host.team().watch(&dog, 4 * MINUTE).await.unwrap();
    host.show(child, Progress { approvals: vec![ApprovalRequestId::new()], ..working(turn, 10) });
    host.team().watch(&dog, 30 * MINUTE).await.unwrap();
    assert!(host.halted.lock().unwrap().is_empty(), "the wait for the user is not its time");
    host.show(child, working(turn, 20));
    host.team().watch(&dog, 2 * MINUTE).await.unwrap();
    assert_eq!(*host.halted.lock().unwrap(), vec![child]);
    // The sub-agent's next turn reads who stopped it and why, not that the user did.
    let note = host.stop_clause(child).unwrap();
    assert!(
        note.contains("more than 5 minutes") && note.contains("waiting for approval"),
        "{note}"
    );
    let reports = told(&host, parent).await;
    let said = summary(reports.last().unwrap());
    assert!(said.contains("5 分钟") && said.contains("等审批"), "{said}");
    assert!(said.contains("agents.send"), "{said}");
}

#[tokio::test]
async fn long_without_progress_the_sub_agent_is_stopped_but_never_while_a_tool_runs() {
    let host = FakeHost::new();
    choose(&host, AgentTeamLimits { minutes_per_task: 120, ..AgentTeamLimits::DEFAULT }).await;
    let (parent, child, turn) = started(&host).await;
    let dog = Watchdog::default();
    host.show(child, working(turn, 10));
    host.team().watch(&dog, 10 * MINUTE).await.unwrap();
    host.show(child, Progress { tool: Some("shell.powershell".into()), ..working(turn, 10) });
    host.team().watch(&dog, 30 * MINUTE).await.unwrap();
    // The call ending is progress too: the quiet time counts from there.
    host.show(child, working(turn, 10));
    host.team().watch(&dog, 10 * MINUTE).await.unwrap();
    host.team().watch(&dog, 10 * MINUTE).await.unwrap();
    assert!(host.halted.lock().unwrap().is_empty());
    host.team().watch(&dog, 6 * MINUTE).await.unwrap();
    assert_eq!(*host.halted.lock().unwrap(), vec![child]);
    let note = host.stop_clause(child).unwrap();
    assert!(note.contains("15 minutes with no progress"), "{note}");
    let said = summary(told(&host, parent).await.last().unwrap());
    assert!(said.contains("15 分钟") && said.contains("没有"), "{said}");
}

#[tokio::test]
async fn a_sub_agent_whose_answer_is_still_coming_is_never_stopped_for_no_progress() {
    let host = FakeHost::new();
    let (parent, child, turn) = started(&host).await;
    let dog = Watchdog::default();
    // One long answer: its tokens are counted only once it ends.
    host.show(child, Progress { answering: true, ..working(turn, 10) });
    for _ in 0..4 {
        host.team().watch(&dog, 10 * MINUTE).await.unwrap();
    }
    assert!(host.halted.lock().unwrap().is_empty(), "an answer under way is progress");
    // The answer ended and nothing came after it.
    host.show(child, working(turn, 10));
    host.team().watch(&dog, 10 * MINUTE).await.unwrap();
    assert!(host.halted.lock().unwrap().is_empty());
    host.team().watch(&dog, 6 * MINUTE).await.unwrap();
    assert_eq!(*host.halted.lock().unwrap(), vec![child]);
    let said = summary(told(&host, parent).await.last().unwrap());
    assert!(said.contains("15 分钟") && said.contains("模型没在回答"), "{said}");
}

#[tokio::test]
async fn by_default_tokens_are_not_limited_and_a_task_has_two_hours() {
    let host = FakeHost::new();
    let (parent, child, turn) = started(&host).await;
    let dog = Watchdog::default();
    host.show(child, Progress { tool: Some("fs.write".into()), ..working(turn, 900_000_000) });
    host.team().watch(&dog, 100 * MINUTE).await.unwrap();
    host.team().watch(&dog, 19 * MINUTE).await.unwrap();
    assert!(host.halted.lock().unwrap().is_empty());
    assert!(told(&host, parent).await.is_empty(), "no warning without a budget");
    host.team().watch(&dog, MINUTE).await.unwrap();
    assert_eq!(*host.halted.lock().unwrap(), vec![child]);
    let said = summary(told(&host, parent).await.last().unwrap());
    assert!(said.contains("120 分钟"), "{said}");
}

#[tokio::test]
async fn new_work_after_a_report_starts_the_clock_again() {
    let host = FakeHost::new();
    choose(&host, AgentTeamLimits { minutes_per_task: 5, ..AgentTeamLimits::DEFAULT }).await;
    let (parent, child, _) = started(&host).await;
    let team = host.team();
    let dog = Watchdog::default();
    team.watch(&dog, 4 * MINUTE).await.unwrap();
    support::answered(&team, child, "看完了").await;
    team.watch(&dog, MINUTE).await.unwrap();
    team.call(call(parent, "agents.send", json!({"agent": "A1", "text": "再看看测试"})))
        .await
        .unwrap();
    host.show(child, working(TurnTaskId::new(), 30));
    team.watch(&dog, 4 * MINUTE).await.unwrap();
    assert!(host.halted.lock().unwrap().is_empty(), "the new task has its own five minutes");
    team.watch(&dog, 2 * MINUTE).await.unwrap();
    assert_eq!(*host.halted.lock().unwrap(), vec![child]);
}
