//! Real P04 publication, injected I/O/SQLite failures, and production startup reconciliation.
//! The interruption seam is test-only and fires after disk commit, never manufactures a receipt.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::{cell::Cell, path::Path};

use aura_permission_domain::{
    ExternalGrantLifecycle, ExternalGrantRecord, ExternalGrantTargetKind, ExternalGrantTerm,
};
use aura_workspace_domain::{
    CommitRequest, ExecutionId, FileResource, ResourceAccess, ResourceRequest, SessionSummary,
    WorkspaceCreateCommand,
};

use super::*;

thread_local! {
    static INTERRUPTION: Cell<Option<&'static str>> = const { Cell::new(None) };
}
static PUBLICATION_FAULTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
#[path = "publication_process_tests.rs"]
mod publication_process_tests;
#[path = "publication_wait_tests.rs"]
mod publication_wait_tests;

pub(super) fn interrupt_at(stage: &'static str) {
    publication_process_tests::hold_for_process_controller(stage);
    INTERRUPTION.with(|point| {
        if point.get() == Some(stage) {
            point.set(None);
            panic!("simulated termination after disk commit at {stage}");
        }
    });
}

fn services(state: &crate::RuntimeState) -> &crate::RuntimeServices {
    let crate::StartupMode::Ready(services) = &state.startup_mode else {
        panic!("isolated Core startup did not reach Ready");
    };
    services
}

struct Target {
    workspace: WorkspaceId,
    session: SessionId,
    grant: aura_domain_common::ExternalGrantId,
    path: PathBuf,
}

fn seed(p04: &P04Services, root: &Path) -> Target {
    let directory = root.join("files");
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("target.txt");
    std::fs::write(&path, b"old").unwrap();
    let workspace = p04
        .workspace
        .create(
            WorkspaceCreateCommand {
                selected_root: directory.display().to_string(),
                display_name: "P04 boundaries".into(),
            },
            CorrelationId::new(),
        )
        .unwrap()
        .workspace;
    let session = SessionId::new();
    p04.persistence
        .upsert_workspace_session(
            workspace.clone(),
            SessionSummary {
                session_id: session,
                workspace_id: workspace.workspace_id,
                title: "publication".into(),
                state: "idle".into(),
            },
            CorrelationId::new(),
        )
        .unwrap();
    let info = aura_windows_platform::inspect_existing_path(&directory, false).unwrap();
    let now = UtcTimestamp::now();
    let grant = aura_domain_common::ExternalGrantId::new();
    p04.persistence
        .create_external_grant(
            ExternalGrantRecord {
                id: grant,
                session_id: session,
                workspace_id: workspace.workspace_id,
                permission_grant_id: None,
                selected_root: canonical_path(directory),
                canonical_root: canonical_path(info.final_path),
                root_identity: domain_identity(&info.identity),
                target_kind: ExternalGrantTargetKind::Directory,
                capabilities: vec![
                    ExternalGrantCapability::Read,
                    ExternalGrantCapability::Write,
                    ExternalGrantCapability::Create,
                ],
                recursive_scope: true,
                lifecycle: ExternalGrantLifecycle::Active,
                revalidation_revision: 1,
                created_at: now,
                updated_at: now,
                suspended_at: None,
                revoked_at: None,
                expires_at: None,
                turn_task_id: None,
                term: ExternalGrantTerm::Session,
            },
            CorrelationId::new(),
        )
        .unwrap();
    Target { workspace: workspace.workspace_id, session, grant, path }
}

fn publish(p04: &P04Services, target: &Target) -> Result<(), IpcErrorEnvelope> {
    p04.export_artifact_to_external_grant(
        target.grant,
        target.workspace,
        target.session,
        &WorkspacePath::parse("target.txt").unwrap(),
        &FsExpectedState::FileHash { hash: ContentHash::blake3(b"old") },
        b"new",
        &ContentHash::blake3(b"new"),
        CorrelationId::new(),
    )
}

fn operation(database: &Path) -> SecurityOperationId {
    let connection = rusqlite::Connection::open(database).unwrap();
    let id: String = connection
        .query_row("SELECT security_operation_id FROM artifact_overwrite_journal", [], |row| {
            row.get(0)
        })
        .unwrap();
    id.parse().unwrap()
}

fn deny_transition(database: &Path, state: &str) {
    assert!(matches!(state, "aborted" | "published"));
    let connection = rusqlite::Connection::open(database).unwrap();
    connection.execute_batch(&format!("CREATE TRIGGER fail_receipt BEFORE UPDATE ON artifact_overwrite_journal WHEN NEW.state='{state}' BEGIN SELECT RAISE(ABORT, 'injected journal receipt failure'); END;")).unwrap();
}

#[tokio::test]
async fn overwrite_uses_the_main_file_owner_without_blocking_siblings() {
    let fixture = crate::r03_tests::Fixture::new();
    let state = fixture.start().await;
    let p04 = services(&state).p04_required();
    let target = seed(&p04, fixture.root.path());
    let facts = p04.workspace.resource_facts(target.workspace, target.session).unwrap();
    let owner = p04.workspace.eligibility_owner();
    let path = WorkspacePath::parse("target.txt").unwrap();
    let resources = vec![facts.object(&path).unwrap().unwrap(), facts.slot(&path).unwrap()];
    let held = owner
        .commit(
            &CommitRequest::many(resources, ResourceAccess::Write),
            target.session,
            ExecutionId::next(),
            None,
        )
        .unwrap();
    let error = publish(&p04, &target).unwrap_err();
    // **A live short execution conflict is the retryable code, and it says so.**
    //
    // This assertion is the corrected contract, not a weakened one. The hold here is another
    // *execution* that will finish, and nothing was published, so the resource owner reports the
    // retryable `file.external_resource_busy` and the caller's asynchronous owner may wait within its
    // original budget and re-enter the unchanged request. The non-retryable
    // `file.external_resource_held` is reserved for a refusal waiting cannot change — a long editor
    // claim, or a stale-eligibility answer — which is why the two codes exist.
    assert_eq!(error.code.as_str(), "file.external_resource_busy");
    assert!(
        error.retryable,
        "a live short conflict must be retryable: the holder is an execution that will finish"
    );
    assert_eq!(std::fs::read(&target.path).unwrap(), b"old");
    owner.release_commit(held);
    let unrelated = owner
        .commit(
            &CommitRequest::one(
                facts.slot(&WorkspacePath::parse("sibling.txt").unwrap()).unwrap(),
                ResourceAccess::Write,
            ),
            target.session,
            ExecutionId::next(),
            None,
        )
        .unwrap();
    publish(&p04, &target).unwrap();
    owner.release_commit(unrelated);
    assert_eq!(std::fs::read(&target.path).unwrap(), b"new");
    assert_eq!(
        p04.persistence
            .artifact_overwrite_journal(operation(&fixture.paths.database_path))
            .unwrap()
            .state,
        ArtifactOverwriteJournalState::Completed
    );
}

#[tokio::test]
async fn rollback_and_failed_failure_receipt_keep_both_causes() {
    let _serial = PUBLICATION_FAULTS.lock().await;
    let fixture = crate::r03_tests::Fixture::new();
    let state = fixture.start().await;
    let p04 = services(&state).p04_required();
    let target = seed(&p04, fixture.root.path());
    deny_transition(&fixture.paths.database_path, "aborted");
    let _fault = aura_windows_platform::publication_fault::arm(
        aura_windows_platform::publication_fault::Stage::DeleteBackupOnCommit,
        &target.path,
    );
    let error = publish(&p04, &target).unwrap_err();
    assert_eq!(error.code.as_str(), "artifact.handle_bound_publish_failed");
    assert!(error.message.contains("delete_backup_on_commit"), "{error:?}");
    assert!(error.details["journal_failure"].as_str().unwrap().contains("injected journal"));
    assert_eq!(
        std::fs::read(&target.path).unwrap(),
        b"old",
        "real compensation must restore old content despite journal failure"
    );
    assert_eq!(
        p04.persistence
            .artifact_overwrite_journal(operation(&fixture.paths.database_path))
            .unwrap()
            .state,
        ArtifactOverwriteJournalState::Applying
    );
}

#[tokio::test]
async fn failed_real_rollback_marks_the_original_journal_needs_reconcile() {
    let _serial = PUBLICATION_FAULTS.lock().await;
    let fixture = crate::r03_tests::Fixture::new();
    let state = fixture.start().await;
    let p04 = services(&state).p04_required();
    let target = seed(&p04, fixture.root.path());
    let _commit = aura_windows_platform::publication_fault::arm(
        aura_windows_platform::publication_fault::Stage::DeleteBackupOnCommit,
        &target.path,
    );
    let _restore = aura_windows_platform::publication_fault::arm(
        aura_windows_platform::publication_fault::Stage::RestoreBackup,
        &target.path,
    );
    let error = publish(&p04, &target).unwrap_err();
    let journal = p04
        .persistence
        .artifact_overwrite_journal(operation(&fixture.paths.database_path))
        .unwrap();
    assert_eq!(journal.state, ArtifactOverwriteJournalState::NeedsReconcile);
    assert!(error.message.contains("restore_backup"));
    assert_eq!(
        std::fs::read(target.path.parent().unwrap().join(&journal.binding.backup_file_name))
            .unwrap(),
        b"old",
        "unconfirmed compensation must keep real recovery bytes"
    );
}

#[tokio::test]
async fn published_bytes_without_a_receipt_are_unknown_and_not_retryable() {
    let fixture = crate::r03_tests::Fixture::new();
    let state = fixture.start().await;
    let p04 = services(&state).p04_required();
    let target = seed(&p04, fixture.root.path());
    deny_transition(&fixture.paths.database_path, "published");
    let error = publish(&p04, &target).unwrap_err();
    assert!(crate::command_settlement::envelope_is_unconfirmed(&error));
    assert!(!error.retryable);
    assert_eq!(std::fs::read(&target.path).unwrap(), b"new");
    assert_eq!(
        error.details["security_operation_id"],
        serde_json::json!(operation(&fixture.paths.database_path))
    );
}

#[tokio::test]
async fn a_deep_external_target_intersects_its_real_ancestor_subtree_and_leaves_siblings_parallel()
{
    // The pre-fix shape located every external target at `at_anchor(parent_identity)`, which claims
    // the parent is itself the outermost directory. A grant folder several levels deep therefore had
    // an anchor that really sits *inside* a higher directory's chain, and a scope held over that
    // higher directory could never be seen to contain the target.
    let fixture = crate::r03_tests::Fixture::new();
    let state = fixture.start().await;
    let p04 = services(&state).p04_required();
    let target = seed(&p04, fixture.root.path());
    let owner = p04.workspace.eligibility_owner();
    let outer = target.path.parent().unwrap();
    let deep = outer.join("deep").join("deeper");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(deep.join("nested.txt"), b"before").unwrap();

    // A scope over the **higher** directory that really contains the deep target.
    let outer_info = aura_windows_platform::inspect_existing_path(outer, false).unwrap();
    let outer_subtree = FileResource::Subtree {
        directory_identity: domain_identity(&outer_info.identity),
        location: aura_workspace::external_ancestry(
            &aura_windows_platform::open_verified_existing(
                outer,
                aura_windows_platform::NativeFileAccess::Metadata,
            )
            .unwrap(),
        )
        .expect("complete outer ancestry"),
        owner: aura_workspace_domain::ResourceOwner {
            workspace_id: target.workspace,
            session_id: target.session,
            path: WorkspacePath::parse("deep").unwrap(),
        },
    };
    let held = owner
        .commit(
            &CommitRequest::one(outer_subtree.clone(), ResourceAccess::Write),
            target.session,
            ExecutionId::next(),
            None,
        )
        .unwrap();
    // The deep external target must really intersect the ancestor scope.
    let parent = aura_windows_platform::open_verified_existing(
        &deep,
        aura_windows_platform::NativeFileAccess::Metadata,
    )
    .unwrap();
    let label = aura_workspace_domain::ResourceOwner {
        workspace_id: target.workspace,
        session_id: target.session,
        path: WorkspacePath::parse("nested.txt").unwrap(),
    };
    let deep_resource = aura_workspace::external_slot(&parent, "nested.txt", label.clone())
        .expect("complete external slot");
    assert!(
        aura_workspace_domain::conflict_between(
            &ResourceRequest::new(deep_resource.clone(), ResourceAccess::Write),
            &ResourceRequest::new(outer_subtree, ResourceAccess::Write),
        )
        .is_some(),
        "a deep external target must intersect the real ancestor directory's subtree"
    );
    // An unrelated sibling directory is not serialised by that same scope.
    let sibling = fixture.root.path().join("outside");
    std::fs::create_dir_all(&sibling).unwrap();
    let sibling_parent = aura_windows_platform::open_verified_existing(
        &sibling,
        aura_windows_platform::NativeFileAccess::Metadata,
    )
    .unwrap();
    let sibling_resource = aura_workspace::external_slot(&sibling_parent, "nested.txt", label)
        .expect("complete sibling slot");
    assert!(
        aura_workspace_domain::conflict_between(
            &ResourceRequest::new(sibling_resource, ResourceAccess::Write),
            &ResourceRequest::new(deep_resource, ResourceAccess::Write),
        )
        .is_none(),
        "two unrelated external targets must stay parallel"
    );
    owner.release_commit(held);
    println!(
        "P04 external ancestry: deep target intersects ancestor subtree; unrelated sibling parallel"
    );
}

#[tokio::test]
async fn a_new_core_runtime_reconciles_actual_applying_and_published_effects_without_replay() {
    for stage in ["applying", "published"] {
        let mut fixture = crate::r03_tests::Fixture::new();
        let state = fixture.start().await;
        let before = state.options.runtime_instance_id;
        let target = seed(&services(&state).p04_required(), fixture.root.path());
        INTERRUPTION.with(|point| point.set(Some(stage)));
        let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            publish(&services(&state).p04_required(), &target)
        }));
        assert!(interrupted.is_err(), "the real publication must reach the armed stage");
        let id = operation(&fixture.paths.database_path);
        let record = services(&state).persistence.artifact_overwrite_journal(id).unwrap();
        assert_eq!(
            record.state,
            if stage == "applying" {
                ArtifactOverwriteJournalState::Applying
            } else {
                ArtifactOverwriteJournalState::Published
            }
        );
        let effect = std::fs::metadata(&target.path).unwrap().modified().unwrap();
        assert_eq!(std::fs::read(&target.path).unwrap(), b"new");
        drop(state);
        fixture.renew_runtime_instance();
        let restarted = fixture.start().await;
        assert_ne!(before, restarted.options.runtime_instance_id);
        let recovered = services(&restarted).persistence.artifact_overwrite_journal(id).unwrap();
        assert_eq!(recovered.state, ArtifactOverwriteJournalState::NeedsReconcile);
        assert_eq!(
            recovered.binding, record.binding,
            "recovery must retain the original operation binding"
        );
        assert_eq!(
            recovered.error_code.as_deref(),
            Some("core_restarted_external_state_ambiguous")
        );
        assert_eq!(
            std::fs::metadata(&target.path).unwrap().modified().unwrap(),
            effect,
            "startup must not replay a write"
        );
        println!(
            "P04 actual-effect restart: stage={stage} original={id} new_runtime={} state=needs_reconcile no_replay=true",
            restarted.options.runtime_instance_id
        );
    }
}
