//! A folder `.gitignore` excludes is recorded as itself and never entered, read or watched; a
//! change to `.gitignore` redraws that boundary, and the folder's contents are listed from disk.
use super::*;
use aura_domain_common::{CorrelationId, RuntimeInstanceId, WorkspacePath};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_windows_platform::{FileWatchEvent, FileWatchEventKind};
use aura_workspace_domain::{CatalogEntryKind, WorkspaceCreateCommand};
use std::sync::Arc;

struct Fixture {
    _root: tempfile::TempDir,
    _data: tempfile::TempDir,
    runtime: Arc<crate::WorkspaceRuntime>,
    workspace_id: aura_domain_common::WorkspaceId,
}

fn path(value: &str) -> WorkspacePath {
    WorkspacePath::parse(value).expect("a workspace path")
}

fn fixture() -> Fixture {
    let root = tempfile::tempdir().expect("workspace folder");
    std::fs::write(root.path().join(".gitignore"), "target/\n").expect(".gitignore");
    std::fs::create_dir_all(root.path().join("target/sub")).expect("ignored folders");
    std::fs::write(root.path().join("target/a.txt"), "built").expect("ignored file");
    std::fs::write(root.path().join("target/sub/b.txt"), "built too").expect("nested file");
    std::fs::create_dir(root.path().join("src")).expect("source folder");
    std::fs::write(root.path().join("src/main.rs"), "fn main() {}").expect("source file");
    let data = tempfile::tempdir().expect("store folder");
    let PersistenceStartup::Ready(persistence) =
        PersistenceCoordinator::start(PersistenceOptions::new(
            data.path().join("store.db"),
            data.path().join("artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("open the store")
    else {
        panic!("the fixture store must be ready")
    };
    let runtime = crate::WorkspaceRuntime::open(Arc::new(persistence), Default::default())
        .expect("open the workspace runtime");
    let created = runtime
        .create(
            WorkspaceCreateCommand {
                selected_root: root.path().display().to_string(),
                display_name: "ignored folders".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("create the workspace");
    runtime.stop_watchers().expect("drive the catalog without a concurrent watcher");
    Fixture { _root: root, _data: data, runtime, workspace_id: created.workspace.workspace_id }
}

impl Fixture {
    fn catalog(&self) -> Arc<PathCatalog> {
        self.runtime.catalog(self.workspace_id).expect("the catalog")
    }

    fn resolver(&self) -> Arc<crate::path::SecurePathResolver> {
        self.runtime.resolver(self.workspace_id).expect("the resolver")
    }

    fn indexed(&self, value: &str) -> bool {
        self.catalog().entry(&path(value)).expect("read the catalog").is_some()
    }

    fn watched(&self, value: &str) -> bool {
        let folder = self.resolver().canonical_root().join(value);
        self.catalog()
            .watch_directories()
            .expect("watch list")
            .iter()
            .any(|watched| watched == &folder || watched.starts_with(&folder))
    }

    fn rebuild(&self) {
        self.catalog().rebuild(&self.resolver(), CorrelationId::new()).expect("rebuild");
    }
}

#[test]
fn an_ignored_folder_is_recorded_but_never_entered_or_watched() {
    let f = fixture();
    let target = f.catalog().entry(&path("target")).expect("read").expect("target is recorded");
    assert_eq!(target.kind, CatalogEntryKind::Directory);
    assert!(target.ignored);
    assert!(!f.indexed("target/a.txt"));
    assert!(!f.indexed("target/sub"));
    assert!(f.indexed("src/main.rs"));
    assert!(!f.watched("target"), "an ignored folder is not watched");
    assert!(f.watched("src"));
}

#[test]
fn a_change_inside_an_ignored_folder_is_not_indexed() {
    let f = fixture();
    std::fs::write(f.resolver().canonical_root().join("target/new.txt"), "new").expect("write");
    f.catalog()
        .apply_changes(
            &f.resolver(),
            CatalogUpdate::new(vec![(path("target/new.txt"), CatalogChange::Changed)], "test"),
            &f.catalog().options(),
            CorrelationId::new(),
        )
        .expect("apply the change");
    assert!(!f.indexed("target/new.txt"));
}

#[test]
fn editing_gitignore_redraws_the_boundary() {
    let f = fixture();
    let root = f.resolver().canonical_root().to_path_buf();
    std::fs::write(root.join(".gitignore"), "").expect("stop ignoring target");
    let event = FileWatchEvent {
        kind: FileWatchEventKind::Modify,
        paths: vec![root.join(".gitignore")],
        detail: None,
    };
    assert!(
        matches!(
            changes_for_watch_events(&f.resolver(), &f.catalog(), &[event]),
            WatchDescription::Undescribed
        ),
        "a .gitignore change is answered with a whole rebuild"
    );
    f.rebuild();
    assert!(f.indexed("target/a.txt"));
    std::fs::write(root.join(".gitignore"), "target/\n").expect("ignore target again");
    f.rebuild();
    assert!(!f.indexed("target/a.txt"));
    assert!(!f.watched("target"));
}

#[test]
fn an_ignored_folder_is_listed_from_disk_page_by_page() {
    let f = fixture();
    let (catalog, resolver) = (f.catalog(), f.resolver());
    let names = |entries: &[CatalogEntry]| {
        entries.iter().map(|entry| entry.path.as_str().to_owned()).collect::<Vec<_>>()
    };
    let (page, _) = crate::children_beyond_index(&catalog, &resolver, &path("target"), None, 10)
        .expect("list")
        .expect("an ignored folder is read from disk");
    assert_eq!(names(&page), ["target/a.txt", "target/sub"]);
    assert!(page.iter().all(|entry| entry.ignored));
    let (nested, _) =
        crate::children_beyond_index(&catalog, &resolver, &path("target/sub"), None, 10)
            .expect("list")
            .expect("a folder below it too");
    assert_eq!(names(&nested), ["target/sub/b.txt"]);
    let (first, _) = crate::children_beyond_index(&catalog, &resolver, &path("target"), None, 1)
        .expect("list")
        .expect("first page");
    assert_eq!(names(&first), ["target/a.txt"]);
    let (second, _) = crate::children_beyond_index(
        &catalog,
        &resolver,
        &path("target"),
        Some(&path("target/a.txt")),
        1,
    )
    .expect("list")
    .expect("second page");
    assert_eq!(names(&second), ["target/sub"]);
    assert!(
        crate::children_beyond_index(&catalog, &resolver, &path("src"), None, 10)
            .expect("list")
            .is_none(),
        "an indexed folder is listed from the catalog"
    );
}
