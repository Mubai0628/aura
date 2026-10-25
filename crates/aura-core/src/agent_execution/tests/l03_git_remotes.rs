//! B3 real local remotes. No system/global owner config, credentials or external server.
use super::l03_git_writes::git;
use super::*;
use std::path::Path;
mod cancellation;
mod consents;
mod failures;
mod http;

fn approve(f: &Fixture, call: &CallContext, decision: ApprovalDecisionKind) -> AuthorizationGrant {
    let engine = f.service().agent_execution_required();
    let backend = engine.broker_backend(call).expect("Native Git backend").descriptor();
    let grants = aura_persistence_v2::security::grants_for_call(
        f.service().broker.lock().expect("grant store").database.connection(),
        &call.operation.fingerprint,
        Some(f.turn.session_id),
    )
    .expect("candidate grants");
    let mut input = call.policy(UtcTimestamp::now(), engine.runtime);
    input.grants = &grants;
    let PolicyAuthorization::Ask { proposal } =
        aura_permission_engine::policy::authorize(&input, &backend, None)
    else {
        panic!("missing Git capability must ask");
    };
    let now = UtcTimestamp::now();
    let request = AuthorizationApproval {
        approval_id: ApprovalRequestId::new(),
        revision: Revision::INITIAL,
        scope: proposal.scope,
        allowed_decisions: proposal.allowed_decisions,
        state: ApprovalState::Pending,
        requested_at: now,
        expires_at: UtcTimestamp::from_value(now.value() + time::Duration::minutes(10)),
    };
    f.service()
        .broker
        .lock()
        .expect("approval store")
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::security::insert_approval(tx, &request)?;
            aura_persistence_v2::security_approval::resolve(
                tx,
                request.approval_id,
                request.revision,
                &call.operation,
                decision,
                backend.class,
                call.snapshot.session_permission_revision,
                engine.runtime,
                now,
            )
        })
        .expect("stored Git approval")
        .expect("allow issues grant")
}

fn initialize(root: &Path) {
    git(root, &["init", "--initial-branch=main"]);
    std::fs::write(root.join("tracked.txt"), b"base\n").expect("fixture content");
    git(root, &["add", "--", "tracked.txt"]);
    git(root, &["commit", "-m", "fixture base"]);
}

fn oid(root: &Path, reference: &str) -> String {
    String::from_utf8(git(root, &["rev-parse", "--verify", reference]))
        .expect("object id")
        .trim()
        .to_owned()
}

fn remote() -> (tempfile::TempDir, String) {
    let temporary = tempfile::Builder::new()
        .prefix("aura-git-test-remotes-")
        .tempdir()
        .expect("owned remote root");
    let bare = temporary.path().join("remote.git");
    std::fs::create_dir(&bare).expect("bare directory");
    git(&bare, &["init", "--bare", "--initial-branch=main"]);
    let url = url::Url::from_file_path(&bare).expect("local file URL").to_string();
    (temporary, url)
}

fn assert_native(f: &Fixture, batch: &AdmittedBatch, id: ToolCallId) {
    let call = &batch.calls[&id];
    assert_eq!(call.operation.content.network, OperationNetwork::BroadProcess);
    let backend = f
        .service()
        .agent_execution_required()
        .broker_backend(call)
        .expect("owned Git backend")
        .descriptor();
    assert_eq!(backend.class, BackendClass::Native);
    assert_eq!(
        backend.guarantees,
        [EnforcementGuarantee::ChildProcessControl, EnforcementGuarantee::BoundedOutput].into()
    );
}

#[tokio::test]
async fn push_publishes_only_the_requested_branch_and_returns_a_native_receipt() {
    let (temporary, url) = remote();
    let f = Fixture::with_exposure_setup(PermissionMode::FullAccess, &["git.push"], |root| {
        initialize(root);
        git(root, &["remote", "add", "origin", &url]);
    })
    .await;
    let expected = oid(&f.root, "HEAD");
    let batch = f.admit(&[(
        "git.push",
        json!({
            "remote":"origin", "branch":"main", "remote_branch":"review", "set_upstream":true
        }),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    assert_eq!(
        batch.calls[&id].operation.content.capabilities,
        [ResolvedCapability::GitRemoteWrite]
    );
    assert_native(&f, &batch, id);
    f.execute(batch).await;
    let out = f.output(id);
    let bare = temporary.path().join("remote.git");
    assert_eq!(oid(&bare, "refs/heads/review"), expected);
    assert_eq!(
        git(&bare, &["for-each-ref", "--format=%(refname)", "refs/heads"]),
        b"refs/heads/review\n"
    );
    assert_eq!(git(&f.root, &["rev-parse", "--abbrev-ref", "@{upstream}"]), b"origin/review\n");
    assert_eq!(out["status"]["head"], expected);
    assert_eq!(out["execution_notice"], aura_workspace_domain::GIT_NATIVE_EXECUTION_NOTICE);
    assert!(out["refs"].as_array().is_some_and(|refs| !refs.is_empty()), "{out}");
}

#[tokio::test]
async fn workspace_fetch_then_default_upstream_pull_is_fast_forward_without_extra_approval() {
    let (temporary, url) = remote();
    let seed = temporary.path().join("seed");
    std::fs::create_dir(&seed).expect("seed checkout");
    initialize(&seed);
    git(&seed, &["remote", "add", "origin", &url]);
    git(&seed, &["push", "--set-upstream", "origin", "main"]);
    let mut f =
        Fixture::with_exposure_setup(PermissionMode::WorkspaceReadWrite, &["git.sync"], |root| {
            git(root, &["clone", "--", &url, "."]);
        })
        .await;
    let before = oid(&f.root, "HEAD");
    std::fs::write(seed.join("tracked.txt"), b"remote change\n").expect("remote edit");
    git(&seed, &["add", "--", "tracked.txt"]);
    git(&seed, &["commit", "-m", "remote next"]);
    git(&seed, &["push", "origin", "main"]);
    let expected = oid(&seed, "HEAD");

    let fetch = f.admit(&[("git.sync", json!({"kind":"fetch","remote":"origin"}))]);
    let id = fetch.batch.calls[0].tool_call_id;
    let batch_id = fetch.batch.tool_batch_id;
    assert_eq!(
        fetch.calls[&id].operation.content.capabilities,
        [ResolvedCapability::GitRemoteRead]
    );
    assert_native(&f, &fetch, id);
    let group = f.execute(fetch).await;
    assert_eq!(oid(&f.root, "refs/remotes/origin/main"), expected);
    assert_eq!(oid(&f.root, "HEAD"), before);
    assert_eq!(f.output(id)["status"]["behind"], 1);
    f.continue_after_tools(batch_id, &group);

    let pull = f.admit(&[("git.sync", json!({"kind":"pull"}))]);
    let id = pull.batch.calls[0].tool_call_id;
    let capabilities = &pull.calls[&id].operation.content.capabilities;
    assert!(capabilities.contains(&ResolvedCapability::GitRemoteRead));
    assert!(capabilities.contains(&ResolvedCapability::GitWrite));
    assert_native(&f, &pull, id);
    f.execute(pull).await;
    assert_eq!(oid(&f.root, "HEAD"), expected);
    assert_eq!(std::fs::read(f.root.join("tracked.txt")).expect("pulled file"), b"remote change\n");
    let out = f.output(id);
    assert_eq!(out["status"]["head"], expected);
    assert_eq!(out["status"]["behind"], 0);
    assert_eq!(out["execution_notice"], aura_workspace_domain::GIT_NATIVE_EXECUTION_NOTICE);
}
