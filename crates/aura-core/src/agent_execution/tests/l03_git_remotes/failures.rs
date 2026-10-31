use super::super::l03_git_writes::result;
use super::*;
use aura_git_service::git;

#[tokio::test]
async fn an_exact_lease_can_replace_only_the_observed_remote_branch() {
    let (temporary, url) = remote();
    let f = Fixture::with_exposure_setup(PermissionMode::FullAccess, &["git.push"], |root| {
        initialize(root);
        git(root, &["remote", "add", "origin", &url]);
        git(root, &["push", "origin", "main"]);
    })
    .await;
    let local = oid(&f.root, "HEAD");
    let seed = temporary.path().join("seed");
    std::fs::create_dir(&seed).expect("seed");
    git(&seed, &["clone", &url, "."]);
    std::fs::write(seed.join("remote-only.txt"), b"observed remote advance\n")
        .expect("remote content");
    git(&seed, &["add", "remote-only.txt"]);
    git(&seed, &["commit", "-m", "observed remote advance"]);
    git(&seed, &["push", "origin", "main", "main:keep"]);
    let bare = temporary.path().join("remote.git");
    let observed = oid(&bare, "refs/heads/main");
    assert_ne!(observed, local);
    let batch = f.admit(&[(
        "git.push",
        json!({"remote":"origin","remote_branch":"main","force_with_lease":observed}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    approve(&f, &batch.calls[&id], ApprovalDecisionKind::AllowOnce);
    f.execute(batch).await;
    let output = f.output(id);
    assert_eq!(output["refs"][0]["before"], observed);
    assert_eq!(output["refs"][0]["after"], local);
    assert_eq!(oid(&bare, "refs/heads/main"), local);
    assert_eq!(oid(&bare, "refs/heads/keep"), observed);
}

#[tokio::test]
async fn opaque_helpers_are_refused_before_an_attempt_with_safe_actionable_feedback() {
    let f = Fixture::with_exposure_setup(PermissionMode::FullAccess, &["git.push"], |root| {
        initialize(root);
        git(root, &["remote", "add", "origin", "ext::command --password=temporary-secret"]);
    })
    .await;
    let batch = f.admit(&[("git.push", json!({"remote":"origin","remote_branch":"main"}))]);
    let id = batch.batch.calls[0].tool_call_id;
    assert!(batch.calls.is_empty());
    f.execute(batch).await;
    let text = serde_json::to_string(&result(&f, id)).expect("paired refusal");
    assert!(text.contains("git.opaque_remote") && text.contains("终端"), "{text}");
    assert!(!text.contains("temporary-secret"), "{text}");
    let stored = aura_persistence_v2::tool::execution::call(
        f.service().broker.lock().expect("call").database.connection(),
        id,
    )
    .expect("record");
    assert!(stored.attempt_id.is_none());
}

#[tokio::test]
async fn multiple_push_urls_return_actionable_feedback_before_an_attempt() {
    let (_first, first) = remote();
    let (_second, second) = remote();
    let f = Fixture::with_exposure_setup(PermissionMode::FullAccess, &["git.push"], |root| {
        initialize(root);
        git(root, &["remote", "add", "origin", &first]);
        git(root, &["remote", "set-url", "--add", "--push", "origin", &first]);
        git(root, &["remote", "set-url", "--add", "--push", "origin", &second]);
    })
    .await;
    let batch = f.admit(&[("git.push", json!({"remote":"origin","remote_branch":"main"}))]);
    let id = batch.batch.calls[0].tool_call_id;
    assert!(batch.calls.is_empty());
    f.execute(batch).await;
    let result = result(&f, id);
    let text = serde_json::to_string(&result).expect("paired refusal");
    assert!(text.contains("git.multiple_remote_urls") && text.contains("终端"), "{text}");
    let stored = aura_persistence_v2::tool::execution::call(
        f.service().broker.lock().expect("call").database.connection(),
        id,
    )
    .expect("record");
    assert!(stored.attempt_id.is_none());
}

#[tokio::test]
async fn rejection_non_fast_forward_and_stale_exact_lease_preserve_the_remote() {
    for case in ["hook", "non_ff", "lease"] {
        let (temporary, url) = remote();
        let bare = temporary.path().join("remote.git");
        let f = Fixture::with_exposure_setup(PermissionMode::FullAccess, &["git.push"], |root| {
            initialize(root);
            git(root, &["remote", "add", "origin", &url]);
            git(root, &["push", "origin", "main"]);
        })
        .await;
        let original = oid(&bare, "refs/heads/main");
        if case == "hook" {
            std::fs::write(
                bare.join("hooks/pre-receive"),
                b"#!/bin/sh\necho temporary-fixture-rejection >&2\nexit 1\n",
            )
            .expect("fixture hook");
        } else if case == "non_ff" {
            let seed = temporary.path().join("seed");
            std::fs::create_dir(&seed).expect("seed");
            git(&seed, &["clone", &url, "."]);
            std::fs::write(seed.join("tracked.txt"), b"remote change\n").expect("change");
            git(&seed, &["commit", "-am", "remote advance"]);
            git(&seed, &["push", "origin", "main"]);
        }
        std::fs::write(f.root.join("tracked.txt"), b"local change\n").expect("change");
        git(&f.root, &["commit", "-am", "local advance"]);
        let before = oid(&bare, "refs/heads/main");
        let mut args = json!({"remote":"origin","remote_branch":"main"});
        if case == "lease" {
            args["force_with_lease"] = json!("f".repeat(40));
        }
        let batch = f.admit(&[("git.push", args)]);
        let id = batch.batch.calls[0].tool_call_id;
        if case == "lease" {
            approve(&f, &batch.calls[&id], ApprovalDecisionKind::AllowOnce);
        }
        f.execute(batch).await;
        let result = result(&f, id);
        let text = serde_json::to_string(&result).expect("receipt");
        assert!(matches!(result.outcome, ToolResultOutcomeV1::Failed { .. }), "{case}: {text}");
        assert!(
            text.contains(match case {
                "lease" => "git.lease_mismatch",
                "non_ff" => "git.non_fast_forward",
                _ => "git.remote_rejected",
            }),
            "{text}"
        );
        assert_eq!(oid(&bare, "refs/heads/main"), before);
        if case != "non_ff" {
            assert_eq!(before, original);
        }
    }
}

#[tokio::test]
async fn divergent_pull_never_rebases_merges_or_discards_local_changes() {
    let (temporary, url) = remote();
    let f =
        Fixture::with_exposure_setup(PermissionMode::WorkspaceReadWrite, &["git.sync"], |root| {
            initialize(root);
            git(root, &["remote", "add", "origin", &url]);
            git(root, &["push", "--set-upstream", "origin", "main"]);
        })
        .await;
    let seed = temporary.path().join("seed");
    std::fs::create_dir(&seed).expect("seed");
    git(&seed, &["clone", &url, "."]);
    std::fs::write(seed.join("tracked.txt"), b"remote advance\n").expect("edit");
    git(&seed, &["commit", "-am", "remote advance"]);
    git(&seed, &["push", "origin", "main"]);
    std::fs::write(f.root.join("local.txt"), b"local advance\n").expect("edit");
    git(&f.root, &["add", "local.txt"]);
    git(&f.root, &["commit", "-m", "local advance"]);
    git(&f.root, &["config", "pull.rebase", "true"]);
    let before = oid(&f.root, "HEAD");
    let batch = f.admit(&[("git.sync", json!({"kind":"pull"}))]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute(batch).await;
    assert!(
        serde_json::to_string(&result(&f, id)).expect("result").contains("git.cannot_fast_forward")
    );
    assert_eq!(oid(&f.root, "HEAD"), before);
    assert!(!f.root.join(".git/MERGE_HEAD").exists());
    assert!(!f.root.join(".git/rebase-merge").exists());
    assert_eq!(std::fs::read(f.root.join("local.txt")).expect("local file"), b"local advance\n");
}

#[tokio::test]
async fn configured_pushurl_is_the_only_receipt_destination_and_multiple_pushurls_are_refused() {
    let (fetch_temp, fetch_url) = remote();
    let (push_temp, push_url) = remote();
    let f = Fixture::with_exposure_setup(PermissionMode::FullAccess, &["git.push"], |root| {
        initialize(root);
        git(root, &["remote", "add", "origin", &fetch_url]);
        git(root, &["remote", "set-url", "--push", "origin", &push_url]);
    })
    .await;
    let expected = oid(&f.root, "HEAD");
    let batch = f.admit(&[("git.push", json!({"remote":"origin","remote_branch":"main"}))]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute(batch).await;
    assert_eq!(f.output(id)["refs"][0]["after"], expected);
    assert_eq!(oid(&push_temp.path().join("remote.git"), "refs/heads/main"), expected);
    assert!(git(&fetch_temp.path().join("remote.git"), &["for-each-ref"]).is_empty());
    git(&f.root, &["remote", "set-url", "--add", "--push", "origin", &fetch_url]);
    let reader = git::GitReader::open(&f.root, Arc::new(std::sync::atomic::AtomicBool::new(false)))
        .expect("isolated reader");
    let error = reader
        .remote_plan("git.push", &json!({"remote":"origin","remote_branch":"main"}))
        .expect_err("cannot authorize multiple endpoints");
    assert!(error.to_string().contains("git.multiple_remote_urls"));
}
