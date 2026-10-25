//! Atomic publish and editor backup files are not user content, even while they exist.
use super::*;
use aura_domain_common::{CorrelationId, RuntimeInstanceId, WorkspacePath};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_windows_platform::{FileWatchEvent, FileWatchEventKind};
use aura_workspace_domain::WorkspaceCreateCommand;
use std::sync::Arc;

const TEMP: &str = ".aura-publish-e430509722f419135a28f603ca53c0c5.tmp";
const BUFFER_BACKUP: &str = ".aura-buffer-01ARZ3NDEKTSV4RRFFQ69G5FAV.bak";
const WRITE_BACKUP: &str = ".aura-write-01ARZ3NDEKTSV4RRFFQ69G5FAV.bak";
// Downloads stage beside the target, and an overwrite parks the old object until commit.
const DOWNLOAD_TEMP: &str = ".aura-download-8f14e45fceea167a5a36dedd4bea2543.tmp";
const DOWNLOAD_BACKUP: &str = ".aura-backup-c9f0f895fb98ab9159f51fd0297e236d.tmp";

struct Fixture {
    root: tempfile::TempDir,
    _data: tempfile::TempDir,
    runtime: Arc<crate::WorkspaceRuntime>,
    workspace: aura_domain_common::WorkspaceId,
}

impl Fixture {
    fn new(with_artifacts: bool) -> Self {
        let root = tempfile::tempdir().expect("create an isolated native publication root");
        for name in [
            "keep.txt",
            "user.tmp",
            "user.bak",
            ".aura-publish-user.tmp",
            ".aura-download-user.tmp",
        ] {
            std::fs::write(root.path().join(name), "user content")
                .expect("seed user files that must remain visible");
        }
        if with_artifacts {
            Self::artifacts_at(root.path());
        }
        let data = tempfile::tempdir().expect("create the catalog's private store");
        let PersistenceStartup::Ready(store) =
            PersistenceCoordinator::start(PersistenceOptions::new(
                data.path().join("store.db"),
                data.path().join("artifacts"),
                RuntimeInstanceId::new(),
            ))
            .expect("open the real catalog store")
        else {
            panic!("the fixture store must be ready")
        };
        let runtime = crate::WorkspaceRuntime::open(Arc::new(store), Default::default())
            .expect("open the real workspace owner");
        let created = runtime
            .create(
                WorkspaceCreateCommand {
                    selected_root: root.path().display().to_string(),
                    display_name: "publication artifacts".into(),
                },
                CorrelationId::new(),
            )
            .expect("scan the actual workspace files");
        runtime.stop_watchers().expect("drive the native event batch directly");
        Self { root, _data: data, runtime, workspace: created.workspace.workspace_id }
    }

    fn artifacts_at(root: &std::path::Path) {
        for name in [TEMP, BUFFER_BACKUP, WRITE_BACKUP, DOWNLOAD_TEMP, DOWNLOAD_BACKUP] {
            std::fs::write(root.join(name), "private publication bytes")
                .expect("create the exact native publisher and save backup names");
        }
    }

    fn assert_user_entries(&self) {
        let catalog = self.runtime.catalog(self.workspace).expect("the real catalog is available");
        let paths = catalog
            .entries()
            .expect("read actual catalog entries")
            .into_iter()
            .filter(|entry| !entry.path.is_root())
            .map(|entry| entry.path.to_string())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            paths,
            [
                "keep.txt",
                "user.tmp",
                "user.bak",
                ".aura-publish-user.tmp",
                ".aura-download-user.tmp"
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            "only the user's entries belong in the catalog"
        );
    }
}

#[test]
fn native_publication_temps_and_save_backups_never_enter_the_real_catalog() {
    Fixture::new(true).assert_user_entries();
}

#[test]
fn events_for_native_publication_artifacts_have_no_user_catalog_scope() {
    let f = Fixture::new(false);
    Fixture::artifacts_at(f.root.path());
    let resolver = f.runtime.resolver(f.workspace).expect("use the native-verified workspace root");
    let catalog = f.runtime.catalog(f.workspace).expect("read the actual catalog");
    let events = [TEMP, BUFFER_BACKUP, WRITE_BACKUP, DOWNLOAD_TEMP, DOWNLOAD_BACKUP]
        .into_iter()
        .map(|name| FileWatchEvent {
            kind: FileWatchEventKind::Create,
            paths: vec![f.root.path().join(name)],
            detail: None,
        })
        .collect::<Vec<_>>();
    let WatchDescription::Described(changes) =
        changes_for_watch_events(&resolver, &catalog, &events)
    else {
        panic!("Aura's own publication events must have a bounded description")
    };
    assert!(changes.is_empty(), "private staging is absent from the user's delta: {changes:?}");
    catalog
        .apply_changes(
            &resolver,
            CatalogUpdate::new(
                vec![(
                    WorkspacePath::parse(".").expect("the verified relative root"),
                    CatalogChange::DirectoryListed,
                )],
                "parent event",
            ),
            &catalog.options(),
            CorrelationId::new(),
        )
        .expect("a parent event lists only user entries");
    f.assert_user_entries();
}
