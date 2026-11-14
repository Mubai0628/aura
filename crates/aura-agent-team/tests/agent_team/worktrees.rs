//! Sub-agents that change files, each in a worktree of its own: starting one, merging and
//! discarding its worktree, what it changed, files two of them changed, a conversation of the
//! user's that an earlier version started in a worktree, and the worktrees going with the
//! conversation.
#![allow(clippy::unwrap_used)]
use crate::support;

use aura_agent_team::{
    AgentsWaitInput, Report, ReportStatus, TeamHost, Worktree, WorktreeChange, WorktreeChangeKind,
    WorktreeDetails, WorktreeMerge, WorktreeState, store, worktree as rows,
};
use aura_domain_common::{SessionId, TurnTaskId, UtcTimestamp, WorkspaceId};
use serde_json::json;
use std::sync::atomic::Ordering;
use support::{FakeHost, call, task};

fn change(path: &str) -> WorktreeChange {
    WorktreeChange {
        path: path.to_owned(),
        kind: WorktreeChangeKind::Modified,
        added: Some(2),
        removed: Some(1),
    }
}

async fn spawn_writer(host: &FakeHost, parent: SessionId, name: &str) -> serde_json::Value {
    host.team()
        .call(call(
            parent,
            "agents.spawn",
            json!({"name": name, "task": task("改登录流程"), "worktree": true}),
        ))
        .await
        .unwrap()
}

/// Ends `child`'s turn with its final answer, which is its report.
async fn ready(host: &FakeHost, child: SessionId) {
    support::answered(&host.team(), child, "改好了：src/login.rs；cargo test -p login 退出码 0。")
        .await;
}

fn worktree_of(host: &FakeHost, child: SessionId) -> aura_agent_team::Worktree {
    host.with_store(|db| rows::worktree_of(db, child)).unwrap().expect("worktree row")
}

#[tokio::test]
async fn a_writing_sub_agent_works_in_a_worktree_of_its_own_with_the_file_tools() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let spawned = spawn_writer(&host, parent, "改登录").await;
    let added = host.added.lock().unwrap().clone();
    assert_eq!(added.len(), 1);
    let (from, plan, own) = &added[0];
    assert_eq!(*from, host.workspace);
    assert!(plan.branch.starts_with("aura/agent/"), "{plan:?}");
    assert_eq!(spawned["branch"], plan.branch.as_str());
    // Its conversation is in the worktree's own workspace, not the parent's.
    let child = host.child(0);
    assert_eq!(host.opened_in.lock().unwrap()[0], (child, Some(*own)));
    // Every tool, in the mode the settings give a sub-agent with a worktree.
    assert!(spawned.get("tools").is_none(), "{spawned}");
    assert_eq!(spawned["mode"], "workspace_read_write");
    assert_eq!(
        host.opened_mode.lock().unwrap()[0],
        (child, aura_security_contract::PermissionMode::WorkspaceReadWrite)
    );
    let worktree = worktree_of(&host, child);
    assert_eq!(worktree.workspace_id, *own);
    assert_eq!(worktree.parent_session_id, Some(parent));
    assert_eq!(worktree.parent_workspace_id, host.workspace);
    assert_eq!(worktree.state, WorktreeState::Active);
    assert_eq!(worktree.details.branch, plan.branch);
    assert_eq!(worktree.details.base, plan.start);
    let (_, sheet) = host.last_message();
    assert!(sheet.contains(&plan.branch), "{sheet}");
}

#[tokio::test]
async fn the_same_spawn_call_again_adds_no_second_worktree() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let once = call(
        parent,
        "agents.spawn",
        json!({"name": "改登录", "task": task("改登录流程"), "worktree": true}),
    );
    let first = host.team().call(once.clone()).await.unwrap();
    let again = host.team().call(once).await.unwrap();
    assert_eq!(first, again);
    assert_eq!(host.added.lock().unwrap().len(), 1);
    assert_eq!(host.count("SELECT count(*) FROM agent_worktrees"), 1);
}

#[tokio::test]
async fn a_workspace_that_is_not_a_repository_gets_only_read_only_sub_agents() {
    let host = FakeHost::new();
    host.git.store(false, Ordering::SeqCst);
    let refused = host
        .team()
        .call(call(
            SessionId::new(),
            "agents.spawn",
            json!({"name": "改登录", "task": task("改登录流程"), "worktree": true}),
        ))
        .await
        .unwrap_err();
    assert!(refused.contains("agents.not_git"), "{refused}");
    assert!(host.added.lock().unwrap().is_empty());
    assert!(host.opened.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_worktree_that_cannot_be_placed_starts_nothing() {
    let host = FakeHost::new();
    *host.plan_refusal.lock().unwrap() =
        Some("agents.disk_low: D: 只剩 60 GB，低于保留的 80 GB".to_owned());
    let refused = host
        .team()
        .call(call(
            SessionId::new(),
            "agents.spawn",
            json!({"name": "改登录", "task": task("改登录流程"), "worktree": true}),
        ))
        .await
        .unwrap_err();
    assert!(refused.contains("agents.disk_low"), "{refused}");
    assert!(host.opened.lock().unwrap().is_empty());
    assert_eq!(host.count("SELECT count(*) FROM agent_team_members"), 0);
}

#[tokio::test]
async fn when_its_conversation_cannot_open_the_worktree_goes_again() {
    let host = FakeHost::new();
    host.fail_open.store(true, Ordering::SeqCst);
    let refused = host
        .team()
        .call(call(
            SessionId::new(),
            "agents.spawn",
            json!({"name": "改登录", "task": task("改登录流程"), "worktree": true}),
        ))
        .await
        .unwrap_err();
    assert!(refused.contains("session.create_failed"), "{refused}");
    let added = host.added.lock().unwrap().clone();
    assert_eq!(*host.removed.lock().unwrap(), vec![added[0].1.path.clone()]);
    assert_eq!(host.count("SELECT count(*) FROM agent_worktrees"), 0);
}

#[tokio::test]
async fn a_running_sub_agent_is_not_merged() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    spawn_writer(&host, parent, "改登录").await;
    let refused =
        host.team().call(call(parent, "agents.merge", json!({"agent": "A1"}))).await.unwrap_err();
    assert!(refused.contains("agents.still_running"), "{refused}");
    assert!(host.merged.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_merged_worktree_is_removed_and_the_files_it_brought_are_named() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    spawn_writer(&host, parent, "改登录").await;
    let child = host.child(0);
    host.changes.lock().unwrap().insert(child, vec![change("src/login.rs")]);
    ready(&host, child).await;
    let merged =
        host.team().call(call(parent, "agents.merge", json!({"agent": "A1"}))).await.unwrap();
    assert_eq!(merged["merged"], true);
    assert_eq!(merged["files"], json!(["src/login.rs"]));
    let worktree = worktree_of(&host, child);
    assert_eq!(*host.removed.lock().unwrap(), vec![worktree.details.path.clone()]);
    assert_eq!(worktree.state, WorktreeState::Merged);
    // Merged once: the same request again finds nothing to merge.
    let again =
        host.team().call(call(parent, "agents.merge", json!({"agent": "A1"}))).await.unwrap_err();
    assert!(again.contains("agents.worktree_closed"), "{again}");
}

#[tokio::test]
async fn a_worktree_that_took_the_uncommitted_files_along_says_so_and_starts_after_them() {
    let host = FakeHost::new();
    *host.carried.lock().unwrap() =
        vec!["mini-game/stardust-breakout.html".to_owned(), "README.md".to_owned()];
    let parent = SessionId::new();
    spawn_writer(&host, parent, "改游戏").await;
    let child = host.child(0);
    assert_eq!(worktree_of(&host, child).details.base, support::CARRIED_COMMIT);
    let (_, sheet) = host.last_message();
    assert!(sheet.contains("还没提交的 2 个文件改动"), "{sheet}");
    assert!(sheet.contains(&support::CARRIED_COMMIT[..8]), "{sheet}");
}

#[tokio::test]
async fn changes_written_into_the_workspace_files_without_a_commit_are_named_and_said() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    spawn_writer(&host, parent, "改游戏").await;
    let child = host.child(0);
    ready(&host, child).await;
    *host.merge_outcome.lock().unwrap() =
        Some(WorktreeMerge::Applied { files: vec!["mini-game/stardust-breakout.html".to_owned()] });
    let merged =
        host.team().call(call(parent, "agents.merge", json!({"agent": "A1"}))).await.unwrap();
    assert_eq!(merged["merged"], true);
    assert_eq!(merged["head"], serde_json::Value::Null);
    assert_eq!(merged["files"], json!(["mini-game/stardust-breakout.html"]));
    let note = merged["note"].as_str().unwrap();
    assert!(note.contains("没有提交") && note.contains("原有的改动都还在"), "{note}");
    // Its work is in the workspace now: the worktree goes.
    let worktree = worktree_of(&host, child);
    assert_eq!(worktree.state, WorktreeState::Merged);
    assert_eq!(*host.removed.lock().unwrap(), vec![worktree.details.path]);
}

#[tokio::test]
async fn a_clash_with_the_workspace_uncommitted_changes_keeps_everything_as_it_was() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    spawn_writer(&host, parent, "改游戏").await;
    let child = host.child(0);
    ready(&host, child).await;
    *host.merge_outcome.lock().unwrap() =
        Some(WorktreeMerge::Clashed { files: vec!["src/login.rs".to_owned()] });
    let result =
        host.team().call(call(parent, "agents.merge", json!({"agent": "A1"}))).await.unwrap();
    assert_eq!(result["merged"], false);
    assert_eq!(result["conflicts"], json!(["src/login.rs"]));
    let note = result["note"].as_str().unwrap();
    assert!(note.contains("没提交的改动") && note.contains("A1"), "{note}");
    assert!(host.removed.lock().unwrap().is_empty());
    assert_eq!(worktree_of(&host, child).state, WorktreeState::Active);
}

#[tokio::test]
async fn a_conflicting_merge_keeps_the_worktree_and_names_the_files() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    spawn_writer(&host, parent, "改登录").await;
    let child = host.child(0);
    ready(&host, child).await;
    *host.merge_outcome.lock().unwrap() =
        Some(WorktreeMerge::Conflicted { files: vec!["src/login.rs".to_owned()] });
    let result =
        host.team().call(call(parent, "agents.merge", json!({"agent": "A1"}))).await.unwrap();
    assert_eq!(result["merged"], false);
    assert_eq!(result["conflicts"], json!(["src/login.rs"]));
    assert!(host.removed.lock().unwrap().is_empty());
    assert_eq!(worktree_of(&host, child).state, WorktreeState::Active);
}

#[tokio::test]
async fn discarding_stops_a_running_sub_agent_and_removes_its_worktree() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    spawn_writer(&host, parent, "改登录").await;
    let child = host.child(0);
    let result =
        host.team().call(call(parent, "agents.discard", json!({"agent": "A1"}))).await.unwrap();
    assert_eq!(result["discarded"], true);
    assert!(host.halted.lock().unwrap().contains(&child));
    let note = host.stop_clause(child).unwrap();
    assert!(note.contains("agents.discard"), "{note}");
    let worktree = worktree_of(&host, child);
    assert_eq!(worktree.state, WorktreeState::Discarded);
    assert_eq!(*host.removed.lock().unwrap(), vec![worktree.details.path]);
    let status = host.team().call(call(parent, "agents.status", json!({}))).await.unwrap();
    assert_eq!(status["agents"][0]["state"], "stopped");
}

#[tokio::test]
async fn files_two_sub_agents_changed_are_told_to_the_main_agent() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    spawn_writer(&host, parent, "改登录").await;
    spawn_writer(&host, parent, "改注册").await;
    let (first, second) = (host.child(0), host.child(1));
    host.changes.lock().unwrap().insert(first, vec![change("src/lib.rs"), change("src/login.rs")]);
    host.changes
        .lock()
        .unwrap()
        .insert(second, vec![change("src/lib.rs"), change("src/signup.rs")]);
    let status = host.team().call(call(parent, "agents.status", json!({}))).await.unwrap();
    assert_eq!(status["overlaps"], json!(["A1、A2 都改了 src/lib.rs，合回时可能冲突"]));
    assert_eq!(status["agents"][0]["branch"], worktree_of(&host, first).details.branch.as_str());
    let wait = AgentsWaitInput { agents: vec![], timeout_seconds: Some(1) };
    let waited = host
        .team()
        .call(call(parent, "agents.wait", serde_json::to_value(wait).unwrap()))
        .await
        .unwrap();
    assert_eq!(waited["overlaps"], status["overlaps"]);
}

#[tokio::test]
async fn the_list_and_the_changes_show_the_worktree_and_the_newest_checks() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    spawn_writer(&host, parent, "改登录").await;
    let child = host.child(0);
    host.changes.lock().unwrap().insert(child, vec![change("src/login.rs")]);
    ready(&host, child).await;
    let list = host.team().list(parent).await.unwrap();
    let shown = list.members[0].worktree.clone().expect("its worktree");
    assert_eq!(shown.branch, worktree_of(&host, child).details.branch);
    assert_eq!(shown.state, WorktreeState::Active);
    assert_eq!(shown.size_bytes, Some(4096));
    let changes = host.team().changes(parent, Some("a1")).await.unwrap();
    assert_eq!(changes.agent.as_deref(), Some("A1"));
    assert_eq!(changes.files, vec![change("src/login.rs")]);
    // A final answer says its checks in its own words; none are listed apart.
    assert!(changes.checks.is_empty() && changes.remaining.is_none(), "{changes:?}");
    // A report an earlier version took lists them.
    let earlier = Report {
        status: ReportStatus::Ready,
        summary: "改好了".to_owned(),
        question: None,
        files: vec!["src/login.rs".to_owned()],
        checks: vec!["cargo test -p login: exit 0".to_owned()],
        remaining: Some("文档没改".to_owned()),
    };
    host.write("test.report", move |tx| {
        store::append_report(
            tx,
            child,
            "call_earlier",
            TurnTaskId::new(),
            &earlier,
            UtcTimestamp::now(),
        )
    })
    .await
    .unwrap();
    let changes = host.team().changes(parent, Some("a1")).await.unwrap();
    assert_eq!(changes.checks, vec!["cargo test -p login: exit 0".to_owned()]);
    assert_eq!(changes.remaining.as_deref(), Some("文档没改"));
    // A read-only sub-agent has no worktree to show.
    host.team()
        .call(call(parent, "agents.spawn", json!({"name": "调研", "task": task("看看构建脚本")})))
        .await
        .unwrap();
    let list = host.team().list(parent).await.unwrap();
    assert!(list.members[1].worktree.is_none());
    let none = host.team().changes(parent, Some("A2")).await.unwrap_err().to_string();
    assert!(none.contains("agents.no_worktree"), "{none}");
}

/// A conversation of the user's in a worktree, as an earlier version started them from the
/// workspace menu: it still merges and discards its own worktree.
#[tokio::test]
async fn an_earlier_conversation_of_the_users_in_a_worktree_merges_its_own_and_keeps_working_in_it()
{
    let host = FakeHost::new();
    let session = SessionId::new();
    let worktree = Worktree {
        workspace_id: WorkspaceId::new(),
        session_id: session,
        parent_session_id: None,
        parent_workspace_id: host.workspace,
        state: WorktreeState::Active,
        revision: 1,
        details: WorktreeDetails {
            path: "C:/Fixture/Worktrees/0a1b2c3d".to_owned(),
            branch: "aura/agent/0a1b2c3d".to_owned(),
            base: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        },
        created_at: UtcTimestamp::now(),
        updated_at: UtcTimestamp::now(),
    };
    let row = worktree.clone();
    host.write("test.worktree", move |tx| rows::insert_worktree(tx, &row)).await.unwrap();
    host.changes.lock().unwrap().insert(session, vec![change("README.md")]);
    let merged = host.team().call(call(session, "agents.merge", json!({}))).await.unwrap();
    assert_eq!(merged["merged"], true);
    // The conversation goes on in the same worktree: nothing is removed.
    assert!(host.removed.lock().unwrap().is_empty());
    assert_eq!(worktree_of(&host, session).state, WorktreeState::Active);
    let discarded = host.team().call(call(session, "agents.discard", json!({}))).await.unwrap();
    assert_eq!(discarded["discarded"], true);
    assert_eq!(*host.removed.lock().unwrap(), vec![worktree.details.path]);
}

#[tokio::test]
async fn deleting_a_conversation_removes_the_worktrees_its_sub_agents_still_have() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    spawn_writer(&host, parent, "改登录").await;
    spawn_writer(&host, parent, "改注册").await;
    let (first, second) = (host.child(0), host.child(1));
    ready(&host, first).await;
    host.team().call(call(parent, "agents.merge", json!({"agent": "A1"}))).await.unwrap();
    host.team().remove_session_worktrees(parent).await.unwrap();
    let removed = host.removed.lock().unwrap().clone();
    // The merged one went when it was merged; the other goes now, once.
    assert_eq!(
        removed,
        vec![worktree_of(&host, first).details.path, worktree_of(&host, second).details.path]
    );
    assert_eq!(worktree_of(&host, second).state, WorktreeState::Discarded);
    let _: WorkspaceId = worktree_of(&host, second).workspace_id;
}
