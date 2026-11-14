//! A writing sub-agent in a worktree of its own, through the real model loop: it changes a file in
//! its copy and is refused a file of the main workspace; the main agent's merge waits for the
//! user, then brings the change into the main workspace and removes the copy and its branch.
use super::l19_team::{begin, call, members, offered, requests, results, texts};
use super::*;
use provider_fixture::{ADDED_FILE, ADDED_TEXT, Scenario, TeamPlan};
use std::path::{Path, PathBuf};

pub(super) fn git(root: &Path, args: &[&str]) -> String {
    let binary = std::env::var_os("AURA_GIT_TEST_BINARY")
        .unwrap_or_else(|| "C:/Program Files/Git/cmd/git.exe".into());
    let out =
        std::process::Command::new(binary).current_dir(root).args(args).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A Git workspace with one commit, a conversation in it that may change its files, a folder of
/// the test's for new worktrees, and its model allowed for sub-agents.
pub(super) async fn prepare(
    host: &crate::r03_tests::Fixture,
    provider: &Provider,
) -> (crate::RuntimeState, SessionId, ModelProfileId, PathBuf, PathBuf) {
    let runtime = host.start().await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("worktree-workspace");
    std::fs::create_dir(&root).expect("workspace folder");
    std::fs::write(root.join("lib.txt"), "base\n").expect("a tracked file");
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "Aura Test"]);
    git(&root, &["config", "user.email", "aura-test@example.invalid"]);
    // The machine's Git may turn line endings into CRLF on checkout; the files compared are LF.
    git(&root, &["config", "core.autocrlf", "false"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "base"]);
    let created = call(
        &runtime,
        "workspace.create",
        json!({"selected_root": root, "display_name": "Worktrees"}),
    )
    .await;
    let workspace: WorkspaceId =
        serde_json::from_value(created["workspace"]["workspace_id"].clone()).expect("workspace");
    let session = create_session(&runtime, workspace, PermissionMode::WorkspaceReadWrite).await;
    let snapshot = call(&runtime, "models.settings.get_snapshot", json!({})).await;
    call(
        &runtime,
        "models.profile.set_subagent_availability",
        json!({
            "expected_revision": snapshot["models_revision"],
            "model_profile_id": model,
            "available_for_subagents": true,
        }),
    )
    .await;
    call(
        &runtime,
        "models.session.set_binding",
        json!({"expected_revision": "0", "session_id": session, "model_profile_id": model}),
    )
    .await;
    let worktrees = host.root.path().join("worktrees");
    let location = call(&runtime, "agent.team.worktrees.location", json!({})).await;
    let chosen = call(
        &runtime,
        "agent.team.worktrees.location.set",
        json!({"location": worktrees, "expected_revision": location["revision"]}),
    )
    .await;
    assert_eq!(chosen["location"], json!(worktrees), "{chosen}");
    (runtime, session, model, root, worktrees)
}

/// The one worktree folder under `location`: `<location>\<workspace folder>\<key>`.
pub(super) fn only_copy(location: &Path) -> PathBuf {
    let folders = |path: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(path)
            .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
            .unwrap_or_default()
    };
    let workspaces = folders(location);
    assert_eq!(workspaces.len(), 1, "{workspaces:?}");
    let copies = folders(&workspaces[0]);
    assert_eq!(copies.len(), 1, "{copies:?}");
    copies[0].clone()
}

#[tokio::test]
async fn a_sub_agent_in_a_worktree_changes_only_its_copy_and_the_merge_waits_for_the_user() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::Worktree }).await;
    let (runtime, session, model, root, location) = prepare(&host, &provider).await;
    let outside = root.join("outside.txt");
    let request = begin(
        &runtime,
        session,
        model,
        &format!("改代码 AURA_EXTERNAL_PATH={}", outside.display()),
    )
    .await;

    // A1's write in the main workspace is outside its copy: it asks the user, as the main agent's
    // write outside its workspace does, and the user says no.
    let (child, outside_write) = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        loop {
            let team = members(&runtime, session).await;
            if let Some(member) = team.first()
                && let Some(id) = member["approvals"].as_array().and_then(|ids| ids.first())
            {
                return (member["session_id"].clone(), id.clone());
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("A1's write outside its copy asks the user");
    let shown = call(
        &runtime,
        "agent.approval.get",
        json!({"session_id": child, "approval_id": outside_write}),
    )
    .await;
    assert!(shown.to_string().contains("outside.txt"), "{shown}");
    call(
        &runtime,
        "agent.approval.resolve",
        json!({
            "session_id": child,
            "approval_id": outside_write,
            "expected_revision": shown["approval"]["revision"],
            "decision": "deny",
        }),
    )
    .await;

    // The main agent's merge waits for the user.
    let approval = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        loop {
            let view: AgentRunProjection =
                serde_json::from_value(call(&runtime, "agent.status", json!(&request)).await)
                    .expect("status");
            if let Some(id) = view.pending_approval_ids.first().copied() {
                return id;
            }
            assert!(!view.turn.state.terminal(), "ended before the merge: {:?}", view.turn.state);
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the merge asks the user");

    // Until then the change is only in the sub-agent's copy, on its own branch.
    let copy = only_copy(&location);
    assert_eq!(std::fs::read_to_string(copy.join(ADDED_FILE)).expect("in the copy"), ADDED_TEXT);
    assert!(!root.join(ADDED_FILE).exists(), "the main workspace is unchanged");
    assert!(!outside.exists(), "the write the user refused did not run");
    let (theirs, _) = requests(&provider);
    let read = results(theirs.last().expect("the sub-agent's last request"));
    assert!(!read[1].contains("logical_hash"), "{read:?}");
    // Its sheet and its tools say it changes files in its copy; nothing it reads says it changes
    // no file.
    let first = &theirs[0];
    let sheet = texts(first, "user").join("\n");
    assert!(sheet.contains("\n## 工作副本\n") && sheet.contains("fs.write"), "{sheet}");
    assert!(sheet.contains("你的权限档位是「工作区读写」"), "{sheet}");
    assert!(!texts(first, "system").iter().any(|t| t.contains("change no file")), "{first}");
    assert!(offered(first).contains("fs.write"), "{first}");
    let team = members(&runtime, session).await;
    let branch = team[0]["worktree"]["branch"].as_str().expect("its branch").to_owned();
    assert!(branch.starts_with("aura/agent/"), "{branch}");
    let changes =
        call(&runtime, "agent.team.changes", json!({"session_id": session, "agent": "A1"})).await;
    assert_eq!(changes["files"][0]["path"], ADDED_FILE, "{changes}");
    assert_eq!(changes["files"][0]["kind"], "added", "{changes}");
    // The worktree's workspace is listed under the workspace it was made from.
    let projection = call(
        &runtime,
        "projection.snapshot",
        json!({"projection_kind": "workspace_session", "projection_scope": {"type": "global"}}),
    )
    .await;
    assert_eq!(projection["payload"]["worktrees"].as_array().map(Vec::len), Some(1));

    let asked = call(
        &runtime,
        "agent.approval.get",
        json!({"session_id": session, "approval_id": approval}),
    )
    .await;
    call(
        &runtime,
        "agent.approval.resolve",
        json!({
            "session_id": session,
            "approval_id": approval,
            "expected_revision": asked["approval"]["revision"],
            "decision": "allow_once",
        }),
    )
    .await;
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;

    // Merged into the main workspace's branch; the copy and its branch are gone.
    assert_eq!(std::fs::read_to_string(root.join(ADDED_FILE)).expect("merged"), ADDED_TEXT);
    assert!(!copy.exists(), "the copy was removed: {}", copy.display());
    assert_eq!(git(&root, &["branch", "--list", &branch]).trim(), "", "its branch was deleted");
    let (_, ours) = requests(&provider);
    let merged = results(ours.last().expect("the conversation's last request"));
    let said_merged =
        |result: &String| result.contains("\"merged\":true") || result.contains("\"merged\": true");
    assert!(merged.last().is_some_and(said_merged), "{merged:?}");
    let team = members(&runtime, session).await;
    assert_eq!(team[0]["worktree"]["state"], "merged", "{team:?}");
}
