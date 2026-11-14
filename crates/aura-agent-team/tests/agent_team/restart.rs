//! After a restart of Core: a sub-agent the restart cut off is told to its main agent as
//! interrupted and waits for it to say go on or stop; one whose end the team missed reports it;
//! one that never started is stopped; one that had reported is left alone. A turn that stopped on
//! an error tells the main agent how to go on. Over a real execution store.
#![allow(clippy::unwrap_used)]
use crate::support;

use aura_agent_team::{
    TeamHost, TurnEnd,
    restart::{self, LastTurn},
    store,
};
use aura_domain_common::{SessionId, TurnTaskId, UtcTimestamp};
use serde_json::{Value, json};
use support::{FakeHost, call, task};

async fn spawned(host: &FakeHost) -> (SessionId, SessionId) {
    let parent = SessionId::new();
    host.team()
        .call(call(parent, "agents.spawn", json!({"name": "调研", "task": task("读代码")})))
        .await
        .unwrap();
    (parent, host.child(0))
}

async fn after_restart(host: &FakeHost, child: SessionId, last: LastTurn) {
    host.write("test.restart", move |tx| {
        let member = store::member_by_child(tx, child)?.unwrap();
        restart::after_restart(tx, &member, last, UtcTimestamp::now())
    })
    .await
    .unwrap();
}

async fn told(host: &FakeHost, parent: SessionId) -> Vec<Value> {
    let waited = host.team().call(call(parent, "agents.wait", json!({}))).await.unwrap();
    waited["reports"].as_array().cloned().unwrap()
}

async fn state(host: &FakeHost, parent: SessionId) -> Value {
    let status = host.team().call(call(parent, "agents.status", json!({}))).await.unwrap();
    status["agents"][0]["state"].clone()
}

#[tokio::test]
async fn a_sub_agent_the_restart_cut_off_is_told_as_interrupted_and_goes_on_when_sent_more() {
    let host = FakeHost::new();
    let (parent, child) = spawned(&host).await;
    let turn = TurnTaskId::new();
    after_restart(&host, child, LastTurn::CutOff(turn)).await;
    // Settled again, as the next start would if this one stopped early: nothing more.
    after_restart(&host, child, LastTurn::CutOff(turn)).await;
    let reports = told(&host, parent).await;
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0]["report"]["status"], "blocked");
    let said = reports[0]["report"]["summary"].as_str().unwrap();
    assert!(said.contains("重启") && said.contains("agents.send"), "{said}");
    assert_eq!(state(&host, parent).await, "reported");
    let sent = host
        .team()
        .call(call(parent, "agents.send", json!({"agent": "A1", "text": "接着干"})))
        .await
        .unwrap();
    assert_eq!(sent["state"], "running");
}

#[tokio::test]
async fn a_turn_that_ended_unheard_before_the_restart_reports_its_answer() {
    let host = FakeHost::new();
    let (parent, child) = spawned(&host).await;
    let end = TurnEnd {
        turn_task_id: TurnTaskId::new(),
        outcome: "completed".into(),
        answer: Some("入口在 src/main.rs。".into()),
        reason: None,
    };
    after_restart(&host, child, LastTurn::Ended(end, None)).await;
    let reports = told(&host, parent).await;
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0]["report"]["status"], "ready");
    assert_eq!(reports[0]["report"]["summary"], "入口在 src/main.rs。");
    assert_eq!(reports[0]["by_turn_end"], true);
    assert_eq!(state(&host, parent).await, "reported");
}

#[tokio::test]
async fn a_sub_agent_that_never_started_is_stopped() {
    let host = FakeHost::new();
    let (parent, child) = spawned(&host).await;
    after_restart(&host, child, LastTurn::None).await;
    assert_eq!(state(&host, parent).await, "stopped");
    assert!(told(&host, parent).await.is_empty());
}

#[tokio::test]
async fn a_sub_agent_that_had_reported_is_left_as_it_was() {
    let host = FakeHost::new();
    let (parent, child) = spawned(&host).await;
    support::answered(&host.team(), child, "看完了").await;
    after_restart(&host, child, LastTurn::CutOff(TurnTaskId::new())).await;
    let reports = told(&host, parent).await;
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0]["report"]["status"], "ready");
    assert_eq!(state(&host, parent).await, "reported");
}

#[tokio::test]
async fn a_turn_that_stopped_on_an_error_tells_the_main_agent_how_to_go_on() {
    let host = FakeHost::new();
    let (parent, child) = spawned(&host).await;
    let reason = "agent.provider_failed: server_error (RateLimit, HTTP Some(429))";
    host.team()
        .turn_ended(
            child,
            TurnEnd {
                turn_task_id: TurnTaskId::new(),
                outcome: "recovery_required".into(),
                answer: None,
                reason: Some(reason.into()),
            },
            None,
        )
        .await
        .unwrap();
    let reports = told(&host, parent).await;
    let said = reports[0]["report"]["summary"].as_str().unwrap();
    assert!(said.contains(reason) && said.contains("agents.send"), "{said}");
    assert_eq!(state(&host, parent).await, "reported");
}

/// In the fourth 0.3.0 trial a sub-agent's report said only "(agent.execution_interrupted)".
#[tokio::test]
async fn a_turn_the_provider_cut_off_reports_why_in_words() {
    let host = FakeHost::new();
    let (parent, child) = spawned(&host).await;
    let cause = "模型服务回答到一半时太久没有再发来内容，重试后也一样";
    host.team()
        .turn_ended(
            child,
            TurnEnd {
                turn_task_id: TurnTaskId::new(),
                outcome: "failed".into(),
                answer: None,
                reason: Some("agent.execution_interrupted".into()),
            },
            Some(cause.into()),
        )
        .await
        .unwrap();
    let reports = told(&host, parent).await;
    assert_eq!(reports[0]["report"]["status"], "failed");
    assert_eq!(
        reports[0]["report"]["summary"],
        format!(
            "这一轮没做完就停了：{cause}。要它接着干就用 agents.send，不要了就用 agents.stop。"
        )
    );
}
