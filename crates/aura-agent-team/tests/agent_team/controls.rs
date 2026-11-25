//! Conversation admission preserves running work and the management surface.
#![allow(clippy::unwrap_used)]
use crate::support::{self, FakeHost, call, task};
use aura_agent_team::{AgentTeamLimits, TeamHost, limits};
use aura_domain_common::SessionId;
use serde_json::json;

async fn set_raw_switch(host: &FakeHost, enabled: bool) {
    host.write("test.controls", move |tx| {
        tx.execute(
            "INSERT INTO agent_team_settings(id,revision,payload,updated_at) VALUES(1,1,?1,'t') ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",
            [json!({"enabled":enabled}).to_string()],
        )?;
        Ok(())
    }).await.unwrap();
}

#[tokio::test]
async fn session_agent_parent_off_refuses_new_spawn_and_send_but_keeps_running_reports_and_stop() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let team = host.team();
    team.call(call(parent, "agents.spawn", json!({"name":"已有任务","task":task("读代码")})))
        .await
        .unwrap();
    host.write("test.controls", move |tx| {
        tx.execute("INSERT INTO test_agent_team_blocked_parent VALUES(?1)", [parent.to_string()])?;
        Ok(())
    })
    .await
    .unwrap();
    let spawn = team
        .call(call(parent, "agents.spawn", json!({"name":"不得新派","task":task("读代码")})))
        .await;
    assert!(spawn.is_err(), "closing the parent switch admitted a new spawn: {spawn:?}");
    let send =
        team.call(call(parent, "agents.send", json!({"agent":"A1","text":"不得追加新工作"}))).await;
    assert!(send.is_err(), "closing the parent switch admitted a new send: {send:?}");
    assert_eq!(team.list(parent).await.unwrap().members.len(), 1);
    support::answered(&team, host.child(0), "当前轮次收尾结果").await;
    let result = team.call(call(parent, "agents.result", json!({"agent":"A1"}))).await.unwrap();
    assert!(result.to_string().contains("当前轮次收尾结果"));
    team.stop_for_user(parent, "A1").await.unwrap();
    assert_eq!(team.list(parent).await.unwrap().members.len(), 1);
}

#[tokio::test]
async fn session_agent_saving_legacy_limits_does_not_reenable_the_global_switch() {
    let host = FakeHost::new();
    set_raw_switch(&host, false).await;
    host.write("test.controls.keep", |tx| {
        limits::set_limits(tx, &AgentTeamLimits { max_running: 5, ..AgentTeamLimits::DEFAULT }, 1)
    })
    .await
    .unwrap();
    let kept = host
        .with_store(|db| {
            let value: Option<bool> = db.query_row(
                "SELECT json_extract(payload,'$.enabled') FROM agent_team_settings WHERE id=1",
                [],
                |row| row.get(0),
            )?;
            Ok(value)
        })
        .unwrap();
    assert_eq!(kept, Some(false));
}
