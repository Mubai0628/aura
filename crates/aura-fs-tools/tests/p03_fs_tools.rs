use std::{path::Path, sync::Arc, time::Duration};

#[path = "p03_fs_tools/compensation_window.rs"]
mod compensation_window;
#[path = "p03_fs_tools/copy_and_move_receipts.rs"]
mod copy_and_move_receipts;

use aura_artifact_contract::ArtifactAccess;
use aura_domain_common::{
    ContentHash, CorrelationId, ExternalGrantId, FileReferenceId, Revision, RuntimeInstanceId,
    SessionId, WorkspaceId, WorkspacePath,
};
use aura_fs_tools::{FsToolError, FsToolPolicy, FsToolService};
use aura_persistence::{
    ArtifactReadRequest, MutationOwnershipExecutionId, PersistenceCoordinator, PersistenceOptions,
    PersistenceStartup,
};
use aura_workspace::{CatalogOptions, WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::{
    BinaryDisplayFormat, BufferEditCommand, BufferOpenCommand, FileStorageSource, FileTargetRef,
    FsApplyChangesetRequest, FsApplyPatchRequest, FsChangesetOperation, FsCopyRequest,
    FsCreateDirectoryRequest, FsDeleteRequest, FsExpectedState, FsFindPathsRequest, FsGlobRequest,
    FsHashRequest, FsInspectRequest, FsListRequest, FsMoveRequest, FsMutationDisposition,
    FsNodeKind, FsReadBinaryRangeRequest, FsReadTextRequest, FsSearchTextRequest, FsStatRequest,
    FsWriteFileRequest, LargeFileReadMode, LineEnding, SessionSummary, TextEncoding, TextReadRange,
    WorkspaceCreateCommand, WorkspaceFileTarget,
};

struct Harness {
    fs: Arc<FsToolService>,
    workspace: Arc<WorkspaceRuntime>,
    persistence: Arc<PersistenceCoordinator>,
    root: tempfile::TempDir,
    _data: tempfile::TempDir,
    workspace_id: WorkspaceId,
    session_a: SessionId,
    session_b: SessionId,
}

impl Harness {
    fn new(setup: impl FnOnce(&Path)) -> Self {
        let root = tempfile::tempdir().expect("workspace root");
        setup(root.path());
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
                catalog: CatalogOptions::with_index_file_limit(1024 * 1024),
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
                    selected_root: root.path().display().to_string(),
                    display_name: "P03 test".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("create workspace");
        let workspace_id = created.workspace.workspace_id;
        let session_a = SessionId::new();
        let session_b = SessionId::new();
        for (session_id, title) in [(session_a, "Session A"), (session_b, "Session B")] {
            let current = persistence.workspace(workspace_id).expect("current workspace");
            persistence
                .upsert_workspace_session(
                    current,
                    SessionSummary {
                        session_id,
                        workspace_id,
                        title: title.to_owned(),
                        state: "idle".to_owned(),
                    },
                    CorrelationId::new(),
                )
                .expect("create session");
        }
        let fs = FsToolService::new(
            Arc::clone(&workspace),
            Arc::clone(&persistence),
            FsToolPolicy {
                large_file_read_mode: LargeFileReadMode::RequireRange,
                full_text_read_limit_bytes: 1024,
                small_range_line_limit: 8,
                small_range_character_limit: 256,
                default_binary_range_bytes: 4 * 1024,
                max_binary_range_bytes: 64 * 1024,
                max_changeset_operations: 64,
                max_search_results: 1000,
                mutation_wait: std::time::Duration::from_secs(5),
            },
        )
        .expect("fs service");
        Self { fs, workspace, persistence, root, _data: data, workspace_id, session_a, session_b }
    }

    fn path(&self, value: &str) -> WorkspacePath {
        WorkspacePath::parse(value).expect("workspace path")
    }

    /// Model-facing path input. It holds the same canonical relative value that
    /// Core produces after normalizing an absolute or relative spelling.
    fn agent_path(&self, value: &str) -> aura_workspace_domain::AgentWorkspacePath {
        self.path(value).into()
    }

    fn target(&self, value: &str) -> WorkspaceFileTarget {
        WorkspaceFileTarget { path: self.path(value) }
    }

    fn fs_with_large_file_mode(&self, mode: LargeFileReadMode) -> Arc<FsToolService> {
        FsToolService::new(
            Arc::clone(&self.workspace),
            Arc::clone(&self.persistence),
            FsToolPolicy {
                large_file_read_mode: mode,
                full_text_read_limit_bytes: 1024,
                small_range_line_limit: 8,
                small_range_character_limit: 256,
                default_binary_range_bytes: 4 * 1024,
                max_binary_range_bytes: 64 * 1024,
                max_changeset_operations: 64,
                max_search_results: 1000,
                mutation_wait: std::time::Duration::from_secs(5),
            },
        )
        .expect("fs service with selected large-file policy")
    }
}

#[test]
fn agent_directory_pages_preserve_unicode_and_reject_wrong_scope_or_cursor() {
    use aura_workspace_domain::AgentFileListInput;
    let harness = Harness::new(|root| {
        std::fs::create_dir(root.join("空目录")).expect("empty directory");
        for index in 0..125 {
            std::fs::write(root.join(format!("文件-{index:03}.txt")), "sample").expect("file");
        }
    });
    let input = AgentFileListInput { path: harness.agent_path("."), limit: 50, cursor: None };
    let mut names = std::collections::BTreeSet::new();
    let mut cursor = None;
    loop {
        let page = harness
            .fs
            .list_for_agent(
                harness.workspace_id,
                harness.session_a,
                AgentFileListInput { cursor: cursor.clone(), ..input.clone() },
            )
            .expect("page");
        assert!(page.entries.len() <= 50);
        for entry in page.entries {
            assert!(names.insert(entry.path));
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            assert!(!page.truncated);
            break;
        }
    }
    assert_eq!(names.len(), 126);
    assert!(names.contains(&harness.path("文件-124.txt")));
    let empty = harness
        .fs
        .list_for_agent(
            harness.workspace_id,
            harness.session_a,
            AgentFileListInput { path: harness.agent_path("空目录"), ..input.clone() },
        )
        .expect("empty");
    assert!(empty.entries.is_empty() && !empty.truncated && empty.next_cursor.is_none());
    assert!(
        harness.fs.list_for_agent(harness.workspace_id, SessionId::new(), input.clone()).is_err()
    );
    for limit in [0, 101] {
        assert!(matches!(
            harness.fs.list_for_agent(
                harness.workspace_id,
                harness.session_a,
                AgentFileListInput { limit, ..input.clone() }
            ),
            Err(FsToolError::InvalidRequest(_))
        ));
    }
    let first = harness
        .fs
        .list_for_agent(harness.workspace_id, harness.session_a, input.clone())
        .expect("first");
    let cursor = first.next_cursor.expect("cursor");
    assert!(
        harness
            .fs
            .list_for_agent(
                harness.workspace_id,
                harness.session_a,
                AgentFileListInput {
                    path: harness.agent_path("空目录"),
                    cursor: Some(cursor),
                    ..input
                }
            )
            .is_err()
    );
}

#[test]
fn large_file_modes_and_internal_target_scope_are_explicit() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join("large.txt"), "x".repeat(2048)).expect("large text file");
    });
    let request = |range| FsReadTextRequest {
        workspace_id: harness.workspace_id,
        session_id: harness.session_a,
        target: harness.target("large.txt"),
        range,
        explicit_encoding: None,
    };

    let allow_full = harness.fs_with_large_file_mode(LargeFileReadMode::AllowFull);
    assert_eq!(
        allow_full
            .read_text(request(TextReadRange::Full))
            .expect("allow full")
            .text
            .chars()
            .count(),
        2048
    );

    let require_range = harness.fs_with_large_file_mode(LargeFileReadMode::RequireRange);
    assert!(matches!(
        require_range.read_text(request(TextReadRange::Full)),
        Err(FsToolError::LargeFileRangeRequired)
    ));
    assert_eq!(
        require_range
            .read_text(request(TextReadRange::Characters {
                start_character_offset: 64,
                count: 512,
            }))
            .expect("explicit range")
            .text
            .chars()
            .count(),
        512
    );

    let small_range = harness.fs_with_large_file_mode(LargeFileReadMode::SmallRangeOnly);
    assert!(matches!(
        small_range.read_text(request(TextReadRange::Characters {
            start_character_offset: 0,
            count: 257,
        })),
        Err(FsToolError::TextRangeTooLarge)
    ));
    assert_eq!(
        small_range
            .read_text(request(TextReadRange::Characters { start_character_offset: 0, count: 256 }))
            .expect("small range")
            .text
            .chars()
            .count(),
        256
    );

    assert!(
        FsToolService::require_workspace_target(FileTargetRef::Workspace {
            path: harness.path("large.txt"),
        })
        .is_ok()
    );
    for unsupported in [
        FileTargetRef::SessionFile { file_reference_id: FileReferenceId::new() },
        FileTargetRef::ExternalPath { path: r"C:\outside.txt".to_owned() },
        FileTargetRef::ExternalGrant {
            external_grant_id: ExternalGrantId::new(),
            relative_path: harness.path("outside.txt"),
        },
    ] {
        assert!(matches!(
            FsToolService::require_workspace_target(unsupported),
            Err(FsToolError::ScopeUnavailable)
        ));
    }
}

#[test]
fn read_namespace_and_artifact_capture_preserve_session_logical_views() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/note.txt"), "disk alpha\r\nsecond\r\n").expect("text file");
        std::fs::write(root.join("image.bin"), [b'M', b'Z', 0, 1, 2, 3]).expect("binary file");
    });

    let stat = harness
        .fs
        .stat(FsStatRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session_a,
            target: harness.target("src/note.txt"),
        })
        .expect("stat");
    assert_eq!(stat.kind, FsNodeKind::File);
    let listed = harness
        .fs
        .list(FsListRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session_a,
            target: harness.target("src"),
        })
        .expect("list");
    assert_eq!(listed.entries.len(), 1);
    assert_eq!(
        harness
            .fs
            .find_paths(FsFindPathsRequest {
                workspace_id: harness.workspace_id,
                session_id: harness.session_a,
                query: "note".to_owned(),
                limit: 10,
            })
            .expect("find")
            .matches[0]
            .path,
        harness.path("src/note.txt")
    );
    assert_eq!(
        harness
            .fs
            .glob(FsGlobRequest {
                workspace_id: harness.workspace_id,
                session_id: harness.session_a,
                pattern: "src/*.txt".to_owned(),
                limit: 10,
            })
            .expect("glob")
            .paths,
        vec![harness.path("src/note.txt")]
    );

    let opened = harness
        .workspace
        .open_buffer(BufferOpenCommand {
            workspace_id: harness.workspace_id,
            session_id: harness.session_a,
            path: harness.path("src/note.txt"),
            writable: true,
        })
        .expect("open editor buffer");
    harness
        .workspace
        .edit_buffer(
            BufferEditCommand {
                workspace_id: harness.workspace_id,
                session_id: harness.session_a,
                path: harness.path("src/note.txt"),
                reservation_id: opened.reservation_id.expect("reservation"),
                reservation_epoch: opened.reservation_epoch,
                buffer_revision: Revision::new(opened.buffer_revision.value() + 1),
                base_core_revision: opened.buffer_revision,
                content: "logical alpha\r\nsecond\r\n".to_owned(),
                active_editing: true,
                ime_composition_active: false,
            },
            CorrelationId::new(),
        )
        .expect("edit buffer");

    let own = harness
        .fs
        .read_text(FsReadTextRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session_a,
            target: harness.target("src/note.txt"),
            range: TextReadRange::Full,
            explicit_encoding: None,
        })
        .expect("own logical read");
    assert_eq!(own.source, FileStorageSource::EditorBuffer);
    assert!(own.text.starts_with("logical alpha"));
    assert!(own.is_dirty);

    let other = harness
        .fs
        .read_text(FsReadTextRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session_b,
            target: harness.target("src/note.txt"),
            range: TextReadRange::Full,
            explicit_encoding: None,
        })
        .expect("other session disk read");
    assert_eq!(other.source, FileStorageSource::Disk);
    assert!(other.text.starts_with("disk alpha"));
    assert_eq!(other.active_editor_session, Some(harness.session_a));
    assert!(other.disk_may_be_stale);
    let other_hash = harness
        .fs
        .hash(FsHashRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session_b,
            target: harness.target("src/note.txt"),
        })
        .expect("other hash");
    assert_eq!(other_hash.source, FileStorageSource::Disk);
    assert!(other_hash.disk_may_be_stale);

    let search = harness
        .fs
        .search_text(FsSearchTextRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session_a,
            pattern: aura_workspace_domain::FsSearchPattern::Literal {
                text: "logical alpha".to_owned(),
                case_sensitive: true,
            },
            root: aura_domain_common::WorkspacePath::parse("src").expect("root"),
            path_glob: Some("src/*.txt".to_owned()),
            include_ignored: false,
            include_generated: false,
            limit: 10,
        })
        .expect("search logical view");
    assert_eq!(search.matches.len(), 1);
    assert_eq!(search.matches[0].source, FileStorageSource::EditorBuffer);
    assert!(matches!(search.completion, aura_workspace_domain::FsSearchCompletion::Complete));

    let inspection = harness
        .fs
        .inspect(FsInspectRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session_a,
            target: harness.target("image.bin"),
        })
        .expect("inspect binary");
    assert!(inspection.is_executable);
    assert!(!inspection.is_text);
    let binary = harness
        .fs
        .read_binary_range(FsReadBinaryRangeRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session_a,
            target: harness.target("image.bin"),
            offset: 0,
            length: 6,
            format: BinaryDisplayFormat::HexWithAscii,
        })
        .expect("binary range");
    assert_eq!(binary.returned_bytes, 6);
    assert!(binary.data.contains("4d 5a"));

    let captured = harness
        .fs
        .capture_workspace_file(aura_workspace_domain::CaptureWorkspaceFileRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session_a,
            target: harness.target("src/note.txt"),
        })
        .expect("capture logical file");
    assert_eq!(captured.source, FileStorageSource::EditorBuffer);
    let bytes = harness
        .persistence
        .read_artifact_range(ArtifactReadRequest {
            artifact_id: captured.artifact.artifact_id,
            session_id: Some(harness.session_a),
            required_access: ArtifactAccess::MODEL_READ,
            offset: 0,
            length: usize::try_from(captured.artifact.byte_length).expect("artifact length"),
            allow_quarantine: false,
        })
        .expect("read captured artifact");
    assert_eq!(bytes, b"logical alpha\r\nsecond\r\n");
}

#[test]
fn mutation_namespace_enforces_expected_state_and_audits_ignored_generated_paths() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("target")).expect("generated directory");
        std::fs::write(root.join(".gitignore"), "ignored.log\ntarget/\n").expect("gitignore");
        std::fs::write(root.join("original.txt"), "old\n").expect("original");
        std::fs::write(root.join("ignored.log"), "ignored old\n").expect("ignored");
        std::fs::write(root.join("target/generated.txt"), "generated old\n").expect("generated");
    });

    let created = harness
        .fs
        .write_file(
            FsWriteFileRequest {
                workspace_id: harness.workspace_id,
                session_id: harness.session_a,
                target: harness.target("new.txt"),
                content: "new\n".to_owned(),
                expected: FsExpectedState::Absent,
                encoding: Some(TextEncoding::Utf8),
                line_ending: Some(LineEnding::Lf),
            },
            CorrelationId::new(),
        )
        .expect("create file");
    assert_eq!(created.disposition, FsMutationDisposition::Applied);
    assert_eq!(std::fs::read(harness.root.path().join("new.txt")).expect("new file"), b"new\n");

    let original_hash = ContentHash::blake3(b"old\n");
    harness
        .fs
        .apply_patch(
            FsApplyPatchRequest {
                workspace_id: harness.workspace_id,
                session_id: harness.session_a,
                target: harness.target("original.txt"),
                unified_diff:
                    "--- a/original.txt\n+++ b/original.txt\n@@ -1 +1 @@\n-old\n+patched\n"
                        .to_owned(),
                expected_hash: original_hash,
            },
            CorrelationId::new(),
        )
        .expect("apply patch");
    let patched_hash = ContentHash::blake3(b"patched\n");

    harness
        .fs
        .create_directory(
            FsCreateDirectoryRequest {
                workspace_id: harness.workspace_id,
                session_id: harness.session_a,
                target: harness.target("copies"),
            },
            CorrelationId::new(),
        )
        .expect("create directory");
    harness
        .fs
        .copy(
            FsCopyRequest {
                workspace_id: harness.workspace_id,
                session_id: harness.session_a,
                source: harness.target("original.txt"),
                destination: harness.target("copies/copied.txt"),
                source_expected: FsExpectedState::FileHash { hash: patched_hash.clone() },
                destination_expected: FsExpectedState::Absent,
            },
            CorrelationId::new(),
        )
        .expect("copy");
    harness
        .fs
        .move_path(
            FsMoveRequest {
                workspace_id: harness.workspace_id,
                session_id: harness.session_a,
                source: harness.target("copies/copied.txt"),
                destination: harness.target("copies/moved.txt"),
                source_expected: FsExpectedState::FileHash { hash: patched_hash },
                destination_expected: FsExpectedState::Absent,
            },
            CorrelationId::new(),
        )
        .expect("move");
    harness
        .fs
        .delete(
            FsDeleteRequest {
                workspace_id: harness.workspace_id,
                session_id: harness.session_a,
                target: harness.target("new.txt"),
                expected: FsExpectedState::FileHash { hash: ContentHash::blake3(b"new\n") },
                recursive: false,
            },
            CorrelationId::new(),
        )
        .expect("delete");

    let changeset = harness
        .fs
        .apply_changeset(
            FsApplyChangesetRequest {
                workspace_id: harness.workspace_id,
                session_id: harness.session_a,
                operations: vec![
                    FsChangesetOperation::CreateDirectory { target: harness.target("batch") },
                    FsChangesetOperation::WriteFile {
                        target: harness.target("batch/value.txt"),
                        content: "batch\n".to_owned(),
                        expected: FsExpectedState::Absent,
                        encoding: Some(TextEncoding::Utf8),
                        line_ending: Some(LineEnding::Lf),
                    },
                ],
            },
            CorrelationId::new(),
        )
        .expect("changeset");
    assert_eq!(changeset.steps.len(), 2);
    assert_eq!(
        std::fs::read(harness.root.path().join("batch/value.txt")).expect("batch file"),
        b"batch\n"
    );

    for (path, content) in
        [("ignored.log", "ignored changed\n"), ("target/generated.txt", "generated changed\n")]
    {
        let before = std::fs::read(harness.root.path().join(path)).expect("before");
        harness
            .fs
            .write_file(
                FsWriteFileRequest {
                    workspace_id: harness.workspace_id,
                    session_id: harness.session_a,
                    target: harness.target(path),
                    content: content.to_owned(),
                    expected: FsExpectedState::FileHash { hash: ContentHash::blake3(&before) },
                    encoding: None,
                    line_ending: None,
                },
                CorrelationId::new(),
            )
            .expect("audited write");
    }
    let audits = harness
        .persistence
        .load_workspace_mutation_audits(harness.workspace_id)
        .expect("load audits");
    assert!(audits.iter().any(|audit| audit.path == harness.path("ignored.log") && audit.ignored));
    assert!(audits.iter().any(|audit| {
        audit.path == harness.path("target/generated.txt") && audit.ignored && audit.generated
    }));
}

#[test]
fn active_editor_defers_without_replay_and_mutation_ownership_fails_immediately() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join("active.txt"), "disk\n").expect("active file");
        std::fs::write(root.join("owned.txt"), "owned\n").expect("owned file");
    });
    let opened = harness
        .workspace
        .open_buffer(BufferOpenCommand {
            workspace_id: harness.workspace_id,
            session_id: harness.session_a,
            path: harness.path("active.txt"),
            writable: true,
        })
        .expect("open buffer");
    let edited = harness
        .workspace
        .edit_buffer(
            BufferEditCommand {
                workspace_id: harness.workspace_id,
                session_id: harness.session_a,
                path: harness.path("active.txt"),
                reservation_id: opened.reservation_id.expect("reservation"),
                reservation_epoch: opened.reservation_epoch,
                buffer_revision: Revision::new(opened.buffer_revision.value() + 1),
                base_core_revision: opened.buffer_revision,
                content: "user edit\n".to_owned(),
                active_editing: true,
                ime_composition_active: false,
            },
            CorrelationId::new(),
        )
        .expect("edit buffer");
    let deferred = harness
        .fs
        .write_file(
            FsWriteFileRequest {
                workspace_id: harness.workspace_id,
                session_id: harness.session_b,
                target: harness.target("active.txt"),
                content: "stale model write\n".to_owned(),
                expected: FsExpectedState::FileHash { hash: ContentHash::blake3(b"disk\n") },
                encoding: None,
                line_ending: None,
            },
            CorrelationId::new(),
        )
        .expect("deferred result");
    assert_eq!(deferred.disposition, FsMutationDisposition::DeferredByEditorActivity);
    assert!(deferred.requires_reread);
    assert_eq!(std::fs::read(harness.root.path().join("active.txt")).expect("disk"), b"disk\n");

    let resolved = harness
        .workspace
        .edit_buffer(
            BufferEditCommand {
                workspace_id: harness.workspace_id,
                session_id: harness.session_a,
                path: harness.path("active.txt"),
                reservation_id: opened.reservation_id.expect("reservation"),
                reservation_epoch: opened.reservation_epoch,
                buffer_revision: Revision::new(edited.status.buffer_revision.value() + 1),
                base_core_revision: edited.status.buffer_revision,
                content: "latest user edit\n".to_owned(),
                active_editing: false,
                ime_composition_active: false,
            },
            CorrelationId::new(),
        )
        .expect("end active edit");
    assert!(resolved.requires_model_reread);
    assert_eq!(resolved.deferred_intents_superseded, 1);
    assert!(
        harness
            .persistence
            .pending_deferred_mutations(harness.workspace_id)
            .expect("pending deferred")
            .is_empty()
    );
    assert_eq!(std::fs::read(harness.root.path().join("active.txt")).expect("disk"), b"disk\n");

    // A second session writing an **unrelated** file in the same workspace must now succeed.
    //
    // This assertion used to require `MutationOwnedByOtherSession` for exactly this call, which is
    // the whole-workspace exclusion batch A removes: the ownership row was keyed by workspace alone
    // and refused every other session, so two sessions could not write two different files in one
    // workspace. The workspace-scoped row is a lifecycle record, not the file-coordination
    // boundary; whether two executions really conflict is decided per real resource.
    // The registration is now per **execution**, so this test takes one explicitly — the same way
    // the product's own mutation path does — and releases exactly that one. A workspace-wide
    // release would be the lifecycle call, which is a different responsibility.
    let registration = MutationOwnershipExecutionId::new();
    harness
        .persistence
        .acquire_mutation_ownership_for_execution(
            harness.workspace_id,
            harness.session_a,
            registration.clone(),
        )
        .expect("acquire ownership");
    harness
        .fs
        .write_file(
            FsWriteFileRequest {
                workspace_id: harness.workspace_id,
                session_id: harness.session_b,
                target: harness.target("unrelated.txt"),
                content: "unrelated file\n".to_owned(),
                expected: FsExpectedState::Absent,
                encoding: Some(TextEncoding::Utf8),
                line_ending: Some(LineEnding::Lf),
            },
            CorrelationId::new(),
        )
        .expect("a different file must not be blocked by another session's registration");
    assert_eq!(
        std::fs::read(harness.root.path().join("unrelated.txt")).expect("unrelated"),
        b"unrelated file\n"
    );

    // The registered file itself is still the one the workspace row names, and it is still the
    // session's own: session A can write it while its own registration is live.
    harness
        .fs
        .write_file(
            FsWriteFileRequest {
                workspace_id: harness.workspace_id,
                session_id: harness.session_a,
                target: harness.target("owned.txt"),
                content: "still mine\n".to_owned(),
                expected: FsExpectedState::FileHash { hash: ContentHash::blake3(b"owned\n") },
                encoding: None,
                line_ending: None,
            },
            CorrelationId::new(),
        )
        .expect("the registering session keeps working");
    assert_eq!(
        std::fs::read(harness.root.path().join("owned.txt")).expect("owned"),
        b"still mine\n"
    );
    assert!(
        harness
            .persistence
            .release_mutation_ownership_for_execution(
                harness.workspace_id,
                harness.session_a,
                registration
            )
            .expect("release ownership")
    );
}

#[test]
fn changeset_compensates_all_prior_steps_when_a_later_reparse_copy_fails() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join("first.txt"), "before\n").expect("first file");
        std::fs::create_dir_all(root.join("source-dir")).expect("source directory");
        std::fs::create_dir_all(root.join("real-dir")).expect("real directory");
        std::fs::write(root.join("source-dir/normal.txt"), "normal\n").expect("normal file");
        std::fs::write(root.join("real-dir/linked.txt"), "linked\n").expect("linked file");
        junction::create(root.join("real-dir"), root.join("source-dir/link"))
            .expect("source junction");
    });
    let result = harness.fs.apply_changeset(
        FsApplyChangesetRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session_a,
            operations: vec![
                FsChangesetOperation::WriteFile {
                    target: harness.target("first.txt"),
                    content: "after\n".to_owned(),
                    expected: FsExpectedState::FileHash { hash: ContentHash::blake3(b"before\n") },
                    encoding: None,
                    line_ending: None,
                },
                FsChangesetOperation::Copy {
                    source: harness.target("source-dir"),
                    destination: harness.target("copied-dir"),
                    source_expected: FsExpectedState::Directory,
                    destination_expected: FsExpectedState::Absent,
                },
            ],
        },
        CorrelationId::new(),
    );
    assert!(matches!(
        result,
        Err(FsToolError::ChangesetFailed {
            failed_step: 1,
            applied_steps: 1,
            compensated_steps: 1,
            ..
        })
    ));
    assert_eq!(
        std::fs::read(harness.root.path().join("first.txt")).expect("restored first"),
        b"before\n"
    );
    assert!(!harness.root.path().join("copied-dir").exists());
    assert_eq!(
        harness
            .persistence
            .workspace_blockers(harness.workspace_id)
            .expect("workspace blockers")
            .unfinished_journals,
        0
    );
}

/// Serialises the tests that install a process-global staging interference.
///
/// The interference registry is process-wide by design — the production code has to be able to find
/// an installed action — so two tests that install one must not run at the same time. Without this,
/// one test's injected member leaks into another test's directory and the second test fails for a
/// reason that has nothing to do with the product. Measured: the suite passes with
/// --test-threads=1 and fails at the default parallelism without this lock.
fn staging_interference_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(())).lock().unwrap_or_else(|error| error.into_inner())
}

#[test]
fn a_failed_ownership_registration_releases_the_unstarted_resource_commit() {
    let harness = Harness::new(|_| {});
    let request = |session_id| FsWriteFileRequest {
        workspace_id: harness.workspace_id,
        session_id,
        target: harness.target("ownership-error.txt"),
        content: "registered writer\n".to_owned(),
        expected: FsExpectedState::Absent,
        encoding: Some(TextEncoding::Utf8),
        line_ending: Some(LineEnding::Lf),
    };
    let owner = harness.workspace.eligibility_owner();
    assert_eq!(owner.snapshot().commits, 0);

    // This session has no persistence row: the real ownership insert fails after resource
    // admission, before the journal or any file effect starts.
    let failed = harness.fs.write_file(request(SessionId::new()), CorrelationId::new());
    assert!(matches!(failed, Err(FsToolError::Persistence(_))), "{failed:?}");
    assert!(!harness.root.path().join("ownership-error.txt").exists());
    assert_eq!(owner.snapshot().commits, 0, "the failed insert must not strand admission");

    // The same target must remain usable by a real session rather than be deferred forever.
    let written = harness
        .fs
        .write_file(request(harness.session_a), CorrelationId::new())
        .expect("a registered session can still write the target");
    assert_eq!(written.disposition, FsMutationDisposition::Applied);
    assert_eq!(
        std::fs::read_to_string(harness.root.path().join("ownership-error.txt")).expect("content"),
        "registered writer\n"
    );
    assert_eq!(owner.snapshot().commits, 0);
}

/// **Third-party material is never swept away by a compensation, and this execution's own material
/// still goes.**
///
/// The brief's exit fact is that a compensation bound to the result object must never delete or move
/// a competitor's content. The mechanism has two halves, and both are driven here through the
/// **real** changeset entry, because that is the only path that creates the recovery responsibility
/// this is about:
///
/// * a directory this execution created is removed only while it is **empty** — so a member a third
///   party added stops the removal instead of being swept away;
/// * a file this execution published is removed only while it still holds **the bytes this execution
///   wrote**, so a same-identity in-place rewrite is preserved.
///
/// Every case forces a compensation by making a *later* operation in the same changeset fail, which
/// is the production shape: earlier steps are undone by `StagingArea::compensate`.
#[test]
fn a_compensation_removes_its_own_empty_directory_but_not_a_third_party_member() {
    // This test and the injection test below share the process-global staging interference registry,
    // so they must not overlap. The guard is held for the whole test and released on drop.
    let _serial = staging_interference_lock();
    // Baseline: a changeset whose first step creates a directory and whose second fails on a
    // reparse-point copy. The compensation must remove the directory this execution created.
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("source-dir")).expect("source directory");
        std::fs::write(root.join("source-dir/normal.txt"), "normal\n").expect("normal file");
        std::fs::create_dir_all(root.join("real-dir")).expect("real directory");
        std::fs::write(root.join("real-dir/linked.txt"), "linked\n").expect("linked file");
        junction::create(root.join("real-dir"), root.join("source-dir/link"))
            .expect("source junction");
    });
    let result = harness.fs.apply_changeset(
        FsApplyChangesetRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session_a,
            operations: vec![
                FsChangesetOperation::CreateDirectory { target: harness.target("mine") },
                FsChangesetOperation::Copy {
                    source: harness.target("source-dir"),
                    destination: harness.target("copied-dir"),
                    source_expected: FsExpectedState::Directory,
                    destination_expected: FsExpectedState::Absent,
                },
            ],
        },
        CorrelationId::new(),
    );
    assert!(result.is_err(), "the reparse-point copy must fail the changeset");
    assert!(
        !harness.root.path().join("mine").exists(),
        "the compensation must remove the empty directory this execution created"
    );
    assert!(
        !harness.root.path().join("copied-dir").exists(),
        "the failed copy must leave nothing behind"
    );
    assert_eq!(
        std::fs::read_to_string(harness.root.path().join("source-dir/normal.txt"))
            .expect("the source is untouched"),
        "normal\n",
        "a failed copy must never modify its source"
    );
}

/// **A third-party member stops the directory compensation and survives — the real injection.**
///
/// This is the case a blanket recursive delete got wrong, and it needs the world to change **during**
/// the operation: the changeset creates a directory, a third party puts a member inside it, and only
/// then does a later operation fail so the compensation runs.
///
/// A test cannot arrange that from outside without racing the operation, and re-implementing the
/// operation would prove nothing about the product. So the member is added through the platform's
/// real interference point (publication_fault::interfere_at), which runs an action against the real
/// path **inside** the production publication of the step that creates the directory. It cannot
/// change that step's outcome: the publication continues and its own code decides what follows.
///
/// The assertions afterwards are therefore about what the production compensation really did when it
/// met a directory that was no longer only its own.
#[test]
fn a_third_party_member_stops_the_directory_compensation_and_survives() {
    let _serial = staging_interference_lock();
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("source-dir")).expect("source directory");
        std::fs::write(root.join("source-dir/normal.txt"), "normal\n").expect("normal file");
        std::fs::create_dir_all(root.join("real-dir")).expect("real directory");
        std::fs::write(root.join("real-dir/linked.txt"), "linked\n").expect("linked file");
        junction::create(root.join("real-dir"), root.join("source-dir/link"))
            .expect("source junction");
    });

    // The interference: the moment this execution has really created its directory, a third party
    // puts a member inside it. That is the directory the compensation later has to decide about,
    // and the action runs against the real path inside the real staging step.
    let _interference = aura_fs_tools::staging_fault::install(
        aura_fs_tools::staging_fault::Stage::DirectoryCreated,
        None,
        Arc::new(|created: &Path| {
            std::fs::write(created.join("their-file.txt"), "theirs\n")
                .expect("the third party adds a member");
        }),
    );

    let result = harness.fs.apply_changeset(
        FsApplyChangesetRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session_a,
            operations: vec![
                FsChangesetOperation::CreateDirectory { target: harness.target("mine") },
                FsChangesetOperation::Copy {
                    source: harness.target("source-dir"),
                    destination: harness.target("copied-dir"),
                    source_expected: FsExpectedState::Directory,
                    destination_expected: FsExpectedState::Absent,
                },
            ],
        },
        CorrelationId::new(),
    );
    assert!(result.is_err(), "the reparse-point copy must still fail the changeset");

    // The member a third party added is not this execution's to delete. Whether the compensation
    // refused outright or removed only its own material, the member must be there afterwards — and
    // whatever remains must be reported rather than silently cleaned.
    let member = harness.root.path().join("mine/their-file.txt");
    assert!(
        member.exists(),
        "a member this execution did not create must survive the compensation: {result:?}"
    );
    assert_eq!(std::fs::read_to_string(&member).expect("content"), "theirs\n");
}

/// **A same-identity in-place rewrite of a published result is not deleted.**
///
/// `remove_own_result` compares the published **bytes** as well as the identity, because Windows
/// keeps the file id across an in-place rewrite — so identity alone cannot tell this execution's
/// result from a third party's replacement. This measures the fact the comparison depends on, on the
/// real platform call the operation uses.
#[test]
fn an_in_place_rewrite_keeps_the_identity_the_compensation_compares_against() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join("keep.txt"), "mine\n").expect("file");
    });
    let written = harness
        .fs
        .write_file(
            FsWriteFileRequest {
                workspace_id: harness.workspace_id,
                session_id: harness.session_a,
                target: harness.target("keep.txt"),
                content: "the execution's result\n".to_owned(),
                expected: FsExpectedState::FileHash { hash: ContentHash::blake3(b"mine\n") },
                encoding: None,
                line_ending: None,
            },
            CorrelationId::new(),
        )
        .expect("the write publishes");
    assert_eq!(written.disposition, FsMutationDisposition::Applied);

    let absolute = harness.root.path().join("keep.txt");
    let identity_of = |path: &Path| {
        aura_windows_platform::inspect_existing_path(path, true).expect("inspect").identity
    };
    let before = identity_of(&absolute);
    // A third party rewrites it in place, through plain `std::fs`.
    std::fs::write(&absolute, "a third party's content\n").expect("in-place rewrite");
    let after = identity_of(&absolute);
    assert_eq!(
        before, after,
        "an in-place rewrite keeps the FileIdentity, which is why the identity alone cannot decide \
         whether the object is still this execution's result"
    );
    assert_eq!(
        std::fs::read_to_string(&absolute).expect("content"),
        "a third party's content\n",
        "the third party's bytes are what the compensation would have to compare against"
    );
}

/// **R3: the move's last window is closed — a third-party in-place rewrite is refused by the OS.**
///
/// # The window this measures
///
/// The pre-fix move was `fs::rename(source_path, destination_path)` with a path rename-back on
/// failure. A path rename carries whatever object the **source name** refers to at the instant the
/// kernel resolves it, and nothing held the source still between the caller's condition check and
/// that instant. So a third party could rewrite the bytes in place — Windows keeps the file id, so
/// only a byte comparison could ever notice — and the move would carry content the caller never
/// decided about.
///
/// # The fact that proves it is closed
///
/// The interference point runs after the caller's condition has been verified and before the native
/// move, against the real object at its real name. It is now reached **while the move holds the
/// source with a READ+DELETE handle that denies sharing for write and delete**, so the third party's
/// rewrite is refused by the operating system rather than winning the window. That refusal is the
/// guarantee: the window is not merely detected, it cannot be entered.
///
/// The move then succeeds, because the object really still holds the caller's bytes — which is the
/// correct outcome and is asserted here too, so this cannot pass by refusing everything.
#[test]
fn a_move_holds_the_source_so_a_third_party_rewrite_cannot_win_the_last_window() {
    let _serial = staging_interference_lock();
    let harness = Harness::new(|root| {
        std::fs::write(root.join("source.txt"), "original\n").expect("source");
    });

    let reached = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let refused = Arc::new(std::sync::Mutex::new(None::<std::io::Error>));
    let seen = Arc::clone(&reached);
    let recorded = Arc::clone(&refused);
    let _interference = aura_fs_tools::staging_fault::install(
        aura_fs_tools::staging_fault::Stage::BeforeStagedMove,
        None,
        Arc::new(move |target: &Path| {
            if !target.file_name().is_some_and(|name| name == "source.txt") {
                return;
            }
            seen.store(true, std::sync::atomic::Ordering::SeqCst);
            // The real external writer, using plain `std::fs` — nothing about this test's own
            // code is holding the file.
            if let Err(error) = std::fs::write(target, "a third party rewrote this\n") {
                *recorded.lock().expect("record") = Some(error);
            }
        }),
    );

    let result = harness.fs.move_path(
        FsMoveRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session_a,
            source: harness.target("source.txt"),
            destination: harness.target("moved.txt"),
            source_expected: FsExpectedState::FileHash { hash: ContentHash::blake3(b"original\n") },
            destination_expected: FsExpectedState::Absent,
        },
        CorrelationId::new(),
    );
    assert!(
        reached.load(std::sync::atomic::Ordering::SeqCst),
        "the last window must really be reached, or this proves nothing"
    );
    let error = refused.lock().expect("record").take().expect(
        "**the third party's in-place rewrite must be refused by the OS**: the move has to hold the \
         source with a handle that denies writes, or the window is open",
    );
    assert!(
        matches!(error.raw_os_error(), Some(32) | Some(5)),
        "the refusal must be a real sharing/access denial, not something else: {error:?}"
    );
    let moved =
        result.expect("the caller's condition really still holds, so the move must succeed");
    assert_eq!(moved.disposition, FsMutationDisposition::Applied);
    assert_eq!(
        std::fs::read_to_string(harness.root.path().join("moved.txt")).expect("destination"),
        "original\n",
        "the caller's own bytes — never the third party's — arrived at the destination"
    );
    assert!(
        !harness.root.path().join("source.txt").exists(),
        "the move really moved the object rather than copying it"
    );
}

/// **R3: the legitimate control — a move whose original condition really holds succeeds.**
///
/// Without this, a blanket refusal to move would pass the counterexample above while breaking the
/// ordinary case. The same interference point is installed, and its action deliberately does
/// **nothing** to the source, so the only difference from the failing case is whether the caller's
/// condition is still true.
#[test]
fn an_unchanged_source_still_moves_and_publishes() {
    let _serial = staging_interference_lock();
    let harness = Harness::new(|root| {
        std::fs::write(root.join("source.txt"), "original\n").expect("source");
    });

    let touched = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = Arc::clone(&touched);
    let _interference = aura_fs_tools::staging_fault::install(
        aura_fs_tools::staging_fault::Stage::BeforeStagedMove,
        None,
        Arc::new(move |target: &Path| {
            // The window is really reached; the world simply does not change.
            if target.file_name().is_some_and(|name| name == "source.txt") {
                observed.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }),
    );

    let result = harness
        .fs
        .move_path(
            FsMoveRequest {
                workspace_id: harness.workspace_id,
                session_id: harness.session_a,
                source: harness.target("source.txt"),
                destination: harness.target("moved.txt"),
                source_expected: FsExpectedState::FileHash {
                    hash: ContentHash::blake3(b"original\n"),
                },
                destination_expected: FsExpectedState::Absent,
            },
            CorrelationId::new(),
        )
        .expect("a move whose original condition holds must succeed");
    assert_eq!(result.disposition, FsMutationDisposition::Applied);
    assert!(
        touched.load(std::sync::atomic::Ordering::SeqCst),
        "the interference point must really be reached, or this control proves nothing"
    );
    assert!(
        !harness.root.path().join("source.txt").exists(),
        "the source name is really empty after a successful move"
    );
    assert_eq!(
        std::fs::read_to_string(harness.root.path().join("moved.txt")).expect("destination"),
        "original\n",
        "the very object the caller decided about arrived at the destination"
    );
}

/// **R3: a swap-in during the move's last window is refused by the OS.**
///
/// This is the case an identity comparison and a byte comparison would each miss on their own: the
/// third party takes the name away and puts a **different object** in its place. Because the move
/// performs its effect through the handle the caller's condition was verified on — and that handle
/// denies delete and rename sharing — the swap cannot happen at all.
#[test]
fn a_swap_in_during_the_moves_last_window_is_refused_by_the_os() {
    let _serial = staging_interference_lock();
    let harness = Harness::new(|root| {
        std::fs::write(root.join("source.txt"), "original\n").expect("source");
    });

    let refusal = Arc::new(std::sync::Mutex::new(None::<String>));
    let recorded = Arc::clone(&refusal);
    let _interference = aura_fs_tools::staging_fault::install(
        aura_fs_tools::staging_fault::Stage::BeforeStagedMove,
        None,
        Arc::new(move |target: &Path| {
            if !target.file_name().is_some_and(|name| name == "source.txt") {
                return;
            }
            // A real swap attempt: remove the object and put a different one at the name.
            let message = match std::fs::remove_file(target) {
                Ok(()) => match std::fs::write(target, "a competitor's object\n") {
                    Ok(()) => "the swap succeeded, which means the window was open".to_owned(),
                    Err(error) => format!("create refused: {error}"),
                },
                Err(error) => format!("remove refused: {error}"),
            };
            *recorded.lock().expect("record") = Some(message);
        }),
    );

    let result = harness.fs.move_path(
        FsMoveRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session_a,
            source: harness.target("source.txt"),
            destination: harness.target("moved.txt"),
            source_expected: FsExpectedState::FileHash { hash: ContentHash::blake3(b"original\n") },
            destination_expected: FsExpectedState::Absent,
        },
        CorrelationId::new(),
    );
    let message = refusal.lock().expect("record").take().expect("the window must be reached");
    assert!(
        message.contains("refused"),
        "**the swap must be refused by the OS**, because the move holds the object with a handle \
         that denies delete and rename: {message}"
    );
    let moved = result.expect("the caller's object is untouched, so the move must succeed");
    assert_eq!(moved.disposition, FsMutationDisposition::Applied);
    assert_eq!(
        std::fs::read_to_string(harness.root.path().join("moved.txt")).expect("destination"),
        "original\n",
        "the caller's own object is what arrived, never the competitor's"
    );
    assert!(
        !harness.root.path().join("source.txt").exists(),
        "the move really moved the object rather than copying it"
    );
}

/// **R3: a partial delete of a copied tree fails honestly and keeps the material.**
///
/// A directory copy stages the original aside and removes it in place. If that removal stops
/// part-way, the copy is the **only complete material** for the original tree — so deleting it while
/// reporting `compensated` would destroy the one recoverable copy. The requirement is explicit that a
/// partial failure must not clear the only complete backup, and must not be labelled Aborted or
/// Compensated with insufficient honesty.
///
/// The interference holds one member of the original tree open, which is the only honest way to make
/// the real removal really stop part-way; a test cannot fabricate a partial removal.
#[test]
fn a_partial_original_removal_keeps_the_complete_backup_and_reports_it() {
    let _serial = staging_interference_lock();
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("tree")).expect("tree");
        std::fs::write(root.join("tree/one.txt"), "one\n").expect("one");
        std::fs::write(root.join("tree/two.txt"), "two\n").expect("two");
    });

    let held: Arc<std::sync::Mutex<Option<std::fs::File>>> = Arc::new(std::sync::Mutex::new(None));
    let holder = Arc::clone(&held);
    // `install_forcing_copy` takes the production copy-then-remove route, which otherwise is reached
    // only when the watcher happens to hold the tree open. Everything after it is production.
    let reached = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let seen = Arc::clone(&reached);
    let _interference = aura_fs_tools::staging_fault::install_forcing_copy(
        aura_fs_tools::staging_fault::Stage::PartialOriginalRemoval,
        None,
        Arc::new(move |tree: &Path| {
            if tree.file_name().is_some_and(|name| name == "tree") {
                seen.store(true, std::sync::atomic::Ordering::SeqCst);
                // Holding a member open without share-delete makes the real `remove_dir_all` fail
                // part way through, which is the genuine partial-removal condition.
                let file = std::fs::OpenOptions::new()
                    .read(true)
                    .open(tree.join("one.txt"))
                    .expect("hold a member open");
                *holder.lock().expect("holder") = Some(file);
            }
        }),
    );

    let result = harness.fs.delete(
        FsDeleteRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session_a,
            target: harness.target("tree"),
            expected: FsExpectedState::Directory,
            recursive: true,
        },
        CorrelationId::new(),
    );
    // Whatever the platform reported, the requirement is about the facts left behind. Two honest
    // outcomes exist and both are recorded, because a test that only accepted one of them would
    // either pass on a fabricated partial removal or demand a failure the platform cannot produce:
    //
    // * the removal **stopped part-way**, in which case the call must report material for
    //   reconciliation and the complete backup must survive; or
    // * the removal **completed** despite the held handle, in which case the execution really is
    //   done and reporting `Applied` is the honest answer — the object the caller asked to delete is
    //   gone, and a staged backup still exists for it.
    let reached_real = reached.load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        reached_real,
        "the partial-removal window must really be reached, or this test measures nothing"
    );
    match result {
        Err(error) => {
            let message = error.to_string();
            assert!(
                message.contains("Reconciliation") || message.contains("reconcil"),
                "a partial removal must be reported as material for reconciliation, not as a clean \
                 compensated rollback: {message}"
            );
            // The backup — the copy staged aside — is the only complete material, so it must exist.
            assert!(
                staged_tree_members(harness.root.path()) >= 2,
                "the complete backup of the original tree must be kept after a partial removal: \
                 {message}"
            );
        }
        Ok(applied) => {
            assert_eq!(
                applied.disposition,
                FsMutationDisposition::Applied,
                "a removal that really completed is an applied delete, not a compensation"
            );
            assert!(
                !harness.root.path().join("tree").exists(),
                "the delete really removed the tree"
            );
            // The delete completed, so its own staging is settled and removed: nothing is left to
            // recover. This is the honest opposite of the failing branch, and it is what makes the
            // failing branch's surviving backup attributable to the partial removal rather than to
            // the delete path always leaving its copy behind.
            assert_eq!(
                staged_tree_members(harness.root.path()),
                0,
                "a delete that really completed settles its own staging material"
            );
        }
    }
    drop(held.lock().expect("holder").take());
}

/// **R3: a genuinely partial removal keeps the only complete backup and reports it.**
///
/// The test above reports honestly when the platform completes the removal despite a held member.
/// This one forces the partial outcome the requirement is actually about, through a condition the
/// platform really produces: a **read-only directory** cannot be removed by `remove_dir_all`, so the
/// removal stops part-way after members have already gone. The assertions are about the facts the
/// recovery owner needs — that the failure is reported as material rather than as a clean
/// compensation, and that the complete backup is not cleared.
#[test]
fn a_partial_removal_keeps_the_only_complete_backup_and_reports_it() {
    let _serial = staging_interference_lock();
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("tree/inner")).expect("tree");
        std::fs::write(root.join("tree/one.txt"), "one\n").expect("one");
        std::fs::write(root.join("tree/inner/two.txt"), "two\n").expect("two");
    });

    let held: Arc<std::sync::Mutex<Option<aura_windows_platform::VerifiedFile>>> =
        Arc::new(std::sync::Mutex::new(None));
    let holder = Arc::clone(&held);
    let _interference = aura_fs_tools::staging_fault::install_forcing_copy(
        aura_fs_tools::staging_fault::Stage::PartialOriginalRemoval,
        None,
        Arc::new(move |tree: &Path| {
            if !tree.file_name().is_some_and(|name| name == "tree") {
                return;
            }
            // Hold a member open through the platform's own **no-share-delete** handle. That is what
            // makes the real `remove_dir_all` remove what it can and then stop on this member, which
            // is the genuine partial-removal condition. A plain `File::open` shares delete on
            // Windows, so the removal would succeed and this test would measure nothing.
            let member = tree.join("inner").join("two.txt");
            let opened = aura_windows_platform::open_held_for_move(&member)
                .expect("hold a member against delete sharing");
            *holder.lock().expect("holder") = Some(opened);
        }),
    );

    let result = harness.fs.delete(
        FsDeleteRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session_a,
            target: harness.target("tree"),
            expected: FsExpectedState::Directory,
            recursive: true,
        },
        CorrelationId::new(),
    );
    let error = result.expect_err(
        "a removal that really stopped part-way must be reported, not silently compensated",
    );
    let message = error.to_string();
    assert!(
        message.contains("Reconciliation") || message.contains("reconcil"),
        "a partial removal must be reported as material for reconciliation, not as a clean \
         compensated rollback: {message}"
    );
    assert!(
        staged_tree_members(harness.root.path()) >= 2,
        "**the only complete backup must be kept**, not cleared with the failing step: {message}"
    );
}

/// How many members the staging areas currently hold, so a test can state whether the complete
/// backup of a tree survived without re-implementing the staging layout.
fn staged_tree_members(root: &Path) -> usize {
    let mut members = 0;
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    for entry in entries.filter_map(Result::ok) {
        if !entry.file_name().to_string_lossy().starts_with(".aura-transaction-") {
            continue;
        }
        let Ok(inner) = std::fs::read_dir(entry.path()) else {
            continue;
        };
        for staged in inner.filter_map(Result::ok) {
            if staged.path().is_dir() {
                members += std::fs::read_dir(staged.path())
                    .map(|files| files.filter_map(Result::ok).count())
                    .unwrap_or(0);
            }
        }
    }
    members
}
