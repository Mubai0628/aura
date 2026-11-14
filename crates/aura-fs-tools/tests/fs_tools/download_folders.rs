//! A download into folders of the workspace that do not exist yet gets them, created by handle
//! below the folder that existed when the call was checked; a link on the way is not entered,
//! and a download that does not finish takes back the empty folders it made.
use aura_domain_common::{CorrelationId, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath};
use aura_fs_tools::{FsToolPolicy, FsToolService, download::inspect_download_target};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::{SessionSummary, WorkspaceCreateCommand};
use std::{path::Path, sync::Arc};

struct Fixture {
    service: Arc<FsToolService>,
    workspace: WorkspaceId,
    session: SessionId,
    root: tempfile::TempDir,
    _data: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("workspace");
        let data = tempfile::tempdir().expect("stores");
        let PersistenceStartup::Ready(persistence) =
            PersistenceCoordinator::start(PersistenceOptions::new(
                data.path().join("aura.sqlite3"),
                data.path().join("artifacts"),
                RuntimeInstanceId::new(),
            ))
            .expect("store")
        else {
            panic!("a new store needs no recovery")
        };
        let persistence = Arc::new(persistence);
        let runtime =
            WorkspaceRuntime::open(Arc::clone(&persistence), WorkspaceRuntimeOptions::default())
                .expect("workspace runtime");
        let created = runtime
            .create(
                WorkspaceCreateCommand {
                    selected_root: root.path().display().to_string(),
                    display_name: "download folders".into(),
                },
                CorrelationId::new(),
            )
            .expect("workspace");
        runtime.wait_until_indexed(std::time::Duration::from_secs(60)).expect("the first walk");
        let workspace = created.workspace.workspace_id;
        let session = SessionId::new();
        persistence
            .upsert_workspace_session(
                persistence.workspace(workspace).expect("workspace record"),
                SessionSummary {
                    session_id: session,
                    workspace_id: workspace,
                    title: "download".into(),
                    state: "idle".into(),
                },
                CorrelationId::new(),
            )
            .expect("session");
        let service = FsToolService::new(runtime, persistence, FsToolPolicy::default())
            .expect("file service");
        Self { service, workspace, session, root, _data: data }
    }

    fn target(&self, relative: &str) -> aura_fs_tools::download::DownloadTarget {
        inspect_download_target(
            &self.root.path().join(relative),
            Some(WorkspacePath::parse(relative).expect("relative path")),
            false,
        )
        .expect("checked target")
    }

    fn download(
        &self,
        target: aura_fs_tools::download::DownloadTarget,
        bytes: &[u8],
    ) -> Result<aura_fs_tools::download::DownloadPublication, aura_fs_tools::FsToolError> {
        let mut stage = self.service.begin_download(target, self.workspace, self.session)?;
        stage.append(bytes)?;
        self.service.finish_download(stage, self.workspace, CorrelationId::new())
    }
}

fn plain_folder(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
}

#[test]
fn a_download_into_folders_that_do_not_exist_yet_creates_them_in_the_workspace() {
    let f = Fixture::new();
    let target = f.target(".cache/s00/pkg.zip");
    assert_eq!(target.missing_folders, vec![".cache".to_owned(), "s00".to_owned()]);
    assert!(!f.root.path().join(".cache").exists(), "checking creates nothing");
    f.service
        .preflight_download(&target, f.workspace, f.session)
        .expect("the eligibility check admits the pending folders");
    let published = f.download(target, b"package bytes").expect("published");
    assert_eq!(published.bytes_written, 13);
    assert!(plain_folder(&f.root.path().join(".cache")));
    assert!(plain_folder(&f.root.path().join(".cache/s00")));
    assert_eq!(
        std::fs::read(f.root.path().join(".cache/s00/pkg.zip")).expect("the file"),
        b"package bytes"
    );
    // A file whose folder exists records none to create, as before.
    assert!(f.target(".cache/s00/other.zip").missing_folders.is_empty());
}

#[test]
fn a_download_that_does_not_finish_takes_back_the_folders_it_made() {
    let f = Fixture::new();
    std::fs::create_dir(f.root.path().join("kept")).expect("an existing folder");
    let target = f.target("kept/new/deeper/pkg.zip");
    assert_eq!(target.missing_folders, vec!["new".to_owned(), "deeper".to_owned()]);
    let mut stage = f.service.begin_download(target, f.workspace, f.session).expect("staged");
    assert!(plain_folder(&f.root.path().join("kept/new/deeper")), "made for the staging file");
    stage.append(b"partial").expect("partial");
    drop(stage);
    assert!(!f.root.path().join("kept/new").exists(), "the empty folders are taken back");
    assert!(plain_folder(&f.root.path().join("kept")), "a folder that was there stays");
}

#[test]
fn a_folder_another_call_created_since_the_check_is_used() {
    let f = Fixture::new();
    let first = f.target("shared/one.bin");
    let second = f.target("shared/two.bin");
    f.download(first, b"one").expect("the first creates the folder");
    f.download(second, b"two").expect("the second finds it made");
    assert_eq!(std::fs::read(f.root.path().join("shared/two.bin")).expect("two"), b"two");
    // A file put where a folder was to be made is not a folder to enter.
    let blocked = f.target("later/three.bin");
    std::fs::write(f.root.path().join("later"), b"a file").expect("a file in the way");
    assert!(f.download(blocked, b"three").is_err());
    assert_eq!(std::fs::read(f.root.path().join("later")).expect("untouched"), b"a file");
}

#[test]
fn a_link_on_the_way_is_not_entered_and_nothing_is_made_outside() {
    let f = Fixture::new();
    let elsewhere = tempfile::tempdir().expect("outside the workspace");
    junction::create(elsewhere.path(), f.root.path().join("linked")).expect("a junction");
    let refused = inspect_download_target(
        &f.root.path().join("linked/new/pkg.zip"),
        Some(WorkspacePath::parse("linked/new/pkg.zip").expect("relative path")),
        false,
    );
    assert!(refused.is_err(), "a link on the way to a new folder is refused");
    assert!(!elsewhere.path().join("new").exists(), "nothing is created through the link");
}

#[test]
fn a_missing_folder_outside_the_workspace_is_named_with_what_to_do() {
    let outside = tempfile::tempdir().expect("outside");
    let error = inspect_download_target(&outside.path().join("missing/pkg.zip"), None, false)
        .expect_err("not created outside the workspace");
    let text = error.to_string();
    assert!(text.contains("http.download_folder_missing"), "{text}");
    assert!(text.contains("missing"), "the folder is named: {text}");
    assert!(!outside.path().join("missing").exists());
}
