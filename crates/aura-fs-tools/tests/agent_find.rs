//! Model-facing filename lookup through the real `FsToolService`.
//!
//! The behaviour that matters is that this is filename lookup, not a text search:
//! it matches whole names, it can carry several names at once, it is sorted and
//! de-duplicated, and it never claims a name is absent while a page is still
//! incomplete.
use std::{path::Path, sync::Arc, time::Duration};

use aura_domain_common::{CorrelationId, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath};
use aura_fs_tools::{FsToolError, FsToolPolicy, FsToolService};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{
    CatalogChange, CatalogOptions, CatalogUpdate, WorkspaceRuntime, WorkspaceRuntimeOptions,
};
use aura_workspace_domain::{
    AgentFileFindInput, AgentFileFindMatchMode, SessionSummary, WorkspaceCreateCommand,
};

struct Harness {
    workspace: Arc<WorkspaceRuntime>,
    persistence: Arc<PersistenceCoordinator>,
    root: tempfile::TempDir,
    _data: tempfile::TempDir,
    workspace_id: WorkspaceId,
    session_id: SessionId,
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
                    selected_root: root.path().display().to_string(),
                    display_name: "find".to_owned(),
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
                    title: "find session".to_owned(),
                    state: "idle".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("create session");
        Self { workspace, persistence, root, _data: data, workspace_id, session_id }
    }

    fn service(&self) -> Arc<FsToolService> {
        FsToolService::new(
            Arc::clone(&self.workspace),
            Arc::clone(&self.persistence),
            FsToolPolicy::default(),
        )
        .expect("fs service")
    }

    /// The workspace's own path catalog, so a test can apply one bounded change and
    /// observe what it does to a query in progress.
    fn catalog(&self) -> Arc<aura_workspace::PathCatalog> {
        self.workspace.catalog(self.workspace_id).expect("catalog")
    }

    fn find(
        &self,
        service: &FsToolService,
        path: &str,
        file_names: &[&str],
        recursive: bool,
        match_mode: AgentFileFindMatchMode,
        limit: u32,
    ) -> aura_workspace_domain::AgentFileFindResult {
        service
            .find_for_agent(
                self.workspace_id,
                self.session_id,
                AgentFileFindInput {
                    path: WorkspacePath::parse(path).expect("path").into(),
                    file_names: file_names.iter().map(|name| (*name).to_owned()).collect(),
                    recursive,
                    match_mode,
                    case_sensitive: false,
                    limit,
                    cursor: None,
                },
            )
            .expect("find")
    }
}

/// A name lookup matches whole names, across several requested names at once, and
/// recurses only when asked.
#[test]
fn find_matches_whole_names_for_several_requests_and_recurses_only_when_asked() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("src/nested")).expect("src");
        std::fs::create_dir_all(root.join("docs")).expect("docs");
        std::fs::write(root.join("main.rs"), "fn main() {}").expect("top level");
        std::fs::write(root.join("src/main.rs"), "fn main() {}").expect("nested");
        std::fs::write(root.join("src/nested/main.rs"), "fn main() {}").expect("deeper");
        // Similar names that must not match an exact request.
        std::fs::write(root.join("src/main.rs.bak"), "backup").expect("backup");
        std::fs::write(root.join("docs/readme.md"), "# readme").expect("readme");
    });
    let service = harness.service();

    // Non-recursive: only the direct children of the requested directory.
    let shallow =
        harness.find(&service, ".", &["main.rs"], false, AgentFileFindMatchMode::Exact, 50);
    let names = shallow.matches.iter().map(|m| m.path.as_str().to_owned()).collect::<Vec<_>>();
    assert_eq!(names, vec!["main.rs".to_owned()], "direct children only");
    assert!(shallow.complete, "the scan finished and the page holds every match");

    // Recursive from the root finds every exact name at any depth, sorted.
    let deep = harness.find(&service, ".", &["main.rs"], true, AgentFileFindMatchMode::Exact, 50);
    let names = deep.matches.iter().map(|m| m.path.as_str().to_owned()).collect::<Vec<_>>();
    assert_eq!(
        names,
        vec!["main.rs".to_owned(), "src/main.rs".to_owned(), "src/nested/main.rs".to_owned()],
        "recursive lookup is sorted by path and excludes the .bak neighbour"
    );

    // Several names in one call.
    let several = harness.find(
        &service,
        ".",
        &["main.rs", "readme.md"],
        true,
        AgentFileFindMatchMode::Exact,
        50,
    );
    let names = several.matches.iter().map(|m| m.path.as_str().to_owned()).collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "docs/readme.md".to_owned(),
            "main.rs".to_owned(),
            "src/main.rs".to_owned(),
            "src/nested/main.rs".to_owned()
        ],
        "one call may ask for several names"
    );

    // A glob is opt-in and still matches whole names only.
    let glob = harness.find(&service, "src", &["*.rs"], true, AgentFileFindMatchMode::Glob, 50);
    let names = glob.matches.iter().map(|m| m.path.as_str().to_owned()).collect::<Vec<_>>();
    assert_eq!(
        names,
        vec!["src/main.rs".to_owned(), "src/nested/main.rs".to_owned()],
        "a glob matches names, not content, and excludes main.rs.bak"
    );
}

/// Paging is stable, and `complete` is the only basis for concluding absence.
#[test]
fn find_pages_are_stable_and_complete_only_at_the_end() {
    let harness = Harness::new(|root| {
        for index in 0..25 {
            std::fs::create_dir_all(root.join(format!("dir{index:02}"))).expect("dir");
            std::fs::write(root.join(format!("dir{index:02}/target.txt")), "body").expect("file");
        }
    });
    let service = harness.service();
    let mut seen = Vec::new();
    let mut cursor = None;
    let mut pages = 0;
    loop {
        let page = service
            .find_for_agent(
                harness.workspace_id,
                harness.session_id,
                AgentFileFindInput {
                    path: WorkspacePath::parse(".").expect("root").into(),
                    file_names: vec!["target.txt".to_owned()],
                    recursive: true,
                    match_mode: AgentFileFindMatchMode::Exact,
                    case_sensitive: false,
                    limit: 10,
                    cursor: cursor.clone(),
                },
            )
            .expect("page");
        pages += 1;
        seen.extend(page.matches.iter().map(|m| m.path.as_str().to_owned()));
        assert_eq!(
            page.complete,
            page.next_cursor.is_none(),
            "a page is complete exactly when there is nothing left to read"
        );
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
        assert!(pages < 10, "paging must terminate");
    }
    assert_eq!(pages, 3, "25 matches at 10 per page take three pages");
    assert_eq!(seen.len(), 25, "no match is skipped or repeated");
    let mut sorted = seen.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted, seen, "the walk is sorted and de-duplicated");
}

/// A cursor belongs to one query. Reusing it for a different request must be
/// refused rather than silently answered.
#[test]
fn a_cursor_is_bound_to_its_query() {
    let harness = Harness::new(|root| {
        for index in 0..15 {
            std::fs::write(root.join(format!("f{index:02}.txt")), "body").expect("file");
        }
    });
    let service = harness.service();
    let first = harness.find(&service, ".", &["*.txt"], false, AgentFileFindMatchMode::Glob, 5);
    let cursor = first.next_cursor.clone().expect("a truncated page offers a cursor");
    assert!(first.truncated && !first.complete, "a truncated page is not complete");

    // A different name set with the same cursor must be rejected.
    let mismatched = service.find_for_agent(
        harness.workspace_id,
        harness.session_id,
        AgentFileFindInput {
            path: WorkspacePath::parse(".").expect("root").into(),
            file_names: vec!["*.md".to_owned()],
            recursive: false,
            match_mode: AgentFileFindMatchMode::Glob,
            case_sensitive: false,
            limit: 5,
            cursor: Some(cursor.clone()),
        },
    );
    assert!(
        matches!(mismatched, Err(FsToolError::InvalidRequest(_))),
        "a cursor from another query must be refused"
    );

    // The matching continuation succeeds and does not repeat the first page.
    let second = service
        .find_for_agent(
            harness.workspace_id,
            harness.session_id,
            AgentFileFindInput {
                path: WorkspacePath::parse(".").expect("root").into(),
                file_names: vec!["*.txt".to_owned()],
                recursive: false,
                match_mode: AgentFileFindMatchMode::Glob,
                case_sensitive: false,
                limit: 5,
                cursor: Some(cursor),
            },
        )
        .expect("continuation");
    let first_paths = first.matches.iter().map(|m| m.path.clone()).collect::<Vec<_>>();
    assert!(
        second.matches.iter().all(|m| !first_paths.contains(&m.path)),
        "the continuation must not repeat the first page"
    );
}

/// The cursor belongs to the whole query, and recursion is part of the query.
///
/// A shallow first page and a deep second page are not the same walk: the deep
/// walk starts at the beginning and finds descendants the shallow page could not
/// see. Accepting the shallow cursor would skip exactly those descendants and
/// still report the result as complete.
#[test]
fn a_cursor_is_bound_to_the_recursive_scope() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("a")).expect("a");
        for path in ["a/early.txt", "z.txt", "zz.txt"] {
            std::fs::write(root.join(path), "body").expect("file");
        }
    });
    let service = harness.service();
    let first = harness.find(&service, ".", &["*.txt"], false, AgentFileFindMatchMode::Glob, 1);
    let cursor = first.next_cursor.clone().expect("a truncated page offers a cursor");
    let widened = service.find_for_agent(
        harness.workspace_id,
        harness.session_id,
        AgentFileFindInput {
            path: WorkspacePath::parse(".").expect("root").into(),
            file_names: vec!["*.txt".to_owned()],
            recursive: true,
            match_mode: AgentFileFindMatchMode::Glob,
            case_sensitive: false,
            limit: 1,
            cursor: Some(cursor),
        },
    );
    assert!(
        matches!(widened, Err(FsToolError::InvalidRequest(_))),
        "a cursor from a non-recursive page must not answer a recursive query: {widened:?}"
    );
}

/// A page stops as soon as it is full instead of walking the whole index, and a
/// continuation resumes where the previous page stopped examining.
///
/// The work a call does has to be bounded by what it returns: a lookup in a large
/// workspace must not read every entry to answer a question whose answer is the
/// first one.
#[test]
fn a_page_stops_at_the_first_match_and_resumes_after_it() {
    let harness = Harness::new(|root| {
        // The match sorts first, so a full walk would still be visible in
        // `scanned_entries`.
        std::fs::write(root.join("aaa-target.txt"), "body").expect("match");
        for index in 0..200 {
            std::fs::write(root.join(format!("zzz-{index:03}.txt")), "body").expect("filler");
        }
    });
    let service = harness.service();
    let first =
        harness.find(&service, ".", &["aaa-target.txt"], false, AgentFileFindMatchMode::Exact, 1);
    assert_eq!(first.matches.len(), 1);
    assert_eq!(
        first.scanned_entries, 1,
        "the walk must stop at the match, not read every entry in the scope"
    );

    // A query with no match still reports how much it really examined, and a
    // budget-limited page stays resumable rather than claiming the name is absent.
    let missing =
        harness.find(&service, ".", &["nothing.txt"], false, AgentFileFindMatchMode::Exact, 1);
    assert!(missing.matches.is_empty());
    assert!(missing.complete, "a small scope really is fully covered");
    assert_eq!(missing.scanned_entries, 201);
}

/// The scope version describes the queried scope, so work elsewhere in the
/// workspace cannot invalidate a page in progress.
#[test]
fn a_cursor_survives_a_change_outside_its_scope_but_not_inside_it() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("alpha")).expect("alpha");
        std::fs::create_dir_all(root.join("beta")).expect("beta");
        for index in 0..30 {
            std::fs::write(root.join(format!("alpha/f{index:02}.txt")), "body").expect("alpha");
        }
        std::fs::write(root.join("beta/other.txt"), "body").expect("beta");
    });
    let service = harness.service();
    let first = harness.find(&service, "alpha", &["*.txt"], false, AgentFileFindMatchMode::Glob, 5);
    let cursor = first.next_cursor.clone().expect("a truncated page offers a cursor");
    assert!(!first.complete && first.truncated);

    // A change in another directory must not make the caller restart.
    std::fs::write(harness.root.path().join("beta/other.txt"), "changed elsewhere")
        .expect("unrelated write");
    let resolver = harness.workspace.resolver(harness.workspace_id).expect("resolver");
    harness
        .catalog()
        .apply_changes(
            &resolver,
            CatalogUpdate::new(
                vec![(
                    WorkspacePath::parse("beta/other.txt").expect("path"),
                    CatalogChange::Changed,
                )],
                "unrelated",
            ),
            &harness.catalog().options(),
            CorrelationId::new(),
        )
        .expect("unrelated update");
    let continued = service
        .find_for_agent(
            harness.workspace_id,
            harness.session_id,
            AgentFileFindInput {
                path: WorkspacePath::parse("alpha").expect("path").into(),
                file_names: vec!["*.txt".to_owned()],
                recursive: false,
                match_mode: AgentFileFindMatchMode::Glob,
                case_sensitive: false,
                limit: 5,
                cursor: Some(cursor.clone()),
            },
        )
        .expect("an unrelated change must not invalidate the cursor");
    assert_eq!(
        continued.scope_revision, first.scope_revision,
        "the scope version describes the queried directory only"
    );

    // A change inside the scope does invalidate it: the page would otherwise be a
    // mixture of two different states.
    std::fs::write(harness.root.path().join("alpha/f00.txt"), "changed inside")
        .expect("scope write");
    harness
        .catalog()
        .apply_changes(
            &resolver,
            CatalogUpdate::new(
                vec![(
                    WorkspacePath::parse("alpha/f00.txt").expect("path"),
                    CatalogChange::Changed,
                )],
                "inside scope",
            ),
            &harness.catalog().options(),
            CorrelationId::new(),
        )
        .expect("scope update");
    let invalidated = service.find_for_agent(
        harness.workspace_id,
        harness.session_id,
        AgentFileFindInput {
            path: WorkspacePath::parse("alpha").expect("path").into(),
            file_names: vec!["*.txt".to_owned()],
            recursive: false,
            match_mode: AgentFileFindMatchMode::Glob,
            case_sensitive: false,
            limit: 5,
            cursor: Some(cursor),
        },
    );
    assert!(
        matches!(invalidated, Err(FsToolError::InvalidRequest(_))),
        "a change inside the scope must make the caller restart: {invalidated:?}"
    );
}

/// An unusable match pattern is a field error, not an empty result.
///
/// Reporting "no matches, complete" for a pattern the engine cannot compile tells
/// the model the file does not exist, which is a wrong fact rather than a wrong
/// page.
#[test]
fn an_invalid_glob_is_a_field_error_not_an_empty_result() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join("one.txt"), "body").expect("file");
    });
    assert!(
        harness.root.path().join("one.txt").is_file(),
        "the fixture file the lookup must be able to find exists"
    );
    let service = harness.service();
    let result = service.find_for_agent(
        harness.workspace_id,
        harness.session_id,
        AgentFileFindInput {
            path: WorkspacePath::parse(".").expect("root").into(),
            file_names: vec!["[".to_owned()],
            recursive: true,
            match_mode: AgentFileFindMatchMode::Glob,
            case_sensitive: false,
            limit: 1,
            cursor: None,
        },
    );
    assert!(
        matches!(result, Err(FsToolError::InvalidRequest(_))),
        "an invalid glob must be a correctable field error: {result:?}"
    );
}
