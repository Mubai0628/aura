//! The limits the user sets for sub-agents: how many of one conversation work at once (one
//! waiting for an approval does not count), and the token budget past which a sub-agent takes no
//! more work. Over a real execution store.
#![allow(clippy::unwrap_used)]
use crate::support;

use aura_agent_team::{AgentTeamLimits, Progress, TeamHost, limits};
use aura_domain_common::{ApprovalRequestId, SessionId};
use serde_json::json;
use support::{FakeHost, call, task};

async fn choose(host: &FakeHost, chosen: AgentTeamLimits) {
    let (_, revision) = host.with_store(limits::limits).unwrap();
    host.write("test.limits", move |tx| limits::set_limits(tx, &chosen, revision)).await.unwrap();
}

#[tokio::test]
async fn session_agent_zero_stops_new_temporary_spawns_without_dropping_existing_members() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let team = host.team();
    team.call(call(parent, "agents.spawn", json!({"name":"已有任务","task":task("读代码")})))
        .await
        .unwrap();
    choose(&host, AgentTeamLimits { max_running: 0, ..AgentTeamLimits::DEFAULT }).await;
    assert_eq!(host.with_store(limits::limits).unwrap().0.max_running, 0);
    let refused = team
        .call(call(parent, "agents.spawn", json!({"name":"不能启动","task":task("读代码")})))
        .await
        .unwrap_err();
    assert!(refused.contains("agents.no_slot"), "{refused}");
    let view = team.list(parent).await.unwrap();
    assert_eq!(view.members.len(), 1);
    assert_eq!(view.members[0].session_id, host.child(0));
}

#[tokio::test]
async fn the_users_limit_on_sub_agents_at_once_holds_and_one_waiting_for_approval_holds_no_slot() {
    let host = FakeHost::new();
    choose(&host, AgentTeamLimits { max_running: 1, ..AgentTeamLimits::DEFAULT }).await;
    let parent = SessionId::new();
    let team = host.team();
    team.call(call(parent, "agents.spawn", json!({"name": "任务1", "task": task("读代码")})))
        .await
        .unwrap();
    let refused = team
        .call(call(parent, "agents.spawn", json!({"name": "任务2", "task": task("读代码")})))
        .await
        .unwrap_err();
    assert!(refused.contains("agents.no_slot") && refused.contains("1 个"), "{refused}");
    // A1 waits for the user to approve something: it holds no slot meanwhile.
    host.show(
        host.child(0),
        Progress {
            working: true,
            approvals: vec![ApprovalRequestId::new()],
            ..Progress::default()
        },
    );
    let second = team
        .call(call(parent, "agents.spawn", json!({"name": "任务2", "task": task("读代码")})))
        .await
        .unwrap();
    assert_eq!(second["agent"], "A2");
}

#[tokio::test]
async fn the_limits_are_kept_at_their_revision_and_a_stale_change_is_refused() {
    let host = FakeHost::new();
    assert_eq!(host.with_store(limits::limits).unwrap(), (AgentTeamLimits::DEFAULT, 0));
    let chosen = AgentTeamLimits {
        max_running: 5,
        token_budget: Some(2_000_000),
        minutes_per_task: 30,
        cpu_percent: 50,
    };
    let revision =
        host.write("test.limits", move |tx| limits::set_limits(tx, &chosen, 0)).await.unwrap();
    assert_eq!(revision, 1);
    assert_eq!(host.with_store(limits::limits).unwrap(), (chosen, 1));
    let stale = host
        .write("test.limits", move |tx| limits::set_limits(tx, &AgentTeamLimits::DEFAULT, 0))
        .await;
    assert!(stale.is_err());
    assert_eq!(host.with_store(limits::limits).unwrap(), (chosen, 1));
}

#[tokio::test]
async fn limits_another_version_saved_are_read_for_what_this_one_knows_and_within_range() {
    let host = FakeHost::new();
    host.write("test.limits", |tx| {
        tx.execute(
            "INSERT INTO agent_team_settings(id, revision, payload, updated_at) \
             VALUES(1, 3, '{\"max_running\":40,\"minutes_per_task\":20,\"identities\":[]}', 't')",
            [],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    let (read, revision) = host.with_store(limits::limits).unwrap();
    assert_eq!(revision, 3);
    assert_eq!(
        read,
        AgentTeamLimits {
            max_running: *AgentTeamLimits::RUNNING.end(),
            minutes_per_task: 20,
            ..AgentTeamLimits::DEFAULT
        }
    );
}

#[tokio::test]
async fn a_saved_token_budget_is_kept_and_one_never_saved_or_cleared_is_none() {
    let host = FakeHost::new();
    let saved = |payload: &'static str| {
        let host = &host;
        async move {
            host.write("test.limits", move |tx| {
                tx.execute(
                    "INSERT INTO agent_team_settings(id, revision, payload, updated_at) \
                     VALUES(1, 1, ?1, 't') ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",
                    [payload],
                )?;
                Ok(())
            })
            .await
            .unwrap();
            host.with_store(limits::limits).unwrap().0
        }
    };
    let kept = saved(
        "{\"max_running\":3,\"token_budget\":5000000,\"minutes_per_task\":60,\"cpu_percent\":75}",
    )
    .await;
    assert_eq!((kept.token_budget, kept.minutes_per_task), (Some(5_000_000), 60));
    let cleared = saved(
        "{\"max_running\":3,\"token_budget\":null,\"minutes_per_task\":60,\"cpu_percent\":75}",
    )
    .await;
    assert_eq!(cleared.token_budget, None);
    let older = saved("{\"max_running\":4}").await;
    assert_eq!((older.token_budget, older.minutes_per_task), (None, 120));
}

#[tokio::test]
async fn without_a_token_budget_a_sub_agent_takes_work_however_many_tokens_it_used() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let team = host.team();
    team.call(call(parent, "agents.spawn", json!({"name": "调研", "task": task("读代码")})))
        .await
        .unwrap();
    let child = host.child(0);
    support::answered(&team, child, "好了").await;
    host.show(child, Progress { tokens: 900_000_000, ..Progress::default() });
    let sent = team
        .call(call(parent, "agents.send", json!({"agent": "A1", "text": "再看看测试"})))
        .await
        .unwrap();
    assert_eq!(sent["state"], "running");
}

#[tokio::test]
async fn a_sub_agent_whose_token_budget_is_used_up_takes_no_more_work_until_it_is_raised() {
    let host = FakeHost::new();
    choose(&host, AgentTeamLimits { token_budget: Some(100_000), ..AgentTeamLimits::DEFAULT })
        .await;
    let parent = SessionId::new();
    let team = host.team();
    team.call(call(parent, "agents.spawn", json!({"name": "调研", "task": task("读代码")})))
        .await
        .unwrap();
    let child = host.child(0);
    support::answered(&team, child, "好了").await;
    host.show(child, Progress { tokens: 100_000, ..Progress::default() });
    let refused = team
        .call(call(parent, "agents.send", json!({"agent": "A1", "text": "再看看测试"})))
        .await
        .unwrap_err();
    assert!(refused.contains("agents.budget_used_up"), "{refused}");
    choose(&host, AgentTeamLimits { token_budget: Some(200_000), ..AgentTeamLimits::DEFAULT })
        .await;
    let sent = team
        .call(call(parent, "agents.send", json!({"agent": "A1", "text": "再看看测试"})))
        .await
        .unwrap();
    assert_eq!(sent["state"], "running");
}
