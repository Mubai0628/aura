//! Reading an authorized external file through the verified handle the permission
//! owner opened.
//!
//! These tests use real files and the real handle verification. They assert the two
//! properties the capability depends on: an external read reports exactly the facts
//! a workspace read of the same bytes reports, and it stays on the handle that was
//! verified — bounded by the requested range, with a replaced file reported instead
//! of read.
use std::{path::Path, sync::Arc, time::Duration};

use aura_domain_common::{
    CorrelationId, ExternalGrantId, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath,
};
use aura_fs_tools::{
    ExternalFindTarget, ExternalListTarget, ExternalNameMatch, ExternalSearchTarget,
    ExternalTextReadTarget, FsExternalFindRequest, FsExternalListRequest, FsExternalSearchRequest,
    FsExternalTextReadRequest, FsToolError, FsToolPolicy, FsToolService,
};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{CatalogOptions, WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::{
    FsNodeKind, FsReadTextRequest, LargeFileReadMode, SessionSummary, TextReadRange,
    WorkspaceCreateCommand, WorkspaceFileTarget,
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
                    display_name: "external read".to_owned(),
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
                    title: "external session".to_owned(),
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

    fn service(&self, policy: FsToolPolicy) -> Arc<FsToolService> {
        FsToolService::new(Arc::clone(&self.workspace), Arc::clone(&self.persistence), policy)
            .expect("fs service")
    }

    /// A target whose handle was really verified by the platform layer, which is
    /// the only thing the read is allowed to trust.
    fn target(&self, name: &str) -> ExternalTextReadTarget {
        let path = self.external.path().join(name);
        let file = aura_windows_platform::open_verified_existing(
            &path,
            aura_windows_platform::NativeFileAccess::Read,
        )
        .expect("a verified external handle");
        ExternalTextReadTarget {
            grant_id: Some(ExternalGrantId::new()),
            session_id: self.session_id,
            display_path: path.display().to_string(),
            relative_path: WorkspacePath::parse(name).expect("relative path"),
            file,
        }
    }

    /// The same for one authorized external directory.
    fn list_target(&self, name: &str) -> ExternalListTarget {
        let path = self.external.path().join(name);
        let directory = aura_windows_platform::open_verified_existing(
            &path,
            aura_windows_platform::NativeFileAccess::Read,
        )
        .expect("a verified external directory handle");
        ExternalListTarget {
            grant_id: Some(ExternalGrantId::new()),
            session_id: self.session_id,
            display_path: path.display().to_string(),
            relative_path: WorkspacePath::parse(name).expect("relative path"),
            directory,
        }
    }

    /// The same directory as a lookup target.
    fn find_target(&self, name: &str) -> ExternalFindTarget {
        let path = self.external.path().join(name);
        let directory = aura_windows_platform::open_verified_existing(
            &path,
            aura_windows_platform::NativeFileAccess::Read,
        )
        .expect("a verified external directory handle");
        ExternalFindTarget {
            grant_id: Some(ExternalGrantId::new()),
            session_id: self.session_id,
            display_path: path.display().to_string(),
            relative_path: WorkspacePath::parse(name).expect("relative path"),
            directory,
        }
    }
}

/// One unchanged authorized directory must report one version across its pages.
///
/// A continuation is only meaningful if the token a page hands back describes the
/// directory, not the page: the caller compares the version of the page it is about
/// to ask for against the version of the page it already has, so a version that
/// changes between pages of an unchanged directory rejects every continuation. The
/// current service derives the version from "this page's entries plus whether more
/// exist", which is exactly that mistake. This is the completion review's probe
/// `unchanged_external_directory_has_one_version_across_pages`, moved in as a
/// regression: it fails on the business assertion, not on setup.
#[test]
fn unchanged_external_directory_has_one_version_across_pages() {
    let harness = Harness::new();
    let root = harness.external.path().join("tree");
    std::fs::create_dir(&root).expect("tree");
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(root.join(name), "body").expect("file");
    }
    let service = harness.service(FsToolPolicy::default());

    let first = service
        .list_external(FsExternalListRequest {
            target: harness.list_target("tree"),
            limit: 1,
            after_name: None,
        })
        .expect("first page");
    let next = service
        .list_external(FsExternalListRequest {
            target: harness.list_target("tree"),
            limit: 1,
            after_name: first.next_after_name.clone(),
        })
        .expect("second page");

    assert_eq!(
        first.scope_revision, next.scope_revision,
        "an unchanged directory must report one version across its pages, or every \
         continuation is rejected for a change that did not happen"
    );
}

/// The continuation token must be usable in the form the caller has it.
///
/// The caller resumes with the grant-relative path it received, because that is the
/// only spelling it has; the service has to interpret the token it handed out. When
/// it compares basenames instead, a continuation past a subdirectory jumps over
/// every name that sorts before it — `a.txt, b.txt, z.txt` resumes from `a` straight
/// to `z`. This is the review's probe
/// `core_relative_path_cursor_can_resume_an_external_subdirectory` as a regression.
#[test]
fn core_relative_path_cursor_can_resume_an_external_subdirectory() {
    let harness = Harness::new();
    let root = harness.external.path().join("tree");
    std::fs::create_dir(&root).expect("tree");
    for name in ["a.txt", "b.txt", "z.txt"] {
        std::fs::write(root.join(name), "body").expect("file");
    }
    let service = harness.service(FsToolPolicy::default());

    let first = service
        .list_external(FsExternalListRequest {
            target: harness.list_target("tree"),
            limit: 1,
            after_name: None,
        })
        .expect("first page");
    let after = first.entries.last().expect("one entry").relative_path.as_str().to_owned();
    let next = service
        .list_external(FsExternalListRequest {
            target: harness.list_target("tree"),
            limit: 1,
            after_name: Some(after),
        })
        .expect("second page");

    assert_eq!(
        next.entries.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(),
        vec!["b.txt"],
        "the token a page hands back must resume at the next name, not skip it"
    );
}

fn external_path(root: &Path, name: &str) -> std::path::PathBuf {
    root.join(name)
}

/// The facts an external read reports must be the facts a workspace read reports
/// for the same bytes, because both go through one decoder and one range rule.
#[test]
fn an_external_read_reports_the_same_facts_as_a_workspace_read() {
    let harness = Harness::new();
    let body = "第一行\r\nsecond line\nthird line";
    std::fs::write(external_path(harness.external.path(), "note.txt"), body)
        .expect("external file");
    std::fs::write(harness.workspace_root.path().join("note.txt"), body).expect("workspace file");
    let service = harness.service(FsToolPolicy::default());

    let external = service
        .read_external_text(FsExternalTextReadRequest {
            target: harness.target("note.txt"),
            range: TextReadRange::Full,
            explicit_encoding: None,
        })
        .expect("external read");
    let workspace = service
        .read_text(FsReadTextRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session_id,
            target: WorkspaceFileTarget { path: WorkspacePath::parse("note.txt").expect("path") },
            range: TextReadRange::Full,
            explicit_encoding: None,
        })
        .expect("workspace read");

    assert_eq!(external.text, workspace.text);
    assert_eq!(external.text, body);
    assert_eq!(external.encoding, workspace.encoding);
    assert_eq!(external.line_ending, workspace.line_ending);
    assert_eq!(external.logical_hash, workspace.logical_hash);
    assert_eq!(external.total_characters, workspace.total_characters);
    assert_eq!(external.total_lines, workspace.total_lines);
    // The external target is named in full, and its relative form is the one the
    // grant scope was checked against.
    assert_eq!(external.relative_path.as_str(), "note.txt");
    assert!(external.display_path.ends_with("note.txt"));
}

/// A range read keeps only the requested range: the file may be far larger than
/// the bytes the caller asked for.
#[test]
fn a_range_read_of_a_large_external_file_retains_only_its_range() {
    let harness = Harness::new();
    let mut body = String::new();
    for index in 0..20_000 {
        body.push_str(&format!("line {index:05}\n"));
    }
    assert!(body.len() > 200_000);
    std::fs::write(external_path(harness.external.path(), "big.txt"), &body)
        .expect("external file");
    let service = harness.service(FsToolPolicy::default());

    let result = service
        .read_external_text(FsExternalTextReadRequest {
            target: harness.target("big.txt"),
            range: TextReadRange::Lines { start_line_index: 10, count: 5 },
            explicit_encoding: None,
        })
        .expect("range read");
    assert_eq!(
        result.text.lines().collect::<Vec<_>>(),
        vec!["line 00010", "line 00011", "line 00012", "line 00013", "line 00014"]
    );
    assert_eq!(result.total_lines, 20_000, "the whole-file facts are still exact");
    assert!(
        result.peak_retained_bytes < body.len() / 4,
        "a range read must not retain the file: {} of {} bytes",
        result.peak_retained_bytes,
        body.len()
    );
}

/// The whole-read policy applies to an external file exactly as it does to a
/// workspace file.
#[test]
fn a_whole_read_above_the_limit_asks_for_a_range() {
    let harness = Harness::new();
    std::fs::write(external_path(harness.external.path(), "big.txt"), "x".repeat(64 * 1024))
        .expect("external file");
    let service = harness.service(FsToolPolicy {
        full_text_read_limit_bytes: 1024,
        large_file_read_mode: LargeFileReadMode::SmallRangeOnly,
        ..FsToolPolicy::default()
    });

    let full = service.read_external_text(FsExternalTextReadRequest {
        target: harness.target("big.txt"),
        range: TextReadRange::Full,
        explicit_encoding: None,
    });
    assert!(
        matches!(full, Err(FsToolError::LargeFileRangeRequired)),
        "a large external file needs a range: {full:?}"
    );
    let ranged = service
        .read_external_text(FsExternalTextReadRequest {
            target: harness.target("big.txt"),
            range: TextReadRange::Characters { start_character_offset: 0, count: 16 },
            explicit_encoding: None,
        })
        .expect("a range is accepted");
    assert_eq!(ranged.text.len(), 16);
}

/// A file replaced while it was read is reported, never returned as the file the
/// permission owner verified.
#[test]
fn a_replaced_external_file_is_reported_instead_of_read() {
    let harness = Harness::new();
    let path = external_path(harness.external.path(), "swap.txt");
    std::fs::write(&path, "original").expect("external file");
    let service = harness.service(FsToolPolicy::default());
    let target = harness.target("swap.txt");

    // Replace the path with a different file object while the verified handle is
    // still open. A platform that refuses the removal while the handle is held
    // cannot express the race, and the guarantee is covered by the identity check
    // itself.
    if std::fs::remove_file(&path).is_err() {
        return;
    }
    std::fs::write(&path, "replaced").expect("replacement file");

    let result = service.read_external_text(FsExternalTextReadRequest {
        target,
        range: TextReadRange::Full,
        explicit_encoding: None,
    });
    match result {
        Err(FsToolError::ExternalFileReplaced { path: reported }) => {
            assert!(reported.ends_with("swap.txt"));
        }
        other => panic!("a replaced external file must be reported: {other:?}"),
    }
}

/// The read is performed on the verified handle, so a target that was never
/// verified as a file cannot be read as one.
#[test]
fn a_directory_target_is_refused() {
    let harness = Harness::new();
    std::fs::create_dir(harness.external.path().join("folder")).expect("folder");
    let service = harness.service(FsToolPolicy::default());
    let result = service.read_external_text(FsExternalTextReadRequest {
        target: harness.target("folder"),
        range: TextReadRange::Full,
        explicit_encoding: None,
    });
    assert!(matches!(result, Err(FsToolError::InvalidRequest(_))), "{result:?}");
}

/// An authorized external directory is listed one level, sorted the same way a
/// workspace listing is, bounded by the limit and pageable through its cursor.
#[test]
fn an_external_directory_lists_one_level_in_order_and_pages() {
    let harness = Harness::new();
    let root = harness.external.path().join("tree");
    std::fs::create_dir_all(root.join("nested/deeper")).expect("nested");
    for name in ["b.txt", "A.txt", "c.txt"] {
        std::fs::write(root.join(name), "body").expect("file");
    }
    let service = harness.service(FsToolPolicy::default());

    let first = service
        .list_external(FsExternalListRequest {
            target: harness.list_target("tree"),
            limit: 2,
            after_name: None,
        })
        .expect("first page");
    // Two entries per page, in the same ordinal order a workspace listing uses.
    assert_eq!(
        first.entries.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(),
        vec!["A.txt", "b.txt"]
    );
    assert!(first.truncated, "more entries remain");
    assert!(first.entries.iter().all(|entry| entry.kind == FsNodeKind::File));
    let after = first.next_after_name.clone().expect("a continuation name");

    let second = service
        .list_external(FsExternalListRequest {
            target: harness.list_target("tree"),
            limit: 2,
            after_name: Some(after),
        })
        .expect("second page");
    let names = second.entries.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>();
    assert_eq!(names, vec!["c.txt", "nested"]);
    assert!(!second.truncated, "the directory ended");
    assert!(second.next_after_name.is_none());
    assert_eq!(second.entries[1].kind, FsNodeKind::Directory);
    // Listing never descends: the nested child is reported, not listed.
    assert!(second.entries.iter().all(|entry| !entry.name.contains("deeper")));
    // Entries are named relative to the authorized root, which is what the grant
    // scope was checked against.
    assert_eq!(
        second.entries[0].relative_path.as_str(),
        "tree/c.txt",
        "the entry path is relative to the grant root"
    );
}

/// A listing digest describes the directory's live contents, so a change to the
/// directory is visible to a caller comparing two listings.
#[test]
fn an_external_listing_digest_changes_when_the_directory_does() {
    let harness = Harness::new();
    let root = harness.external.path().join("state");
    std::fs::create_dir_all(&root).expect("dir");
    std::fs::write(root.join("one.txt"), "one").expect("file");
    let service = harness.service(FsToolPolicy::default());
    let first = service
        .list_external(FsExternalListRequest {
            target: harness.list_target("state"),
            limit: 10,
            after_name: None,
        })
        .expect("first listing");
    std::fs::write(root.join("two.txt"), "two").expect("second file");
    let second = service
        .list_external(FsExternalListRequest {
            target: harness.list_target("state"),
            limit: 10,
            after_name: None,
        })
        .expect("second listing");
    assert_ne!(
        first.scope_revision, second.scope_revision,
        "the listing digest describes the directory's live contents"
    );
    assert_eq!(second.entries.len(), 2);
}

/// A file target is not a directory: listing it is a request error, not an empty
/// directory.
#[test]
fn listing_a_file_target_is_refused() {
    let harness = Harness::new();
    std::fs::write(harness.external.path().join("one.txt"), "one").expect("file");
    let service = harness.service(FsToolPolicy::default());
    let result = service.list_external(FsExternalListRequest {
        target: harness.list_target("one.txt"),
        limit: 10,
        after_name: None,
    });
    assert!(matches!(result, Err(FsToolError::InvalidRequest(_))), "{result:?}");
}

/// An external filename lookup walks in path order, recurses only when asked, and
/// pages without skipping a candidate.
#[test]
fn an_external_lookup_walks_in_path_order_and_pages() {
    let harness = Harness::new();
    let root = harness.external.path().join("search");
    std::fs::create_dir_all(root.join("nested")).expect("nested");
    for name in ["a.txt", "nested/b.txt", "nested/c.txt", "z.txt"] {
        std::fs::write(root.join(name), "body").expect("file");
    }
    let service = harness.service(FsToolPolicy::default());

    // One page at a time, recursive: every match exactly once, in path order.
    let mut seen = Vec::new();
    let mut after = None;
    let mut pages = 0;
    loop {
        let page = service
            .find_external(
                FsExternalFindRequest {
                    target: harness.find_target("search"),
                    file_names: vec!["*.txt".to_owned()],
                    recursive: true,
                    match_mode: ExternalNameMatch::Glob,
                    case_sensitive: true,
                    after,
                    consumed_names: Vec::new(),
                    listing_directory: None,
                    pending: Vec::new(),
                    finished_directories: Vec::new(),
                    partial_listings: Vec::new(),
                    limit: 1,
                    scan_budget: 64,
                },
                None,
            )
            .expect("page");
        seen.extend(page.matches.iter().map(|entry| entry.relative_path.as_str().to_owned()));
        pages += 1;
        match page.resume_after {
            Some(path) if page.truncated => after = Some(path),
            _ => {
                assert!(page.complete, "the last page finished the walk");
                break;
            }
        }
        assert!(pages < 10, "paging terminates");
    }
    assert_eq!(
        seen,
        vec!["search/a.txt", "search/nested/b.txt", "search/nested/c.txt", "search/z.txt"],
        "the walk is in path order, complete and without repeats"
    );

    // Without recursion only the direct children are candidates.
    let shallow = service
        .find_external(
            FsExternalFindRequest {
                target: harness.find_target("search"),
                file_names: vec!["*.txt".to_owned()],
                recursive: false,
                match_mode: ExternalNameMatch::Glob,
                case_sensitive: true,
                after: None,
                consumed_names: Vec::new(),
                listing_directory: None,
                pending: Vec::new(),
                finished_directories: Vec::new(),
                partial_listings: Vec::new(),
                limit: 10,
                scan_budget: 64,
            },
            None,
        )
        .expect("shallow");
    assert_eq!(
        shallow.matches.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(),
        vec!["a.txt", "z.txt"]
    );
    assert!(shallow.complete);
}

/// A walk that ran out of budget is resumable and never claims a name is absent,
/// and an unusable pattern is a field error rather than an empty result.
#[test]
fn an_external_lookup_is_bounded_and_reports_absence_only_when_complete() {
    let harness = Harness::new();
    let root = harness.external.path().join("bounded");
    std::fs::create_dir_all(&root).expect("dir");
    for index in 0..12 {
        std::fs::write(root.join(format!("f{index:02}.txt")), "body").expect("file");
    }
    let service = harness.service(FsToolPolicy::default());
    let page = service
        .find_external(
            FsExternalFindRequest {
                target: harness.find_target("bounded"),
                file_names: vec!["f00.txt".to_owned()],
                recursive: false,
                match_mode: ExternalNameMatch::Exact,
                case_sensitive: true,
                after: None,
                consumed_names: Vec::new(),
                listing_directory: None,
                pending: Vec::new(),
                finished_directories: Vec::new(),
                partial_listings: Vec::new(),
                limit: 5,
                // A small budget stops the walk before it reaches the whole directory.
                scan_budget: 3,
            },
            None,
        )
        .expect("bounded page");
    assert_eq!(page.scanned_entries, 3, "the budget bounds one call");
    assert!(!page.complete, "an unfinished walk never reports absence");
    assert!(page.truncated);
    assert!(page.resume_after.is_some());

    let invalid = service.find_external(
        FsExternalFindRequest {
            target: harness.find_target("bounded"),
            file_names: vec!["[".to_owned()],
            recursive: false,
            match_mode: ExternalNameMatch::Glob,
            case_sensitive: false,
            after: None,
            consumed_names: Vec::new(),
            listing_directory: None,
            pending: Vec::new(),
            finished_directories: Vec::new(),
            partial_listings: Vec::new(),
            limit: 5,
            scan_budget: 64,
        },
        None,
    );
    assert!(matches!(invalid, Err(FsToolError::InvalidRequest(_))), "{invalid:?}");
}

/// A directory wider than the scan budget is not a complete answer.
///
/// The budget is spent on the directory entries the walk really reads, so a directory
/// holding more names than the walk may read stops in the middle of it. `complete` must
/// then be false even though this page found nothing: a name that was never read is not
/// a name that is absent. Continuing from the resume point still reaches the name that
/// sorts after the cut, so nothing is skipped either.
#[test]
fn a_directory_wider_than_the_budget_is_not_reported_complete() {
    let harness = Harness::new();
    let root = harness.external.path().join("wide");
    std::fs::create_dir(&root).expect("wide");
    for index in 0..12 {
        std::fs::write(root.join(format!("f{index:02}.txt")), "body").expect("file");
    }
    // The wanted name sorts after every filler, so a walk that stopped early cannot
    // have seen it.
    std::fs::write(root.join("zz-target.txt"), "target").expect("target file");
    let service = harness.service(FsToolPolicy::default());

    let first = service
        .find_external(
            FsExternalFindRequest {
                target: harness.find_target("wide"),
                file_names: vec!["zz-target.txt".to_owned()],
                recursive: false,
                match_mode: ExternalNameMatch::Exact,
                case_sensitive: false,
                after: None,
                consumed_names: Vec::new(),
                listing_directory: None,
                pending: Vec::new(),
                finished_directories: Vec::new(),
                partial_listings: Vec::new(),
                limit: 10,
                scan_budget: 4,
            },
            None,
        )
        .expect("first page");
    assert!(
        first.matches.is_empty(),
        "the walk cannot have reached the wanted name: {:?}",
        first.matches
    );
    assert!(
        !first.complete,
        "a walk that could not read the whole directory must not claim the name is absent"
    );
    assert!(first.scanned_entries <= 4, "scanned {} entries", first.scanned_entries);
    let resume = first.resume_after.clone().expect("a resume point inside the directory");

    let second = service
        .find_external(
            FsExternalFindRequest {
                target: harness.find_target("wide"),
                file_names: vec!["zz-target.txt".to_owned()],
                recursive: false,
                match_mode: ExternalNameMatch::Exact,
                case_sensitive: false,
                after: Some(resume),
                consumed_names: Vec::new(),
                listing_directory: None,
                pending: Vec::new(),
                finished_directories: Vec::new(),
                partial_listings: Vec::new(),
                limit: 10,
                scan_budget: 64,
            },
            None,
        )
        .expect("second page");
    assert_eq!(
        second.matches.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(),
        vec!["zz-target.txt"],
        "continuing from the resume point must reach the name after the cut"
    );
    assert!(second.complete, "the second page read the whole directory");
}

/// A continuation sees what changed after the walk passed its boundary.
///
/// The first page spends its whole read budget on the first entry, so it has to stop
/// before reaching the name that is asked for and say it is incomplete. The file then
/// appears — after the point the walk had reached — and resuming from the resume point
/// must find it. A walk that treated the earlier partial read as a complete absence
/// would answer "no such file" about a file that is there.
#[test]
fn an_external_lookup_continues_into_names_added_after_the_boundary() {
    let harness = Harness::new();
    let root = harness.external.path().join("tree");
    std::fs::create_dir(&root).expect("tree");
    // Three entries, so the one-entry budget below really stops inside the directory
    // and the wanted name sorts after the point the walk reached.
    for name in ["a1.txt", "a2.txt", "a3.txt"] {
        std::fs::write(root.join(name), "keep").expect("filler file");
    }
    let service = harness.service(FsToolPolicy::default());

    let first = service
        .find_external(
            FsExternalFindRequest {
                target: harness.find_target("tree"),
                file_names: vec!["b-added.txt".to_owned()],
                recursive: false,
                match_mode: ExternalNameMatch::Exact,
                case_sensitive: false,
                after: None,
                consumed_names: Vec::new(),
                listing_directory: None,
                pending: Vec::new(),
                finished_directories: Vec::new(),
                partial_listings: Vec::new(),
                limit: 10,
                scan_budget: 1,
            },
            None,
        )
        .expect("first page");
    assert!(first.matches.is_empty());
    assert!(!first.complete, "one entry of a two-entry directory is not the whole walk");
    let resume = first.resume_after.clone().expect("a resume point after the read entry");

    // The file appears after the point the walk reached.
    std::fs::write(root.join("b-added.txt"), "added").expect("added file");
    let second = service
        .find_external(
            FsExternalFindRequest {
                target: harness.find_target("tree"),
                file_names: vec!["b-added.txt".to_owned()],
                recursive: false,
                match_mode: ExternalNameMatch::Exact,
                case_sensitive: false,
                after: Some(resume),
                consumed_names: Vec::new(),
                listing_directory: None,
                pending: Vec::new(),
                finished_directories: Vec::new(),
                partial_listings: Vec::new(),
                limit: 10,
                scan_budget: 64,
            },
            None,
        )
        .expect("second page");
    assert_eq!(
        second.matches.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(),
        vec!["b-added.txt"],
        "the continuation must reach the name that appeared after the boundary"
    );
    assert!(second.complete, "the second page read the whole directory");
}

/// The same query retried from the start reports the same page.
///
/// A caller that lost a cursor, or a test that repeats a lookup, must be able to run it
/// again and get the same answer: the order comes from the path key, not from directory
/// enumeration order, so the page and its version are reproducible.
#[test]
fn an_external_lookup_retried_from_the_start_reports_the_same_page() {
    let harness = Harness::new();
    let root = harness.external.path().join("tree");
    std::fs::create_dir(&root).expect("tree");
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(root.join(name), "body").expect("file");
    }
    let service = harness.service(FsToolPolicy::default());
    let query = || FsExternalFindRequest {
        target: harness.find_target("tree"),
        file_names: vec!["b.txt".to_owned(), "c.txt".to_owned()],
        recursive: false,
        match_mode: ExternalNameMatch::Exact,
        case_sensitive: false,
        after: None,
        consumed_names: Vec::new(),
        listing_directory: None,
        pending: Vec::new(),
        finished_directories: Vec::new(),
        partial_listings: Vec::new(),
        limit: 1,
        scan_budget: 64,
    };

    let first = service.find_external(query(), None).expect("first run");
    let again = service.find_external(query(), None).expect("retry");
    assert_eq!(
        first.matches.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(),
        again.matches.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(),
        "a retried lookup must report the same match"
    );
    assert_eq!(first.scope_revision, again.scope_revision, "and the same page version");
    assert_eq!(first.resume_after, again.resume_after);
}

/// An external text search finds a literal inside the authorized tree.
///
/// It reports the facts a reader needs to judge the hit: which file, which line, where
/// in the line, and the hash of the content the match came from — plus how much it
/// really read, so "no match" can be told apart from "did not look".
#[test]
fn an_external_text_search_reports_where_the_match_is() {
    let harness = Harness::new();
    let root = harness.external.path().join("text");
    std::fs::create_dir(&root).expect("text root");
    std::fs::write(root.join("one.txt"), "alpha\nneedle here\nomega\n").expect("file");
    std::fs::write(root.join("two.txt"), "nothing to find\n").expect("file");
    let service = harness.service(FsToolPolicy::default());

    let found = service
        .search_external(
            FsExternalSearchRequest {
                target: ExternalSearchTarget {
                    grant_id: None,
                    session_id: harness.session_id,
                    display_path: root.display().to_string(),
                    relative_path: WorkspacePath::parse("text").expect("relative"),
                    directory: aura_windows_platform::open_verified_existing(
                        &root,
                        aura_windows_platform::NativeFileAccess::Read,
                    )
                    .expect("verified directory"),
                },
                pattern: aura_workspace_domain::FsSearchPattern::Literal {
                    text: "needle".to_owned(),
                    case_sensitive: true,
                },
                recursive: false,
                limit: 10,
                scan_budget: 64,
                max_file_bytes: 1024 * 1024,
                byte_budget: 1024 * 1024,
            },
            None,
        )
        .expect("search");

    assert_eq!(found.matches.len(), 1, "{:?}", found.matches);
    let found = &found.matches[0];
    assert_eq!(found.name, "one.txt");
    assert_eq!(found.line_number, 2);
    assert_eq!(found.column_character_offset, 0);
    assert_eq!(found.match_character_length, 6);
    assert!(found.line_text.contains("needle"));
    assert_eq!(
        found.logical_hash,
        aura_domain_common::ContentHash::blake3(b"alpha\nneedle here\nomega\n")
    );
    assert!(!found.line_truncated);
}

/// Cancellation stops an external walk instead of finishing work nobody waits for.
#[test]
fn an_external_walk_answers_cancellation() {
    let harness = Harness::new();
    let root = harness.external.path().join("cancel");
    std::fs::create_dir(&root).expect("cancel root");
    for index in 0..8 {
        std::fs::write(root.join(format!("f{index}.txt")), "body").expect("file");
    }
    let service = harness.service(FsToolPolicy::default());
    let token = aura_fs_tools::SearchCancel::new();
    token.cancel();

    let cancelled = service.find_external(
        FsExternalFindRequest {
            target: harness.find_target("cancel"),
            file_names: vec!["f0.txt".to_owned()],
            recursive: false,
            match_mode: ExternalNameMatch::Exact,
            case_sensitive: false,
            after: None,
            consumed_names: Vec::new(),
            listing_directory: None,
            pending: Vec::new(),
            finished_directories: Vec::new(),
            partial_listings: Vec::new(),
            limit: 10,
            scan_budget: 64,
        },
        Some(&token),
    );
    assert!(matches!(cancelled, Err(FsToolError::Cancelled)), "{cancelled:?}");

    let searched = service.search_external(
        FsExternalSearchRequest {
            target: ExternalSearchTarget {
                grant_id: None,
                session_id: harness.session_id,
                display_path: root.display().to_string(),
                relative_path: WorkspacePath::parse("cancel").expect("relative"),
                directory: aura_windows_platform::open_verified_existing(
                    &root,
                    aura_windows_platform::NativeFileAccess::Read,
                )
                .expect("verified directory"),
            },
            pattern: aura_workspace_domain::FsSearchPattern::Literal {
                text: "body".to_owned(),
                case_sensitive: true,
            },
            recursive: false,
            limit: 10,
            scan_budget: 64,
            max_file_bytes: 1024 * 1024,
            byte_budget: 1024 * 1024,
        },
        Some(&token),
    );
    assert!(matches!(searched, Err(FsToolError::Cancelled)), "{searched:?}");
}

/// A page that spends its whole scan budget must still move forward.
///
/// Six entries with a budget of two takes three pages: every continuation must name a
/// cursor the walk has not returned before, and following them has to end with all six
/// names exactly once. A budget that stops in the middle of the listing is not a reason
/// to restart the listing.
#[test]
fn an_external_lookup_that_spends_its_budget_still_moves_forward() {
    let harness = Harness::new();
    let root = harness.external.path().join("bounded");
    std::fs::create_dir(&root).expect("bounded root");
    for index in 0..6 {
        std::fs::write(root.join(format!("f{index:02}.txt")), "body").expect("file");
    }
    let service = harness.service(FsToolPolicy::default());
    let mut after = None;
    let mut consumed = Vec::new();
    let mut listing_directory = None;
    let mut pending = Vec::new();
    let mut finished = Vec::new();
    let mut partials = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut cursors = std::collections::BTreeSet::<String>::new();
    for _ in 0..12 {
        let page = service
            .find_external(
                FsExternalFindRequest {
                    target: harness.find_target("bounded"),
                    file_names: vec!["*.txt".to_owned()],
                    recursive: false,
                    match_mode: ExternalNameMatch::Glob,
                    case_sensitive: true,
                    after: after.clone(),
                    consumed_names: consumed.clone(),
                    listing_directory: listing_directory.clone(),
                    pending: pending.clone(),
                    finished_directories: finished.clone(),
                    partial_listings: partials.clone(),
                    limit: 100,
                    scan_budget: 2,
                },
                None,
            )
            .expect("bounded page");
        for found in &page.matches {
            assert!(seen.insert(found.relative_path.clone()), "a name was returned twice");
        }
        assert!(
            page.scanned_entries <= 2,
            "a continuation charges only the entries it newly reads, got {}",
            page.scanned_entries
        );
        if page.complete {
            assert_eq!(seen.len(), 6, "a complete walk reports every name once: {seen:?}");
            return;
        }
        assert!(
            page.consumed_names.len() > consumed.len(),
            "a page must take new names, not stop on the prefix it already listed"
        );
        let next = page.resume_after.clone().expect("an incomplete page names its cursor");
        assert!(
            cursors.insert(page.consumed_names.join("\n")),
            "a continuation repeated without the tree changing: {next:?}"
        );
        after = Some(next);
        consumed = page.consumed_names;
        listing_directory = page.listing_directory;
        pending = page.pending;
        finished = page.finished_directories;
        partials = page.partial_listings;
    }
    panic!("an unchanged bounding directory never finished");
}

/// Match positions are character positions, not UTF-8 byte positions.
///
/// `"前缀命中后缀"` has the match at character two; a byte offset would say six. The
/// excerpt has to be the text around the match, so a caller that highlights
/// `column_character_offset .. offset + length` marks the match itself.
#[test]
fn an_external_search_reports_character_offsets() {
    let harness = Harness::new();
    let root = harness.external.path().join("search");
    std::fs::create_dir(&root).expect("search root");
    std::fs::write(root.join("unicode.txt"), "前缀命中后缀\n").expect("unicode file");
    let service = harness.service(FsToolPolicy::default());
    let target = harness.find_target("search");

    let result = service
        .search_external(
            FsExternalSearchRequest {
                target: ExternalSearchTarget {
                    grant_id: target.grant_id,
                    session_id: target.session_id,
                    display_path: target.display_path,
                    relative_path: target.relative_path,
                    directory: target.directory,
                },
                pattern: aura_workspace_domain::FsSearchPattern::Literal {
                    text: "命中".to_owned(),
                    case_sensitive: true,
                },
                recursive: false,
                limit: 10,
                scan_budget: 64,
                max_file_bytes: 1024 * 1024,
                byte_budget: 1024 * 1024,
            },
            None,
        )
        .expect("search");

    assert_eq!(result.matches.len(), 1, "{:?}", result.matches);
    let found = &result.matches[0];
    assert_eq!(found.column_character_offset, 2, "character offset, not a byte offset");
    assert_eq!(found.match_character_length, 2, "character length, not a byte length");
    assert_eq!(found.line_text, "前缀命中后缀");
}

/// Follows every field the walk needs to resume: examined names, entries listed but
/// not yet delivered, finished directories, and directories whose listing stopped early.
fn continue_external_find(
    harness: &Harness,
    directory: &str,
    recursive: bool,
    budget: u64,
    limit: u32,
) -> (Vec<String>, bool, Vec<u64>) {
    let service = harness.service(FsToolPolicy::default());
    let mut after = None;
    let mut consumed_names = Vec::new();
    let mut listing_directory = None;
    let mut pending = Vec::new();
    let mut finished = Vec::new();
    let mut partials = Vec::new();
    let mut seen = Vec::new();
    let mut scans = Vec::new();
    for _ in 0..12 {
        let page = service
            .find_external(
                FsExternalFindRequest {
                    target: harness.find_target(directory),
                    file_names: vec!["*.txt".to_owned()],
                    recursive,
                    match_mode: ExternalNameMatch::Glob,
                    case_sensitive: true,
                    after,
                    consumed_names,
                    listing_directory,
                    pending,
                    finished_directories: finished,
                    partial_listings: partials,
                    limit,
                    scan_budget: budget,
                },
                None,
            )
            .expect("external find page");
        scans.push(page.scanned_entries);
        seen.extend(page.matches.iter().map(|entry| entry.relative_path.to_string()));
        if page.complete {
            return (seen, true, scans);
        }
        after = page.resume_after;
        consumed_names = page.consumed_names;
        listing_directory = page.listing_directory;
        pending = page.pending;
        finished = page.finished_directories;
        partials = page.partial_listings;
    }
    (seen, false, scans)
}

#[test]
fn an_external_page_keeps_names_it_listed_but_did_not_return() {
    let harness = Harness::new();
    let root = harness.external.path().join("kept");
    std::fs::create_dir(&root).expect("root");
    for name in ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt", "f.txt"] {
        std::fs::write(root.join(name), "body").expect("file");
    }
    let (seen, complete, _) = continue_external_find(&harness, "kept", true, 3, 1);
    assert!(complete, "paging finishes: {seen:?}");
    assert_eq!(
        seen,
        ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt", "f.txt"]
            .map(|name| format!("kept/{name}"))
            .to_vec(),
        "every listed name is delivered"
    );
}

#[test]
fn an_external_recursive_page_with_a_small_budget_reaches_nested_files() {
    let harness = Harness::new();
    let root = harness.external.path().join("nested");
    for name in ["a/one.txt", "b/two.txt", "z.txt"] {
        let file = root.join(name);
        std::fs::create_dir_all(file.parent().expect("parent directory")).expect("parent");
        std::fs::write(file, "body").expect("file");
    }
    let (mut seen, complete, _) = continue_external_find(&harness, "nested", true, 2, 10);
    seen.sort();
    let mut expected =
        ["a/one.txt", "b/two.txt", "z.txt"].map(|name| format!("nested/{name}")).to_vec();
    expected.sort();
    assert!(complete, "a small budget still finishes the tree: {seen:?}");
    assert_eq!(seen, expected);
}

#[test]
fn an_external_continuation_charges_only_newly_read_entries() {
    let harness = Harness::new();
    let root = harness.external.path().join("budget");
    std::fs::create_dir(&root).expect("root");
    for name in ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt", "f.txt"] {
        std::fs::write(root.join(name), "body").expect("file");
    }
    let (_, _, scans) = continue_external_find(&harness, "budget", true, 2, 10);
    assert!(scans.iter().all(|count| *count <= 2), "scan counts stay inside the budget: {scans:?}");
    assert_eq!(
        scans.iter().sum::<u64>(),
        6,
        "a resumed listing reads each directory entry once: {scans:?}"
    );
}

/// Six files in one child, with a budget smaller than that child, must all come back.
///
/// The first page can list only part of the child. That partial directory has to be
/// entered again; finishing the root and reporting complete drops the rest.
#[test]
fn an_external_partial_child_directory_is_resumed_until_every_file_is_returned() {
    let harness = Harness::new();
    let child = harness.external.path().join("root").join("child");
    std::fs::create_dir_all(&child).expect("child");
    for name in ["a", "b", "c", "d", "e", "f"] {
        std::fs::write(child.join(format!("{name}.txt")), "content").expect("file");
    }
    let (mut seen, complete, _) = continue_external_find(&harness, "root", true, 3, 20);
    seen.sort();
    let mut expected =
        ["a", "b", "c", "d", "e", "f"].map(|name| format!("root/child/{name}.txt")).to_vec();
    expected.sort();
    assert!(complete, "the walk must not finish while child names remain: {seen:?}");
    assert_eq!(seen, expected);
}

/// A cursor-supplied file that is not on disk is not a successful match.
#[test]
fn an_external_cursor_cannot_invent_a_file_match() {
    let harness = Harness::new();
    std::fs::create_dir(harness.external.path().join("root")).expect("root");
    let result = harness.service(FsToolPolicy::default()).find_external(
        FsExternalFindRequest {
            target: harness.find_target("root"),
            file_names: vec!["*.txt".to_owned()],
            recursive: false,
            match_mode: ExternalNameMatch::Glob,
            case_sensitive: true,
            after: None,
            consumed_names: Vec::new(),
            listing_directory: None,
            pending: vec![aura_fs_tools::FsExternalPending {
                relative_path: WorkspacePath::parse("root/never-existed.txt").expect("path"),
                name: "never-existed.txt".to_owned(),
                kind: FsNodeKind::File,
                byte_length: Some(123),
            }],
            finished_directories: Vec::new(),
            partial_listings: Vec::new(),
            limit: 20,
            scan_budget: 3,
        },
        None,
    );
    assert!(result.is_err(), "a missing cursor entry must not be reported as a match: {result:?}");
}

/// A cursor that calls a junction a directory must not list the junction target.
#[test]
fn an_external_cursor_does_not_follow_a_junction_out_of_the_root() {
    let harness = Harness::new();
    let root = harness.external.path().join("root");
    std::fs::create_dir(&root).expect("root");
    let outside = tempfile::tempdir().expect("outside");
    std::fs::write(outside.path().join("outside-secret.txt"), "synthetic marker").expect("secret");
    let junction = root.join("alias");
    let output = std::process::Command::new("cmd.exe")
        .args(["/c", "mklink", "/J"])
        .arg(&junction)
        .arg(outside.path())
        .output()
        .expect("mklink");
    assert!(output.status.success(), "junction fixture failed: {output:?}");
    let result = harness.service(FsToolPolicy::default()).find_external(
        FsExternalFindRequest {
            target: harness.find_target("root"),
            file_names: vec!["*.txt".to_owned()],
            recursive: true,
            match_mode: ExternalNameMatch::Glob,
            case_sensitive: true,
            after: None,
            consumed_names: Vec::new(),
            listing_directory: None,
            pending: vec![aura_fs_tools::FsExternalPending {
                relative_path: WorkspacePath::parse("root/alias").expect("path"),
                name: "alias".to_owned(),
                kind: FsNodeKind::Directory,
                byte_length: None,
            }],
            finished_directories: Vec::new(),
            partial_listings: Vec::new(),
            limit: 20,
            scan_budget: 30,
        },
        None,
    );
    std::fs::remove_dir(&junction).expect("remove junction");
    if let Ok(page) = &result {
        assert!(
            page.matches.iter().all(|entry| entry.name != "outside-secret.txt"),
            "junction target names must not be returned: {page:?}"
        );
    }
}

/// Replacing a listed directory with a junction between pages invalidates the page.
#[test]
fn an_external_continuation_stops_when_a_directory_becomes_a_junction() {
    let harness = Harness::new();
    let root = harness.external.path().join("root");
    let child = root.join("child");
    std::fs::create_dir_all(&child).expect("child");
    let outside = tempfile::tempdir().expect("outside");
    std::fs::write(outside.path().join("outside-secret.txt"), "synthetic marker").expect("secret");
    let service = harness.service(FsToolPolicy::default());
    let first = service
        .find_external(
            FsExternalFindRequest {
                target: harness.find_target("root"),
                file_names: vec!["*.txt".to_owned()],
                recursive: true,
                match_mode: ExternalNameMatch::Glob,
                case_sensitive: true,
                after: None,
                consumed_names: Vec::new(),
                listing_directory: None,
                pending: Vec::new(),
                finished_directories: Vec::new(),
                partial_listings: Vec::new(),
                limit: 20,
                scan_budget: 1,
            },
            None,
        )
        .expect("first page");
    assert!(!first.complete, "one entry of budget cannot finish the tree");
    std::fs::remove_dir(&child).expect("remove child");
    let output = std::process::Command::new("cmd.exe")
        .args(["/c", "mklink", "/J"])
        .arg(&child)
        .arg(outside.path())
        .output()
        .expect("mklink");
    assert!(output.status.success(), "junction fixture failed: {output:?}");
    let result = service.find_external(
        FsExternalFindRequest {
            target: harness.find_target("root"),
            file_names: vec!["*.txt".to_owned()],
            recursive: true,
            match_mode: ExternalNameMatch::Glob,
            case_sensitive: true,
            after: first.resume_after.clone(),
            consumed_names: first.consumed_names.clone(),
            listing_directory: first.listing_directory.clone(),
            pending: first.pending.clone(),
            finished_directories: first.finished_directories.clone(),
            partial_listings: first.partial_listings.clone(),
            limit: 20,
            scan_budget: 3,
        },
        None,
    );
    std::fs::remove_dir(&child).expect("remove junction");
    if let Ok(page) = &result {
        assert!(
            page.matches.iter().all(|entry| entry.name != "outside-secret.txt"),
            "a replaced directory must not expose names outside the root: {page:?}"
        );
    }
}

/// Two lookups of one directory keep their own position. The second lookup does not
/// move the iterator the first one will resume.
#[test]
fn interleaved_external_queries_keep_independent_positions() {
    let harness = Harness::new();
    let root = harness.external.path().join("flat");
    std::fs::create_dir(&root).expect("flat root");
    for name in ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt", "f.txt"] {
        std::fs::write(root.join(name), "body").expect("file");
    }
    let service = harness.service(FsToolPolicy::default());
    let mut page =
        service.find_external(flat_query(&harness, "*.txt", 2), None).expect("first page");
    assert!(!page.complete, "the first page stops inside the directory");
    let mut other = flat_query(&harness, "?.txt", 4);
    other.target.session_id = SessionId::new();
    service.find_external(other, None).expect("second query");
    let mut names = page.matches.iter().map(|entry| entry.name.clone()).collect::<Vec<_>>();
    for _ in 0..6 {
        if page.complete {
            break;
        }
        page = service.find_external(flat_resume(&harness, &page, 2), None).expect("resume");
        names.extend(page.matches.iter().map(|entry| entry.name.clone()));
    }
    names.sort();
    assert_eq!(names, ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt", "f.txt"]);
}

fn flat_query(harness: &Harness, pattern: &str, budget: u64) -> FsExternalFindRequest {
    FsExternalFindRequest {
        target: harness.find_target("flat"),
        file_names: vec![pattern.to_owned()],
        recursive: false,
        match_mode: ExternalNameMatch::Glob,
        case_sensitive: true,
        after: None,
        consumed_names: Vec::new(),
        listing_directory: None,
        pending: Vec::new(),
        finished_directories: Vec::new(),
        partial_listings: Vec::new(),
        limit: 20,
        scan_budget: budget,
    }
}

fn flat_resume(
    harness: &Harness,
    previous: &aura_fs_tools::FsExternalFindResult,
    budget: u64,
) -> FsExternalFindRequest {
    let mut request = flat_query(harness, "*.txt", budget);
    request.target.grant_id = previous.grant_id;
    request.after = previous.resume_after.clone();
    request.consumed_names = previous.consumed_names.clone();
    request.listing_directory = previous.listing_directory.clone();
    request.pending = previous.pending.clone();
    request.finished_directories = previous.finished_directories.clone();
    request.partial_listings = previous.partial_listings.clone();
    request
}

/// A cursor that says the directory is finished, without a page this service issued,
/// is not an empty complete result.
#[test]
fn an_external_cursor_cannot_forge_directory_exhaustion() {
    let harness = Harness::new();
    let root = harness.external.path().join("present");
    std::fs::create_dir(&root).expect("root");
    std::fs::write(root.join("existing.txt"), "body").expect("file");
    let service = harness.service(FsToolPolicy::default());
    let mut request = FsExternalFindRequest {
        target: harness.find_target("present"),
        file_names: vec!["*.txt".to_owned()],
        recursive: false,
        match_mode: ExternalNameMatch::Glob,
        case_sensitive: true,
        after: None,
        consumed_names: Vec::new(),
        listing_directory: None,
        pending: Vec::new(),
        finished_directories: Vec::new(),
        partial_listings: Vec::new(),
        limit: 20,
        scan_budget: 8,
    };
    request.finished_directories.push(request.target.relative_path.clone());
    match service.find_external(request, None) {
        Err(_) => {}
        Ok(page) => assert!(
            !(page.complete && page.matches.is_empty()),
            "a forged finished list must not report an empty complete directory"
        ),
    }
}

/// Two queries with the same arguments are not one iterator. Replaying the page
/// A already continued must not take the iterator B is still using.
#[test]
fn the_same_external_query_keeps_a_cursor_for_each_caller() {
    let harness = Harness::new();
    let root = harness.external.path().join("flat");
    std::fs::create_dir(&root).expect("flat root");
    for name in ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt", "f.txt"] {
        std::fs::write(root.join(name), "body").expect("file");
    }
    let service = harness.service(FsToolPolicy::default());
    let first = service.find_external(flat_query(&harness, "*.txt", 2), None).expect("A");
    let second = service.find_external(flat_query(&harness, "*.txt", 2), None).expect("B");
    let continued = service.find_external(flat_resume(&harness, &first, 2), None).expect("A next");
    assert!(!continued.matches.is_empty());
    let replay = service.find_external(flat_resume(&harness, &first, 2), None);
    assert!(replay.is_err(), "a consumed page must not run again: {replay:?}");
    service.find_external(flat_resume(&harness, &second, 2), None).expect("B next");
}
