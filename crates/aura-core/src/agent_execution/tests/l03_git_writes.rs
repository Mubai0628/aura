//! Actual Git mutations, admission and durable tool receipts; no real Provider or owner config.
use super::*;
use std::{path::Path, sync::atomic::AtomicBool, time::Duration};
mod cancellation;

pub(super) fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let runner = aura_backend_windows_user::git::GitRunner::detect()
        .expect("isolated fixture profile")
        .runner
        .expect("real Git");
    let result = runner
        .run_native(
            root,
            &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            &AtomicBool::new(false),
            Duration::from_secs(10),
            1024 * 1024,
        )
        .expect("fixture Git Job");
    assert_eq!(result.exit_code, Some(0), "{args:?}: {result:?}");
    result.stdout
}

fn setup(root: &Path) {
    git(root, &["init", "--initial-branch=main"]);
    std::fs::write(root.join("selected.txt"), b"base\n").expect("selected file");
    std::fs::write(root.join("unrelated.txt"), b"base\n").expect("unrelated file");
    git(root, &["add", "--", "selected.txt", "unrelated.txt"]);
    git(root, &["commit", "-m", "fixture base"]);
}

async fn execute(f: &Fixture, name: &str, args: serde_json::Value) -> ToolCallId {
    let batch = f.admit(&[(name, args)]);
    let id = batch.batch.calls[0].tool_call_id;
    assert_eq!(batch.calls[&id].operation.content.capabilities, [ResolvedCapability::GitWrite]);
    let backend = f
        .service()
        .agent_execution_required()
        .broker_backend(&batch.calls[&id])
        .expect("owned Git backend")
        .descriptor();
    assert_eq!(backend.class, BackendClass::Native);
    assert_eq!(
        backend.guarantees,
        [EnforcementGuarantee::ChildProcessControl, EnforcementGuarantee::BoundedOutput].into()
    );
    f.execute(batch).await;
    id
}

pub(super) fn result(f: &Fixture, id: ToolCallId) -> ToolResultV1 {
    aura_persistence_v2::tool::execution::call(
        f.service().broker.lock().expect("durable receipt").database.connection(),
        id,
    )
    .expect("durable call")
    .result
    .expect("terminal result")
}

#[tokio::test]
async fn commit_selected_files_preserves_unrelated_staged_changes_and_returns_the_real_commit() {
    let f =
        Fixture::with_exposure_setup(PermissionMode::WorkspaceReadWrite, &["git.commit"], |root| {
            setup(root);
            std::fs::write(root.join("selected.txt"), b"selected change\n").expect("change");
            std::fs::write(root.join("新 文件.txt"), b"new file\n").expect("new file");
            std::fs::write(root.join("unrelated.txt"), b"keep staged\n").expect("staged file");
            git(root, &["add", "--", "unrelated.txt"]);
        })
        .await;
    let id = execute(
        &f,
        "git.commit",
        json!({"message":"selected commit\n\ncomplete body", "paths":["selected.txt","新 文件.txt"]}),
    )
    .await;
    let out = f.output(id);
    let head = String::from_utf8(git(&f.root, &["rev-parse", "HEAD"])).expect("head");
    assert_eq!(out["commit"]["commit"], head.trim());
    assert_eq!(out["commit"]["subject"], "selected commit");
    assert_eq!(out["branch"], "main");
    assert_eq!(out["status"]["total_entries"], 1);
    assert_eq!(out["status"]["entries"][0]["path"], "unrelated.txt");
    assert_eq!(out["status"]["head"], out["commit"]["commit"]);
    assert_eq!(out["execution_notice"], "与在终端运行 git 相同，Aura 不隔离它的文件写入");
    assert_eq!(git(&f.root, &["show", "HEAD:unrelated.txt"]), b"base\n");
    assert_eq!(
        String::from_utf8(git(&f.root, &["diff", "--cached", "--name-only"])).expect("staged"),
        "unrelated.txt\n"
    );
    assert_eq!(git(&f.root, &["show", "HEAD:新 文件.txt"]), b"new file\n");
    assert!(!matches!(result(&f, id).side_effect_state, SideEffectStateV1::NoEffect));
    let store = f.service().broker.lock().expect("activity journal");
    let page = aura_persistence_v2::tool::activity::page(
        store.database.connection(),
        &aura_tool_projection::AgentToolActivityRequest {
            session_id: f.turn.session_id,
            before_call_id: None,
            limit: 20,
            call_ids: Some(vec![id]),
        },
    )
    .expect("real activity projection");
    assert!(
        page.calls[0].display.summary.contains("与在终端运行 git 相同，Aura 不隔离它的文件写入")
    );
}

#[tokio::test]
async fn staged_only_commits_the_index_without_staging_unstaged_content() {
    let f =
        Fixture::with_exposure_setup(PermissionMode::WorkspaceReadWrite, &["git.commit"], |root| {
            setup(root);
            std::fs::write(root.join("selected.txt"), b"staged\n").expect("staged version");
            git(root, &["add", "--", "selected.txt"]);
            std::fs::write(root.join("selected.txt"), b"unstaged\n").expect("unstaged version");
        })
        .await;
    let id = execute(&f, "git.commit", json!({"message":"index commit", "staged_only":true})).await;
    assert_eq!(f.output(id)["commit"]["subject"], "index commit");
    assert_eq!(git(&f.root, &["show", "HEAD:selected.txt"]), b"staged\n");
    assert_eq!(std::fs::read(f.root.join("selected.txt")).expect("worktree"), b"unstaged\n");
}

#[tokio::test]
async fn branch_create_and_switch_is_native_and_records_its_actual_head() {
    let f =
        Fixture::with_exposure_setup(PermissionMode::WorkspaceReadWrite, &["git.branch"], setup)
            .await;
    let before = git(&f.root, &["rev-parse", "HEAD"]);
    let id =
        execute(&f, "git.branch", json!({"kind":"create", "name":"l03/分支", "switch":true})).await;
    let out = f.output(id);
    assert_eq!(out["branch"], "l03/分支");
    assert_eq!(out["created"], true);
    assert_eq!(out["switched"], true);
    assert_eq!(out["previous_branch"], "main");
    assert_eq!(out["previous_head"], std::str::from_utf8(&before).expect("head").trim());
    assert_eq!(out["status"]["branch"], "l03/分支");
    assert_eq!(git(&f.root, &["rev-parse", "HEAD"]), before);
    assert_eq!(git(&f.root, &["symbolic-ref", "--short", "HEAD"]), "l03/分支\n".as_bytes());
    assert_eq!(out["execution_notice"], "与在终端运行 git 相同，Aura 不隔离它的文件写入");
}

#[tokio::test]
async fn readonly_commit_and_branch_pause_before_any_native_mutation() {
    for (name, args) in [
        ("git.commit", json!({"message":"needs approval", "paths":["selected.txt"]})),
        ("git.branch", json!({"kind":"create", "name":"needs-approval"})),
    ] {
        let f = Fixture::with_exposure_setup(PermissionMode::ReadOnly, &[name], setup).await;
        let before = git(&f.root, &["rev-parse", "HEAD"]);
        let batch = f.admit(&[(name, args)]);
        let id = batch.batch.calls[0].tool_call_id;
        let (_sender, cancel) = tokio::sync::watch::channel(false);
        let outcome = f
            .service()
            .agent_execution_required()
            .execute_batch(batch, f.service().commands_required().clone(), cancel)
            .await
            .expect("precise Git question");
        assert!(matches!(outcome, BatchOutcome::AwaitingApproval { .. }));
        let call = aura_persistence_v2::tool::execution::call(
            f.service().broker.lock().expect("call journal").database.connection(),
            id,
        )
        .expect("pending call");
        assert!(call.attempt_id.is_none() && call.result.is_none());
        assert_eq!(git(&f.root, &["rev-parse", "HEAD"]), before);
    }
}
