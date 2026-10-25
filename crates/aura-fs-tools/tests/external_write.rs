//! Writing an authorized external file through the handles the permission owner
//! verified.
//!
//! These tests use real files, real handle verification and the real publication
//! path. They assert the properties the capability depends on: an external write
//! decides `expected_state` and encoding exactly as a workspace write does, it never
//! turns a create into a silent overwrite, it stays on the verified target, and it
//! leaves the workspace staging area out of it entirely.
use std::{sync::Arc, time::Duration};

use aura_domain_common::{
    CorrelationId, ExternalGrantId, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath,
};
use aura_fs_tools::{
    ExternalWriteKind, ExternalWriteTarget, FsExternalPatchRequest, FsExternalWriteRequest,
    FsToolError, FsToolPolicy, FsToolService,
};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{CatalogOptions, WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::{
    FsExpectedState, FsWriteFileRequest, SessionSummary, WorkspaceCreateCommand,
    WorkspaceFileTarget,
};

struct Harness {
    external: tempfile::TempDir,
    workspace_root: tempfile::TempDir,
    workspace: Arc<WorkspaceRuntime>,
    persistence: Arc<PersistenceCoordinator>,
    _data: tempfile::TempDir,
    workspace_id: WorkspaceId,
    session_id: SessionId,
}

impl Harness {
    fn new() -> Self {
        let external = tempfile::tempdir().expect("external root");
        let workspace_root = tempfile::tempdir().expect("workspace root");
        let data = tempfile::tempdir().expect("data root");
        let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
            data.path().join("aura.sqlite3"),
            data.path().join("artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("persistence startup")
        {
            PersistenceStartup::Ready(value) => Arc::new(value),
            PersistenceStartup::RecoveryRequired(value) => {
                panic!("unexpected recovery: {}", value.reason)
            }
        };
        let workspace = WorkspaceRuntime::open(
            Arc::clone(&persistence),
            WorkspaceRuntimeOptions {
                catalog: CatalogOptions::with_index_file_limit(64 * 1024 * 1024),
                watcher_coalesce: Duration::from_millis(20),
                watcher_poll_interval: Duration::from_secs(1),
                watcher_rescan_attempts: 1,
                ..WorkspaceRuntimeOptions::default()
            },
        )
        .expect("workspace runtime");
        let created = workspace
            .create(
                WorkspaceCreateCommand {
                    selected_root: workspace_root.path().display().to_string(),
                    display_name: "external write".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("create workspace");
        let workspace_id = created.workspace.workspace_id;
        let session_id = SessionId::new();
        let current = persistence.workspace(workspace_id).expect("current workspace");
        persistence
            .upsert_workspace_session(
                current,
                SessionSummary {
                    session_id,
                    workspace_id,
                    title: "external write session".to_owned(),
                    state: "idle".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("create session");
        Self {
            external,
            workspace_root,
            workspace,
            persistence,
            _data: data,
            workspace_id,
            session_id,
        }
    }

    fn service(&self) -> Arc<FsToolService> {
        FsToolService::new(
            Arc::clone(&self.workspace),
            Arc::clone(&self.persistence),
            FsToolPolicy::default(),
        )
        .expect("fs service")
    }

    fn path(&self, name: &str) -> std::path::PathBuf {
        self.external.path().join(name)
    }

    /// A replacement target whose handle was really verified, which is the only
    /// thing the write is allowed to trust.
    fn existing(&self, name: &str) -> ExternalWriteTarget {
        let path = self.path(name);
        // A directory handle cannot be opened for writing, and the point of that case
        // is the refusal, so a directory is verified through a read handle.
        let access = if path.is_dir() {
            aura_windows_platform::NativeFileAccess::Read
        } else {
            aura_windows_platform::NativeFileAccess::ReadWrite
        };
        let file = aura_windows_platform::open_verified_existing(&path, access)
            .expect("a verified external handle");
        ExternalWriteTarget {
            grant_id: Some(ExternalGrantId::new()),
            session_id: self.session_id,
            display_path: path.display().to_string(),
            relative_path: WorkspacePath::parse(name).expect("relative path"),
            kind: ExternalWriteKind::Existing(file),
        }
    }

    /// A create target: a verified parent directory plus the name to publish.
    fn create(&self, name: &str) -> ExternalWriteTarget {
        let path = self.path(name);
        let parent = aura_windows_platform::open_verified_publish_parent(self.external.path())
            .expect("a verified external parent");
        ExternalWriteTarget {
            grant_id: Some(ExternalGrantId::new()),
            session_id: self.session_id,
            display_path: path.display().to_string(),
            relative_path: WorkspacePath::parse(name).expect("relative path"),
            kind: ExternalWriteKind::New { parent, leaf_name: name.into(), final_path: path },
        }
    }

    /// The same bytes, written through the workspace transaction, so the two paths
    /// can be compared byte for byte.
    fn workspace_write(
        &self,
        service: &FsToolService,
        name: &str,
        content: &str,
        expected: FsExpectedState,
    ) {
        service
            .write_file(
                FsWriteFileRequest {
                    workspace_id: self.workspace_id,
                    session_id: self.session_id,
                    target: WorkspaceFileTarget { path: WorkspacePath::parse(name).expect("path") },
                    content: content.to_owned(),
                    expected,
                    encoding: None,
                    line_ending: None,
                },
                CorrelationId::new(),
            )
            .expect("workspace write");
    }

    fn entries(&self) -> Vec<String> {
        let mut names = std::fs::read_dir(self.external.path())
            .expect("external directory")
            .map(|entry| entry.expect("entry").file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        names.sort();
        names
    }
}

fn hash_of(text: &str) -> FsExpectedState {
    FsExpectedState::FileHash { hash: aura_domain_common::ContentHash::blake3(text.as_bytes()) }
}

/// An overwrite must decide its encoding exactly as a workspace write of the same
/// content does, because both go through one rule. The external file here carries a
/// UTF-8 BOM and CRLF; a new file must get UTF-8/LF instead.
#[test]
fn an_external_overwrite_keeps_the_encoding_a_workspace_overwrite_keeps() {
    let harness = Harness::new();
    let original = "\u{feff}第一行\r\nsecond line\r\n";
    std::fs::write(harness.path("note.txt"), original).expect("external file");
    std::fs::write(harness.workspace_root.path().join("note.txt"), original)
        .expect("workspace file");
    let service = harness.service();

    let external = service
        .write_external(FsExternalWriteRequest {
            target: harness.create("created.txt"),
            content: "新建\n".to_owned(),
            expected: FsExpectedState::Absent,
            encoding: Some(aura_workspace_domain::TextEncoding::Utf8),
            line_ending: Some(aura_workspace_domain::LineEnding::Lf),
        })
        .expect("external create");
    assert!(external.created);
    assert_eq!(
        std::fs::read(harness.path("created.txt")).expect("created bytes"),
        "新建\n".as_bytes()
    );

    let written = service
        .write_external(FsExternalWriteRequest {
            target: harness.existing("note.txt"),
            content: "第一行\r\nchanged line\r\n".to_owned(),
            expected: hash_of(original),
            encoding: None,
            line_ending: None,
        })
        .expect("external overwrite");
    harness.workspace_write(&service, "note.txt", "第一行\r\nchanged line\r\n", hash_of(original));

    let external_bytes = std::fs::read(harness.path("note.txt")).expect("external bytes");
    let workspace_bytes =
        std::fs::read(harness.workspace_root.path().join("note.txt")).expect("workspace bytes");
    assert_eq!(external_bytes, workspace_bytes, "one encoding rule for both paths");
    // The BOM and CRLF survived, which is what makes the two byte strings equal.
    assert!(external_bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
    assert!(external_bytes.windows(2).any(|pair| pair == b"\r\n"));
    assert!(!written.created);
    assert!(written.previous_hash.is_some());
    assert_ne!(written.previous_hash.as_ref(), Some(&written.logical_hash));
    assert_eq!(written.bytes_written, external_bytes.len() as u64);
}

/// The expected-state rule is the workspace one: a hash that no longer matches
/// refuses the write instead of overwriting what is there now.
#[test]
fn an_external_write_refuses_an_unexpected_hash_without_touching_the_file() {
    let harness = Harness::new();
    std::fs::write(harness.path("note.txt"), "current\n").expect("external file");
    let service = harness.service();
    let stale = aura_domain_common::ContentHash::blake3(b"something else");

    let error = service
        .write_external(FsExternalWriteRequest {
            target: harness.existing("note.txt"),
            content: "replacement\n".to_owned(),
            expected: FsExpectedState::FileHash { hash: stale },
            encoding: None,
            line_ending: None,
        })
        .expect_err("stale hash must be refused");
    assert!(
        matches!(error.kind(), FsToolError::ExternalExpectedState { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(std::fs::read_to_string(harness.path("note.txt")).expect("unchanged"), "current\n");
}

/// A create is never turned into an overwrite: if the name is taken, the call
/// conflicts and the existing content stays.
#[test]
fn creating_over_a_file_that_exists_is_a_conflict_not_an_overwrite() {
    let harness = Harness::new();
    std::fs::write(harness.path("taken.txt"), "existing content\n").expect("external file");
    let service = harness.service();

    let error = service
        .write_external(FsExternalWriteRequest {
            target: harness.create("taken.txt"),
            content: "clobbered\n".to_owned(),
            expected: FsExpectedState::Absent,
            encoding: Some(aura_workspace_domain::TextEncoding::Utf8),
            line_ending: Some(aura_workspace_domain::LineEnding::Lf),
        })
        .expect_err("an existing file is not a create target");
    assert!(
        matches!(error.kind(), FsToolError::ExternalExpectedState { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(
        std::fs::read_to_string(harness.path("taken.txt")).expect("unchanged"),
        "existing content\n"
    );
}

/// A completed external write leaves nothing behind: the temporary publication file
/// the platform uses is gone, because that publication is handle-relative and never
/// enters the workspace staging area.
#[test]
fn a_completed_external_write_leaves_no_staging_file_behind() {
    let harness = Harness::new();
    std::fs::write(harness.path("note.txt"), "before\n").expect("external file");
    let service = harness.service();

    service
        .write_external(FsExternalWriteRequest {
            target: harness.existing("note.txt"),
            content: "after\n".to_owned(),
            expected: hash_of("before\n"),
            encoding: None,
            line_ending: None,
        })
        .expect("external overwrite");
    service
        .write_external(FsExternalWriteRequest {
            target: harness.create("fresh.txt"),
            content: "new\n".to_owned(),
            expected: FsExpectedState::Absent,
            encoding: Some(aura_workspace_domain::TextEncoding::Utf8),
            line_ending: Some(aura_workspace_domain::LineEnding::Lf),
        })
        .expect("external create");

    assert_eq!(harness.entries(), vec!["fresh.txt".to_owned(), "note.txt".to_owned()]);
}

/// A file replaced after its handle was verified must not be written: the destination is
/// re-checked against the verified object before anything is published.
#[test]
fn an_external_write_refuses_a_target_that_was_replaced_after_verification() {
    let harness = Harness::new();
    std::fs::write(harness.path("note.txt"), "verified\n").expect("external file");
    let service = harness.service();
    let target = harness.existing("note.txt");
    // A different file object now carries the same name.
    std::fs::remove_file(harness.path("note.txt")).expect("remove verified file");
    std::fs::write(harness.path("note.txt"), "swapped in\n").expect("replacement file");

    let error = service
        .write_external(FsExternalWriteRequest {
            target,
            content: "written anyway\n".to_owned(),
            expected: hash_of("verified\n"),
            encoding: None,
            line_ending: None,
        })
        .expect_err("a replaced target must be refused");
    assert_eq!(
        error.stage,
        aura_fs_tools::ExternalPublishStage::Refused,
        "a destination refused before anything moved must say it published nothing: {error}"
    );
    assert!(
        matches!(error.kind(), FsToolError::ExternalFileReplaced { .. }),
        "the refusal must name the replaced object, not its contents: {error}"
    );
    assert_eq!(
        std::fs::read_to_string(harness.path("note.txt")).expect("unchanged"),
        "swapped in\n"
    );
}

/// A patch is applied to the verified file, decoded and re-encoded in that file's
/// own convention, and the hash it was built from is checked first.
#[test]
fn an_external_patch_applies_to_the_verified_file_and_keeps_its_encoding() {
    let harness = Harness::new();
    let original = "\u{feff}alpha\r\nbeta\r\n";
    std::fs::write(harness.path("note.txt"), original).expect("external file");
    let service = harness.service();
    let hash = aura_domain_common::ContentHash::blake3(original.as_bytes());

    let patched = service
        .patch_external(FsExternalPatchRequest {
            target: harness.existing("note.txt"),
            unified_diff:
                "--- a/note.txt\n+++ b/note.txt\n@@ -1,2 +1,2 @@\n alpha\n-beta\n+gamma\n"
                    .to_owned(),
            expected_hash: hash.clone(),
        })
        .expect("external patch");
    assert_eq!(patched.previous_hash.as_ref(), Some(&hash));
    assert!(!patched.created);
    let bytes = std::fs::read(harness.path("note.txt")).expect("patched bytes");
    assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]), "the BOM is preserved");
    assert!(bytes.windows(2).any(|pair| pair == b"\r\n"), "CRLF is preserved");
    let text = String::from_utf8(bytes).expect("utf-8");
    assert!(text.contains("alpha"));
    assert!(text.contains("gamma"));
    assert!(!text.contains("beta"));
}

/// A diff that does not match the real bytes, or one that was built from a different
/// hash, must fail without writing anything.
#[test]
fn a_patch_that_does_not_match_writes_nothing() {
    let harness = Harness::new();
    std::fs::write(harness.path("note.txt"), "alpha\r\nbeta\r\n").expect("external file");
    let service = harness.service();

    let stale = service
        .patch_external(FsExternalPatchRequest {
            target: harness.existing("note.txt"),
            unified_diff:
                "--- a/note.txt\n+++ b/note.txt\n@@ -1,2 +1,2 @@\n alpha\n-beta\n+gamma\n"
                    .to_owned(),
            expected_hash: aura_domain_common::ContentHash::blake3(b"another revision"),
        })
        .expect_err("a stale hash must be refused");
    assert!(
        matches!(stale.kind(), FsToolError::ExternalExpectedState { .. }),
        "unexpected error: {stale}"
    );

    let mismatched = service
        .patch_external(FsExternalPatchRequest {
            target: harness.existing("note.txt"),
            unified_diff: "--- a/note.txt\n+++ b/note.txt\n@@ -1,2 +1,2 @@\n nothing like this\n-is here\n+or here\n"
                .to_owned(),
            expected_hash: aura_domain_common::ContentHash::blake3(b"alpha\r\nbeta\r\n"),
        })
        .expect_err("hunks that do not match must be refused");
    assert!(
        matches!(mismatched.kind(), FsToolError::InvalidPatch(_)),
        "unexpected error: {mismatched}"
    );
    assert_eq!(
        std::fs::read_to_string(harness.path("note.txt")).expect("unchanged"),
        "alpha\r\nbeta\r\n"
    );
}

/// Deleting or creating through a patch is not implemented for external paths, and
/// saying so is better than applying half of it.
#[test]
fn an_external_patch_refuses_the_dev_null_form() {
    let harness = Harness::new();
    std::fs::write(harness.path("note.txt"), "alpha\n").expect("external file");
    let service = harness.service();

    let error = service
        .patch_external(FsExternalPatchRequest {
            target: harness.existing("note.txt"),
            unified_diff: "--- /dev/null\n+++ b/note.txt\n@@ -0,0 +1 @@\n+created\n".to_owned(),
            expected_hash: aura_domain_common::ContentHash::blake3(b"alpha\n"),
        })
        .expect_err("the /dev/null form is not implemented");
    assert!(matches!(error.kind(), FsToolError::InvalidPatch(_)), "unexpected error: {error}");
    assert_eq!(std::fs::read_to_string(harness.path("note.txt")).expect("unchanged"), "alpha\n");
}

/// A directory is not a file target on either path.
#[test]
fn a_directory_is_refused_as_an_external_write_target() {
    let harness = Harness::new();
    std::fs::create_dir(harness.path("folder")).expect("external directory");
    let service = harness.service();

    let error = service
        .write_external(FsExternalWriteRequest {
            target: harness.existing("folder"),
            content: "not a file\n".to_owned(),
            expected: hash_of(""),
            encoding: None,
            line_ending: None,
        })
        .expect_err("a directory is not a file target");
    assert!(matches!(error.kind(), FsToolError::InvalidRequest(_)), "unexpected error: {error}");
    assert!(harness.path("folder").is_dir());
}

/// One of two writers sharing a destination must lose, and losing must mean *not writing*.
///
/// Both callers verify the same original bytes and ask to replace them, which is what two
/// calls to the same file with the same expected state really look like. The final
/// observation each publication makes is the only thing that can tell them apart, so the
/// loser has to refuse instead of overwriting the winner with content derived from a state
/// that no longer exists. The test does not require a particular interleaving — either writer
/// may win — but it does require that exactly one does and that the file holds that one's
/// bytes.
#[test]
fn only_one_of_two_writers_sharing_a_destination_may_publish() {
    let harness = Harness::new();
    std::fs::write(harness.path("contended.txt"), "original\n").expect("external file");
    let service = harness.service();
    let expected = hash_of("original\n");

    let mut targets = Vec::new();
    for _ in 0..2 {
        targets.push(harness.existing("contended.txt"));
    }
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let mut writers = Vec::new();
    for (index, target) in targets.into_iter().enumerate() {
        let service = std::sync::Arc::clone(&service);
        let expected = expected.clone();
        let barrier = std::sync::Arc::clone(&barrier);
        writers.push(std::thread::spawn(move || {
            // Both writers are past their read and about to publish when the race begins.
            barrier.wait();
            let body = format!("writer-{index}\n");
            let outcome = service.write_external(FsExternalWriteRequest {
                target,
                content: body.clone(),
                expected,
                encoding: Some(aura_workspace_domain::TextEncoding::Utf8),
                line_ending: Some(aura_workspace_domain::LineEnding::Lf),
            });
            (index, body, outcome)
        }));
    }
    let results =
        writers.into_iter().map(|writer| writer.join().expect("writer")).collect::<Vec<_>>();

    let winners = results.iter().filter(|(_, _, outcome)| outcome.is_ok()).collect::<Vec<_>>();
    let losers = results.iter().filter(|(_, _, outcome)| outcome.is_err()).collect::<Vec<_>>();
    assert_eq!(winners.len(), 1, "exactly one writer may publish: {results:?}");
    assert_eq!(losers.len(), 1, "the other writer must be refused: {results:?}");

    let (_, winner_body, _) = winners[0];
    assert_eq!(
        std::fs::read_to_string(harness.path("contended.txt")).expect("final content"),
        *winner_body,
        "the file must hold the winner's bytes, not a mix and not the loser's"
    );
    let (_, _, error) = losers[0];
    let error = error.as_ref().expect_err("the loser reported an error");
    assert!(
        matches!(
            error.stage,
            aura_fs_tools::ExternalPublishStage::Refused
                | aura_fs_tools::ExternalPublishStage::NeverPublished
        ),
        "the loser must report that it published nothing, whatever stopped it: {error}"
    );
    assert!(
        matches!(error.kind(), FsToolError::ExternalExpectedState { .. }),
        "the loser must be refused as a conflict, not as an unexplained platform fault: {error}"
    );
}

/// The final observation happens inside the publication, so a change made after the caller's
/// own read is caught and the file keeps the newer bytes.
#[test]
fn a_change_between_the_read_and_the_publication_is_refused() {
    let harness = Harness::new();
    std::fs::write(harness.path("late.txt"), "expected body\n").expect("external file");
    let service = harness.service();
    let target = harness.existing("late.txt");
    let expected = hash_of("expected body\n");

    // The hook fires when this call has already read the file and the publication has not yet
    // made its own final checks — exactly the window another process would use. Holding it
    // open makes the interleaving a fact of the test rather than a matter of timing. It is
    // bound to this one destination, so a test running beside it is not stalled by it.
    let reached = std::sync::Arc::new(std::sync::Barrier::new(2));
    let release = std::sync::Arc::new(std::sync::Barrier::new(2));
    let hook_reached = std::sync::Arc::clone(&reached);
    let hook_release = std::sync::Arc::clone(&release);
    let _hook = aura_fs_tools::external_faults::install(
        &harness.path("late.txt").display().to_string(),
        aura_fs_tools::external_faults::ExternalPublishRendezvous {
            before_final_checks: Some(std::sync::Arc::new(move |_path: &str| {
                hook_reached.wait();
                hook_release.wait();
            })),
            before_platform_publish: None,
        },
    );

    let writer = {
        let service = std::sync::Arc::clone(&service);
        std::thread::spawn(move || {
            service.write_external(FsExternalWriteRequest {
                target,
                content: "written by this call\n".to_owned(),
                expected,
                encoding: Some(aura_workspace_domain::TextEncoding::Utf8),
                line_ending: Some(aura_workspace_domain::LineEnding::Lf),
            })
        })
    };
    // The writer is now held between its observation and its publication, so an outside
    // change lands in the window the design says is covered.
    reached.wait();
    std::fs::write(harness.path("late.txt"), "changed by someone else\n").expect("outside change");
    release.wait();

    let outcome = writer.join().expect("writer");
    assert!(
        outcome.is_err(),
        "a publication whose destination changed under it must not report success"
    );
    let error = outcome.as_ref().expect_err("checked above");
    assert_eq!(
        error.stage,
        aura_fs_tools::ExternalPublishStage::Refused,
        "the change was seen before anything was renamed, so nothing was published: {error}"
    );
    assert!(
        matches!(error.kind(), FsToolError::ExternalExpectedState { .. }),
        "a stale expected state must be reported as the conflict it is: {error}"
    );
    assert_eq!(
        std::fs::read_to_string(harness.path("late.txt")).expect("final content"),
        "changed by someone else\n",
        "the refused call must not have overwritten the newer content"
    );
}

/// A rewrite that lands **before** the publication takes the destination is caught by the
/// content check, even though the file object never changed.
///
/// This is the deterministic form of the stale-expected-state case the review asked for: the
/// outside writer rewrites the file in place, which keeps the file's identity, and Aura must
/// refuse rather than overwrite content it never agreed to replace.
#[test]
fn an_in_place_rewrite_before_the_publication_is_refused() {
    let harness = Harness::new();
    std::fs::write(harness.path("rewritten.txt"), "original body\n").expect("external file");
    let service = harness.service();
    let target = harness.existing("rewritten.txt");
    let expected = hash_of("original body\n");
    // The file object the call was verified against, recorded so the rewrite below can be
    // shown to have kept it.
    let identity_before =
        aura_windows_platform::inspect_existing_path(&harness.path("rewritten.txt"), true)
            .expect("inspect the original object")
            .identity;

    let reached = std::sync::Arc::new(std::sync::Barrier::new(2));
    let release = std::sync::Arc::new(std::sync::Barrier::new(2));
    let hook_reached = std::sync::Arc::clone(&reached);
    let hook_release = std::sync::Arc::clone(&release);
    let _hook = aura_fs_tools::external_faults::install(
        &harness.path("rewritten.txt").display().to_string(),
        aura_fs_tools::external_faults::ExternalPublishRendezvous {
            before_final_checks: Some(std::sync::Arc::new(move |_path: &str| {
                hook_reached.wait();
                hook_release.wait();
            })),
            before_platform_publish: None,
        },
    );

    let writer = {
        let service = std::sync::Arc::clone(&service);
        std::thread::spawn(move || {
            service.write_external(FsExternalWriteRequest {
                target,
                content: "Aura body\n".to_owned(),
                expected,
                encoding: Some(aura_workspace_domain::TextEncoding::Utf8),
                line_ending: Some(aura_workspace_domain::LineEnding::Lf),
            })
        })
    };
    // Rewrite in place through the same path, which keeps the file object — and therefore its
    // identity — exactly as it was.
    reached.wait();
    std::fs::write(harness.path("rewritten.txt"), "outside body\n").expect("in-place rewrite");
    let identity_after =
        aura_windows_platform::inspect_existing_path(&harness.path("rewritten.txt"), true)
            .expect("inspect the rewritten object")
            .identity;
    assert_eq!(
        identity_after, identity_before,
        "the rewrite must not have replaced the file object, or this is not the in-place case"
    );
    release.wait();

    let outcome = writer.join().expect("writer");
    let error = outcome.as_ref().expect_err("a stale expected state must be refused");
    assert_eq!(
        error.stage,
        aura_fs_tools::ExternalPublishStage::Refused,
        "nothing was renamed, so the stage must say so: {error}"
    );
    assert!(
        matches!(error.kind(), FsToolError::ExternalExpectedState { .. }),
        "the refusal must name the expected state that no longer holds: {error}"
    );
    assert_eq!(
        std::fs::read_to_string(harness.path("rewritten.txt")).expect("final content"),
        "outside body\n",
        "the newer content must survive"
    );
}

/// A publication may not claim success by publishing bytes that are not the ones it validated.
///
/// The hook here holds the writer *after* its final checks, so an outside change arrives too
/// late for those checks to see it. The write still has to be reported honestly: either it
/// published — and the file holds its bytes — or it did not. What it must never do is report
/// the new content and leave someone else's, and the stage it records has to match the file.
#[test]
fn a_late_change_leaves_the_result_consistent_with_the_file() {
    let harness = Harness::new();
    std::fs::write(harness.path("racing.txt"), "start\n").expect("external file");
    let service = harness.service();
    let target = harness.existing("racing.txt");
    let expected = hash_of("start\n");

    let reached = std::sync::Arc::new(std::sync::Barrier::new(2));
    let release = std::sync::Arc::new(std::sync::Barrier::new(2));
    let hook_reached = std::sync::Arc::clone(&reached);
    let hook_release = std::sync::Arc::clone(&release);
    let _hook = aura_fs_tools::external_faults::install(
        &harness.path("racing.txt").display().to_string(),
        aura_fs_tools::external_faults::ExternalPublishRendezvous {
            before_final_checks: None,
            before_platform_publish: Some(std::sync::Arc::new(move |_path: &str| {
                hook_reached.wait();
                hook_release.wait();
            })),
        },
    );

    let writer = {
        let service = std::sync::Arc::clone(&service);
        std::thread::spawn(move || {
            service.write_external(FsExternalWriteRequest {
                target,
                content: "our body\n".to_owned(),
                expected,
                encoding: Some(aura_workspace_domain::TextEncoding::Utf8),
                line_ending: Some(aura_workspace_domain::LineEnding::Lf),
            })
        })
    };
    reached.wait();
    // The destination object is replaced outright, so the platform's own identity comparison
    // is what has to refuse the publication.
    std::fs::remove_file(harness.path("racing.txt")).expect("remove verified file");
    std::fs::write(harness.path("racing.txt"), "another object\n").expect("replacement file");
    release.wait();

    let outcome = writer.join().expect("writer");
    let content = std::fs::read_to_string(harness.path("racing.txt")).expect("final content");
    match outcome {
        Ok(written) => {
            assert_eq!(
                content, "our body\n",
                "a reported publication must be the content on disk (hash {})",
                written.logical_hash
            );
        }
        Err(error) => {
            assert_eq!(
                content, "another object\n",
                "a refused publication must leave the other object alone: {error}"
            );
            // The destination was replaced under the publication, so the identity check
            // refused it before anything was renamed: that is a refusal, not a publication
            // that started and stopped.
            assert_eq!(
                error.stage,
                aura_fs_tools::ExternalPublishStage::Refused,
                "a publication that never started must say so: {error}"
            );
            assert!(
                matches!(error.kind(), FsToolError::ExternalFileReplaced { .. }),
                "the refusal must name the replaced object: {error}"
            );
        }
    }
}
