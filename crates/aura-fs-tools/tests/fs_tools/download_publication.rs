//! Real coordinator and native publication, with no response-sized client allocation.
use aura_domain_common::{CorrelationId, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath};
use aura_fs_tools::{FsToolPolicy, FsToolService, download::inspect_download_target};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::{SessionSummary, WorkspaceCreateCommand};
use std::{io::Read, sync::Arc};

struct Fixture {
    service: Arc<FsToolService>,
    workspace: WorkspaceId,
    session: SessionId,
    root: tempfile::TempDir,
    _data: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("isolated download workspace");
        let data = tempfile::tempdir().expect("isolated download stores");
        let PersistenceStartup::Ready(persistence) =
            PersistenceCoordinator::start(PersistenceOptions::new(
                data.path().join("aura.sqlite3"),
                data.path().join("artifacts"),
                RuntimeInstanceId::new(),
            ))
            .expect("open fixture store")
        else {
            panic!("new store needs no recovery")
        };
        let persistence = Arc::new(persistence);
        let runtime =
            WorkspaceRuntime::open(Arc::clone(&persistence), WorkspaceRuntimeOptions::default())
                .expect("workspace runtime");
        let created = runtime
            .create(
                WorkspaceCreateCommand {
                    selected_root: root.path().display().to_string(),
                    display_name: "download fixture".into(),
                },
                CorrelationId::new(),
            )
            .expect("workspace registration");
        runtime.wait_until_indexed(std::time::Duration::from_secs(60)).expect("the first walk");
        let workspace = created.workspace.workspace_id;
        let session = SessionId::new();
        persistence
            .upsert_workspace_session(
                persistence.workspace(workspace).expect("workspace record"),
                SessionSummary {
                    session_id: session,
                    workspace_id: workspace,
                    title: "download session".into(),
                    state: "idle".into(),
                },
                CorrelationId::new(),
            )
            .expect("session registration");
        let service = FsToolService::new(runtime, persistence, FsToolPolicy::default())
            .expect("file service");
        Self { service, workspace, session, root, _data: data }
    }
    fn stage(&self) -> aura_fs_tools::download::DownloadStaging {
        let target = inspect_download_target(
            &self.root.path().join("target.bin"),
            Some(WorkspacePath::parse("target.bin").expect("relative path")),
            true,
        )
        .expect("frozen target");
        self.service
            .begin_download(target, self.workspace, self.session)
            .expect("coordinator staging admission")
    }
    fn finish(
        &self,
        stage: aura_fs_tools::download::DownloadStaging,
    ) -> Result<aura_fs_tools::download::DownloadPublication, aura_fs_tools::FsToolError> {
        self.service.finish_download(stage, self.workspace, CorrelationId::new())
    }
    fn no_staging(&self) {
        assert!(
            std::fs::read_dir(self.root.path()).expect("workspace enumeration").all(|entry| !entry
                .expect("directory item")
                .file_name()
                .to_string_lossy()
                .starts_with(".aura-download-")),
            "temporary objects must be removed after release"
        );
    }
}

#[test]
fn s07_n8_two_hundred_mib_is_staged_in_bounded_chunks_and_published_with_identity_receipt() {
    let f = Fixture::new();
    let path = f.root.path().join("target.bin");
    std::fs::write(&path, b"previous").expect("old content");
    let mut stage = f.stage();
    let block = [b'Z'; 64 * 1024];
    let mut expected = blake3::Hasher::new();
    for _ in 0..3200 {
        stage.append(&block).expect("bounded staging write");
        expected.update(&block);
    }
    assert_eq!(std::fs::read(&path).expect("old target before publish"), b"previous");
    let result = f.finish(stage).expect("atomic publication");
    assert_eq!(result.bytes_written, 200 * 1024 * 1024);
    assert_eq!(result.max_chunk_bytes, 64 * 1024, "actual write buffer high water stays fixed");
    assert_eq!(result.hash.hex, expected.finalize().to_hex().to_string());
    assert_eq!(result.receipts.len(), 1, "one actual identity replacement receipt");
    let mut read = std::fs::File::open(path).expect("published content");
    let mut actual = blake3::Hasher::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = read.read(&mut buffer).expect("bounded verification");
        if n == 0 {
            break;
        }
        actual.update(&buffer[..n]);
    }
    assert_eq!(actual.finalize(), expected.finalize());
    f.no_staging();
}

#[test]
fn s07_n9_cancelled_staging_preserves_the_old_file_and_releases_temporary_objects() {
    let f = Fixture::new();
    let path = f.root.path().join("target.bin");
    std::fs::write(&path, b"previous").expect("old content");
    let mut stage = f.stage();
    stage.append(b"incomplete response").expect("partial response");
    drop(stage);
    assert_eq!(std::fs::read(&path).expect("target after cancellation"), b"previous");
    f.no_staging();
    let _next = f.stage();
}

#[test]
fn s07_n7_replaced_or_edited_target_is_never_overwritten_by_a_pending_download() {
    for replace in [false, true] {
        let f = Fixture::new();
        let path = f.root.path().join("target.bin");
        std::fs::write(&path, b"previous").expect("old content");
        let mut stage = f.stage();
        stage.append(b"download").expect("staged response");
        if replace {
            std::fs::rename(&path, f.root.path().join("parked.bin"))
                .expect("external atomic editor replacement");
        }
        std::fs::write(&path, b"external change").expect("external update");
        assert!(f.finish(stage).is_err(), "frozen identity and content must both remain valid");
        assert_eq!(std::fs::read(&path).expect("external bytes remain"), b"external change");
        f.no_staging();
    }
}

#[test]
fn s07_n8_unbounded_chunks_are_refused_before_any_publication() {
    let f = Fixture::new();
    let mut stage = f.stage();
    assert!(stage.append(&[0u8; 64 * 1024 + 1]).is_err());
    drop(stage);
    assert!(!f.root.path().join("target.bin").exists());
    f.no_staging();
}
