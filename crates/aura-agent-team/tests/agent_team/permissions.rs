//! What sub-agents start with: the permission mode the settings give each kind of sub-agent, kept
//! beside the limits. Over a real execution store.
#![allow(clippy::unwrap_used)]
use crate::support;

use aura_agent_team::{AgentTeamLimits, AgentTeamPermissions, TeamHost, limits};
use aura_domain_common::SessionId;
use aura_security_contract::PermissionMode;
use serde_json::json;
use support::{FakeHost, call, task};

#[tokio::test]
async fn the_permissions_are_kept_beside_the_limits_and_saving_the_limits_alone_keeps_them() {
    let host = FakeHost::new();
    assert_eq!(
        host.with_store(limits::settings).unwrap(),
        (AgentTeamLimits::DEFAULT, AgentTeamPermissions::DEFAULT, 0)
    );
    let chosen = AgentTeamPermissions {
        worktree_mode: PermissionMode::FullAccess,
        shared_mode: PermissionMode::WorkspaceReadWrite,
        allow_deletions: true,
        allow_outside_writes: false,
        allow_downloads: true,
        main_agent_approves: true,
    };
    host.write("test.settings", move |tx| {
        limits::set_settings(tx, &AgentTeamLimits::DEFAULT, Some(&chosen), 0)
    })
    .await
    .unwrap();
    let raised = AgentTeamLimits { max_running: 6, ..AgentTeamLimits::DEFAULT };
    host.write("test.limits", move |tx| limits::set_limits(tx, &raised, 1)).await.unwrap();
    assert_eq!(host.with_store(limits::settings).unwrap(), (raised, chosen, 2));
}

#[tokio::test]
async fn settings_saved_before_sub_agents_had_permissions_read_the_defaults() {
    let host = FakeHost::new();
    host.write("test.limits", |tx| {
        tx.execute(
            "INSERT INTO agent_team_settings(id, revision, payload, updated_at) \
             VALUES(1, 3, '{\"max_running\":2,\"token_budget\":null,\"minutes_per_task\":60,\"cpu_percent\":75}', 't')",
            [],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    let (read, permissions, revision) = host.with_store(limits::settings).unwrap();
    assert_eq!((read.max_running, revision), (2, 3));
    assert_eq!(permissions, AgentTeamPermissions::DEFAULT);
}

#[tokio::test]
async fn a_new_sub_agent_starts_in_the_mode_the_settings_give_its_kind() {
    let host = FakeHost::new();
    let chosen = AgentTeamPermissions {
        shared_mode: PermissionMode::WorkspaceReadWrite,
        ..AgentTeamPermissions::DEFAULT
    };
    host.write("test.settings", move |tx| {
        limits::set_settings(tx, &AgentTeamLimits::DEFAULT, Some(&chosen), 0)
    })
    .await
    .unwrap();
    let spawned = host
        .team()
        .call(call(
            SessionId::new(),
            "agents.spawn",
            json!({"name": "读代码", "task": task("读代码")}),
        ))
        .await
        .unwrap();
    assert_eq!(spawned["mode"], "workspace_read_write");
    assert_eq!(host.opened_mode.lock().unwrap()[0].1, PermissionMode::WorkspaceReadWrite);
    let (_, text) = host.last_message();
    assert!(text.contains("你的权限档位是「工作区读写」"), "{text}");
    // fs.delete in its workspace sends to the Recycle Bin and does not ask in this mode; a
    // command's deletion may.
    assert!(text.contains("用 fs.delete 删文件（进回收站）不用问"), "{text}");
}
