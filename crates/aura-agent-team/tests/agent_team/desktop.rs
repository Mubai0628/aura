//! What the user does to a sub-agent from the desktop besides stopping it: its worktree's folder,
//! only while the worktree is in use, and taking back what its commands could do without asking.
//! The list names the workspace each sub-agent works in.
#![allow(clippy::unwrap_used)]
use crate::support;

use aura_agent_team::{
    AgentTeamLimits, AgentTeamPermissions, AllowanceKind, TeamHost, limits, worktree as rows,
};
use aura_domain_common::SessionId;
use serde_json::json;
use support::{FakeHost, call, task};

#[tokio::test]
async fn the_folder_of_a_worktree_in_use_is_given_and_nothing_else() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    host.team()
        .call(call(
            parent,
            "agents.spawn",
            json!({"name": "改登录", "task": task("改登录流程"), "worktree": true}),
        ))
        .await
        .unwrap();
    host.team()
        .call(call(parent, "agents.spawn", json!({"name": "读代码", "task": task("读代码")})))
        .await
        .unwrap();
    let child = host.child(0);
    let worktree = host.with_store(|db| rows::worktree_of(db, child)).unwrap().unwrap();
    let folder = host.team().worktree_folder(parent, "A1").await.unwrap();
    assert_eq!(folder, worktree.details.path);
    // The list names each one's workspace: its worktree's, or the starting conversation's.
    let listed = host.team().list(parent).await.unwrap();
    assert_eq!(listed.members[0].workspace_id, Some(worktree.workspace_id));
    let starting = host.parent(parent).await.unwrap().workspace_id;
    assert_eq!(listed.members[1].workspace_id, Some(starting));

    let shared = host.team().worktree_folder(parent, "A2").await.unwrap_err();
    assert!(shared.to_string().starts_with("agents.no_worktree"), "{shared}");
    host.team().call(call(parent, "agents.discard", json!({"agent": "A1"}))).await.unwrap();
    let discarded = host.team().worktree_folder(parent, "A1").await.unwrap_err();
    assert!(discarded.to_string().starts_with("agents.no_worktree"), "{discarded}");
    let unknown = host.team().worktree_folder(parent, "A9").await.unwrap_err();
    assert!(unknown.to_string().starts_with("agents.unknown_agent"), "{unknown}");
}

#[tokio::test]
async fn an_allowance_taken_back_is_gone_and_the_others_stay() {
    let host = FakeHost::new();
    let chosen = AgentTeamPermissions {
        allow_deletions: true,
        allow_outside_writes: true,
        allow_downloads: true,
        ..AgentTeamPermissions::DEFAULT
    };
    host.write("test.settings", move |tx| {
        limits::set_settings(tx, &AgentTeamLimits::DEFAULT, Some(&chosen), 0)
    })
    .await
    .unwrap();
    let parent = SessionId::new();
    host.team()
        .call(call(
            parent,
            "agents.spawn",
            json!({"name": "改登录", "task": task("改登录流程"), "worktree": true}),
        ))
        .await
        .unwrap();
    let allowed = |host: &FakeHost| {
        let child = host.child(0);
        host.with_store(|db| aura_agent_team::store::member_by_child(db, child))
            .unwrap()
            .unwrap()
            .details
            .allowed
    };
    assert!(allowed(&host).deletions && allowed(&host).outside_writes && allowed(&host).downloads);
    host.team().revoke_allowance(parent, "A1", AllowanceKind::Downloads).await.unwrap();
    let now = allowed(&host);
    assert!(now.deletions && now.outside_writes && !now.downloads, "{now:?}");
    // Taking back what is not given changes nothing.
    host.team().revoke_allowance(parent, "A1", AllowanceKind::Downloads).await.unwrap();
    assert_eq!(allowed(&host), now);
    host.team().revoke_allowance(parent, "A1", AllowanceKind::Deletions).await.unwrap();
    assert!(!allowed(&host).deletions);
    let listed = host.team().list(parent).await.unwrap();
    assert!(listed.members[0].allowed.outside_writes && !listed.members[0].allowed.deletions);
}
