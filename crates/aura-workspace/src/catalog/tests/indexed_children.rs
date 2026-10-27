//! A page of an indexed folder's children walks only that folder's own subtree: an empty folder
//! with many unrelated index records after it walks none of them, a sibling whose name begins like
//! the folder's (`src.bak`, `src2`) is never taken for it, and the root and the cursor keep their
//! path-order paging.
use super::*;
use aura_domain_common::{CorrelationId, RuntimeInstanceId, WorkspacePath};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace_domain::WorkspaceCreateCommand;
use std::sync::Arc;

struct Fixture {
    _root: tempfile::TempDir,
    _data: tempfile::TempDir,
    catalog: Arc<PathCatalog>,
}

fn path(value: &str) -> WorkspacePath {
    WorkspacePath::parse(value).expect("a workspace path")
}

fn fixture() -> Fixture {
    let root = tempfile::tempdir().expect("workspace folder");
    for folder in ["a-empty", "src/sub", "src.bak", "src2", "z"] {
        std::fs::create_dir_all(root.path().join(folder)).expect("folder");
    }
    for file in
        ["src/a.rs", "src/sub/b.rs", "src.bak/x.txt", "src2/y.txt", "!first.txt", "-note.txt"]
    {
        std::fs::write(root.path().join(file), "text").expect("file");
    }
    std::fs::write(root.path().join("中文.txt"), "text").expect("file");
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
                display_name: "indexed children".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("create the workspace");
    let catalog = runtime.catalog(created.workspace.workspace_id).expect("the catalog");
    catalog.wait_until_built(std::time::Duration::from_secs(60)).expect("the first build");
    runtime.stop_watchers().expect("drive the catalog without a concurrent watcher");
    assert!(
        catalog.entry(&path("a-empty")).expect("read").is_some(),
        "the empty folder is indexed"
    );
    Fixture { _root: root, _data: data, catalog }
}

/// Index records under `z/` that no disk file backs, numbered `numbers` so that each call adds new
/// records instead of replacing earlier ones.
fn add_unrelated(catalog: &PathCatalog, numbers: std::ops::Range<usize>) {
    let mut state = catalog.state.write().expect("catalog state");
    let template = state.entries.get(&path("src2/y.txt")).expect("a file record").clone();
    for index in numbers {
        let mut entry = template.clone();
        entry.path = path(&format!("z/{index:06}.txt"));
        entry.parent_path = path("z");
        entry.file_name = format!("{index:06}.txt");
        state.entries.insert(entry.path.clone(), entry);
    }
}

/// One page, with the index entries the production method iterated for it.
fn page(
    catalog: &PathCatalog,
    parent: &str,
    after: Option<&str>,
    limit: usize,
) -> (Vec<String>, usize) {
    crate::catalog::boundary::VISITED.with(|count| count.set(0));
    let (entries, _) = catalog
        .children_after(&path(parent), after.map(path).as_ref(), limit)
        .expect("a page of indexed children");
    let visited = crate::catalog::boundary::VISITED.with(std::cell::Cell::get);
    (entries.into_iter().map(|entry| entry.path.as_str().to_owned()).collect(), visited)
}

#[test]
fn an_empty_indexed_folder_walks_none_of_the_records_after_it() {
    let f = fixture();
    let mut added = 0;
    let mut walked = Vec::new();
    for total in [1_000, 10_000, 100_000] {
        add_unrelated(&f.catalog, added..total);
        added = total;
        let state = f.catalog.state.read().expect("catalog state");
        let unrelated = state.entries.keys().filter(|key| key.as_str().starts_with("z/")).count();
        drop(state);
        assert_eq!(unrelated, total, "unrelated records under z/");
        let (children, visited) = page(&f.catalog, "a-empty", None, 1_001);
        assert!(children.is_empty(), "{children:?}");
        walked.push((total, visited));
    }
    // Every size is measured before any is judged, so a failure shows how the walk grows.
    println!("index entries walked for the empty folder, by unrelated records: {walked:?}");
    for (total, visited) in walked {
        assert!(visited <= 1, "{visited} index entries walked with {total} unrelated records");
    }
}

#[test]
fn a_folder_page_takes_neither_like_named_siblings_nor_grandchildren() {
    let f = fixture();
    add_unrelated(&f.catalog, 0..1_000);
    let (children, visited) = page(&f.catalog, "src", None, 1_001);
    println!("index entries walked for src/ beside src.bak/, src2/ and 1,000 others: {visited}");
    assert_eq!(children, ["src/a.rs", "src/sub"]);
    assert!(visited <= 3, "only src/'s own subtree: {visited}");
    assert_eq!(page(&f.catalog, "src.bak", None, 1_001).0, ["src.bak/x.txt"]);
    assert_eq!(page(&f.catalog, "src2", None, 1_001).0, ["src2/y.txt"]);
    assert!(page(&f.catalog, "src/sub/b.rs", None, 10).0.is_empty(), "a file has no children");
}

#[test]
fn root_and_cursor_pages_keep_path_order_without_repeats() {
    let f = fixture();
    let all = page(&f.catalog, ".", None, 100).0;
    assert_eq!(
        all,
        ["!first.txt", "-note.txt", "a-empty", "src", "src.bak", "src2", "z", "中文.txt"]
    );
    let mut paged = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let (children, _) = page(&f.catalog, ".", after.as_deref(), 3);
        let Some(last) = children.last().cloned() else { break };
        paged.extend(children);
        after = Some(last);
    }
    assert_eq!(paged, all, "pages of three reach every child once");
    assert!(page(&f.catalog, ".", Some("中文.txt"), 3).0.is_empty(), "after the last child");
    // A cursor whose entry is gone still continues after its place in path order.
    assert_eq!(page(&f.catalog, "src", Some("src/b-gone.rs"), 10).0, ["src/sub"]);
    assert_eq!(page(&f.catalog, "src", Some("src/a-gone.rs"), 10).0, ["src/a.rs", "src/sub"]);
    assert_eq!(page(&f.catalog, "src", Some("src/a.rs"), 10).0, ["src/sub"]);
}
