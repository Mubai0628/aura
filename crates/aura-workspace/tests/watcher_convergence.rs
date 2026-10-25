//! Convergence between the index and the files on disk, driven by the real watcher.
//!
//! The user-visible failure this file pins down: a directory created and deleted by a
//! Shell script stayed in the root listing for the rest of the session, at an unchanged
//! revision, while opening it answered `fs.not_found`. The index was never told, because
//! the watcher was rebuilt after every batch — `WorkspaceWatchHandle` drained the old
//! channel, built a replacement `WorkspaceFileWatcher`, and dropped the old receiver
//! with whatever had arrived in between still buffered on it.
//!
//! `index_correctness.rs` deliberately stops the watcher and applies one named change at
//! a time, so it cannot see this: the loss is in *delivery*, not in the delta logic. The
//! cases below therefore drive the real runtime, the real watcher and a real isolated
//! database, and they change the tree with `std::fs` instead of Aura's own write path so
//! nothing but the watcher can explain the result.
//!
//! The central case holds the watcher thread at exactly the instant the old code threw
//! the channel away (`watcher_batch_hook`), deletes the directory there, and then
//! requires the index to converge. A sleep could only make that race unlikely; the
//! barrier makes it certain.
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use aura_domain_common::{CorrelationId, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{
    CatalogOptions, PathCatalog, WatcherBatchHook, WorkspaceRuntime, WorkspaceRuntimeOptions,
};
use aura_workspace_domain::{SessionSummary, WorkspaceCreateCommand};

/// How long a case waits for convergence before it fails.
///
/// This is a test deadline, not a product promise: the watcher is asynchronous, and the
/// point of each case is *whether* the index converges, never how quickly.
const CONVERGENCE_DEADLINE: Duration = Duration::from_secs(20);

struct Harness {
    root: tempfile::TempDir,
    data: tempfile::TempDir,
    persistence: Arc<PersistenceCoordinator>,
    runtime: Arc<WorkspaceRuntime>,
    workspace_id: WorkspaceId,
    catalog: Arc<PathCatalog>,
}

impl Harness {
    fn new(hook: Option<WatcherBatchHook>) -> Self {
        let root = tempfile::tempdir().expect("workspace root");
        let data = tempfile::tempdir().expect("data root");
        let persistence = Arc::new(open_persistence(&data));
        let runtime = WorkspaceRuntime::open(
            Arc::clone(&persistence),
            WorkspaceRuntimeOptions {
                catalog: CatalogOptions::with_index_file_limit(64 * 1024 * 1024),
                // Short windows keep the cases quick; the behaviour under test does not
                // depend on either duration.
                watcher_coalesce: Duration::from_millis(50),
                watcher_poll_interval: Duration::from_millis(100),
                watcher_batch_hook: hook,
                ..WorkspaceRuntimeOptions::default()
            },
        )
        .expect("workspace runtime");
        let created = runtime
            .create(
                WorkspaceCreateCommand {
                    selected_root: root.path().display().to_string(),
                    display_name: "watcher convergence".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("create workspace");
        let workspace_id: WorkspaceId = created.workspace.workspace_id;
        let current = persistence.workspace(workspace_id).expect("current workspace");
        persistence
            .upsert_workspace_session(
                current,
                SessionSummary {
                    session_id: SessionId::new(),
                    workspace_id,
                    title: "watcher session".to_owned(),
                    state: "idle".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("create session");
        let catalog = runtime.catalog(workspace_id).expect("catalog");
        Self { root, data, persistence, runtime, workspace_id, catalog }
    }

    fn indexed(&self, path: &str) -> bool {
        self.catalog
            .entry(&WorkspacePath::parse(path).expect("workspace path"))
            .expect("entry")
            .is_some()
    }

    fn wait_for<F>(&self, what: &str, mut ready: F)
    where
        F: FnMut() -> bool,
    {
        let deadline = Instant::now() + CONVERGENCE_DEADLINE;
        while Instant::now() < deadline {
            if ready() {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!(
            "{what} did not happen within {CONVERGENCE_DEADLINE:?}; index holds {:?}",
            self.catalog
                .entries()
                .expect("entries")
                .into_iter()
                .map(|entry| entry.path.as_str().to_owned())
                .collect::<Vec<_>>()
        );
    }

    fn wait_indexed(&self, path: &str) {
        self.wait_for(&format!("{path} to be indexed"), || self.indexed(path));
    }

    fn wait_gone(&self, path: &str) {
        self.wait_for(&format!("{path} to leave the index"), || !self.indexed(path));
    }

    fn path(&self, relative: &str) -> std::path::PathBuf {
        self.root.path().join(relative)
    }
}

fn open_persistence(data: &tempfile::TempDir) -> PersistenceCoordinator {
    match PersistenceCoordinator::start(PersistenceOptions::new(
        data.path().join("aura.sqlite3"),
        data.path().join("artifacts"),
        RuntimeInstanceId::new(),
    ))
    .expect("persistence startup")
    {
        PersistenceStartup::Ready(value) => value,
        PersistenceStartup::RecoveryRequired(value) => {
            panic!("unexpected recovery: {}", value.reason)
        }
    }
}

/// A barrier the watcher thread must pass through on its first batch.
///
/// It is built from atomics rather than channels because the hook has to be `Send +
/// Sync`: it is stored in the runtime's options, which are shared. The thread reports
/// entry by setting a flag and then spins until the case releases it — the wait is on
/// the watcher thread, and the case observes it by polling with a deadline.
#[derive(Default)]
struct BarrierState {
    entered: std::sync::atomic::AtomicBool,
    release: std::sync::atomic::AtomicBool,
    used: std::sync::atomic::AtomicBool,
}

struct BatchBarrier {
    state: Arc<BarrierState>,
}

impl BatchBarrier {
    /// A one-shot barrier: the first batch holds the thread, later ones pass through.
    ///
    /// Later batches must not block, or the runtime's own shutdown — which joins this
    /// thread — would wait for a release the case has no reason to send.
    fn new() -> (WatcherBatchHook, Self) {
        let state = Arc::new(BarrierState::default());
        let hook = {
            let state = Arc::clone(&state);
            // The hook fires on the watcher thread for every batch. The first time, it
            // reports that a batch has been applied and then waits — which is the instant
            // the old implementation replaced the watcher object.
            WatcherBatchHook::new(move || {
                use std::sync::atomic::Ordering;
                if state.used.swap(true, Ordering::AcqRel) {
                    return;
                }
                state.entered.store(true, Ordering::Release);
                while !state.release.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(5));
                }
            })
        };
        (hook, Self { state })
    }

    /// Waits until a batch has been applied, and returns while the thread is held.
    fn wait_until_held(&self) {
        let deadline = Instant::now() + CONVERGENCE_DEADLINE;
        while Instant::now() < deadline {
            if self.state.entered.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the watcher never reached the barrier");
    }

    fn release(&self) {
        self.state.release.store(true, std::sync::atomic::Ordering::Release);
    }
}

/// The regression: an event that arrives while a batch is being applied is processed.
///
/// The directory is created, indexed, and then deleted while the watcher thread is held
/// at the barrier. With the old implementation the deletion was delivered to a channel
/// that had already been replaced, and the index kept the directory forever.
#[test]
fn an_event_that_arrives_during_an_update_is_still_processed() {
    let (hook, barrier) = BatchBarrier::new();
    let harness = Harness::new(Some(hook));
    std::fs::create_dir(harness.path("probe")).expect("create probe");
    harness.wait_indexed("probe");

    // The batch that indexed `probe` has been applied, and the thread is held here.
    barrier.wait_until_held();
    std::fs::remove_dir(harness.path("probe")).expect("delete probe");
    // Let the backend deliver the removal into the channel that already exists. This is
    // not the proof of anything: it only makes the *old* implementation lose the event
    // deterministically instead of occasionally.
    std::thread::sleep(Duration::from_millis(400));
    barrier.release();

    harness.wait_gone("probe");
}

/// A directory created and deleted with no Aura write path involved still converges.
#[test]
fn a_directory_created_and_deleted_quickly_converges() {
    let harness = Harness::new(None);
    let directory = harness.path("aura_tmp_like_probe");
    std::fs::create_dir(&directory).expect("create");
    std::fs::write(directory.join("payload.txt"), "written and removed at once").expect("write");
    std::fs::remove_dir_all(&directory).expect("remove");

    // Whatever the watcher managed to deliver, the index has to end up agreeing with a
    // tree that does not contain the directory. Nothing else will ever touch it again.
    harness.wait_gone("aura_tmp_like_probe");
    assert!(!directory.exists(), "the fixture removed the directory from disk");
}

/// Content written into a directory before it was watched is still indexed.
///
/// The directory is created, indexed, and only then registered with the backend. A file
/// written in that window produces no event from any watcher — the parent's registration
/// is not recursive — so the only way it can be indexed is the bounded re-read of the
/// directory that was just registered.
#[test]
fn contents_written_during_registration_are_indexed() {
    let (hook, barrier) = BatchBarrier::new();
    let harness = Harness::new(Some(hook));
    std::fs::create_dir(harness.path("fresh")).expect("create fresh");
    harness.wait_indexed("fresh");
    barrier.wait_until_held();

    // Nothing is watching `fresh` yet, so neither of these writes produces an event.
    std::fs::write(harness.path("fresh/inner.txt"), "inner").expect("inner file");
    std::fs::create_dir(harness.path("fresh/nested")).expect("nested directory");
    std::fs::write(harness.path("fresh/nested/deep.txt"), "deep").expect("deep file");
    barrier.release();

    harness.wait_indexed("fresh/inner.txt");
    harness.wait_indexed("fresh/nested/deep.txt");
}

/// A directory deleted while it was watched also leaves the watch set.
///
/// The set is derived from the index, and it used to be extended on every discovery and
/// never pruned, so every temporary directory a workspace had ever contained stayed
/// registered for the life of the process.
#[test]
fn a_deleted_directory_leaves_the_watch_set() {
    let harness = Harness::new(None);
    std::fs::create_dir(harness.path("gone_soon")).expect("create");
    harness.wait_indexed("gone_soon");
    let watched_before = harness.catalog.watch_directories().expect("watch directories");
    assert!(
        watched_before.iter().any(|directory| directory.ends_with("gone_soon")),
        "the new directory must be watched once it is indexed"
    );

    std::fs::remove_dir(harness.path("gone_soon")).expect("remove");
    harness.wait_gone("gone_soon");
    harness.wait_for("the watch set to drop the deleted directory", || {
        !harness
            .catalog
            .watch_directories()
            .expect("watch directories")
            .iter()
            .any(|directory| directory.ends_with("gone_soon"))
    });
}

/// Deleting one directory does not invalidate an unrelated directory's cursor.
#[test]
fn an_unrelated_directory_keeps_its_revision_and_cursor() {
    let harness = Harness::new(None);
    for name in ["alpha", "beta"] {
        std::fs::create_dir(harness.path(name)).expect("create");
        std::fs::write(harness.path(name).join("file.txt"), name).expect("file");
    }
    harness.wait_indexed("alpha/file.txt");
    harness.wait_indexed("beta/file.txt");
    let beta = WorkspacePath::parse("beta").expect("beta path");
    let (entries, _, beta_revision) =
        harness.catalog.children_page(&beta, None, 10).expect("beta page");
    assert!(
        entries.iter().any(|entry| entry.path.as_str() == "beta/file.txt"),
        "the fixture indexed the file beta's page is asked about"
    );

    std::fs::remove_dir_all(harness.path("alpha")).expect("remove alpha");
    harness.wait_gone("alpha/file.txt");

    let (_, catalog_revision, beta_revision_after) =
        harness.catalog.children_page(&beta, None, 10).expect("beta page after");
    assert_eq!(
        beta_revision, beta_revision_after,
        "an unrelated directory's revision moved with a change it did not contain"
    );
    assert!(catalog_revision.value() > 0, "the catalog itself did move on: {catalog_revision:?}");
}

/// Reopening the isolated store does not bring a deleted directory back.
#[test]
fn a_reopened_store_does_not_resurrect_a_deleted_directory() {
    let harness = Harness::new(None);
    std::fs::create_dir(harness.path("transient")).expect("create");
    harness.wait_indexed("transient");
    std::fs::remove_dir(harness.path("transient")).expect("remove");
    harness.wait_gone("transient");
    let workspace_id = harness.workspace_id;
    let data = harness.data;
    let root = harness.root;
    let persistence = Arc::clone(&harness.persistence);
    drop(harness.runtime);
    drop(persistence);

    // A fresh process opening the same data root installs the online workspace again and
    // rebuilds the index from the durable store, which is where a stale entry would
    // survive a restart.
    let persistence = Arc::new(open_persistence(&data));
    let reopened = WorkspaceRuntime::open(
        persistence,
        WorkspaceRuntimeOptions {
            catalog: CatalogOptions::with_index_file_limit(64 * 1024 * 1024),
            ..WorkspaceRuntimeOptions::default()
        },
    )
    .expect("reopen runtime");
    let catalog = reopened.catalog(workspace_id).expect("catalog");
    let entries = catalog.entries().expect("entries");
    assert!(
        !entries.iter().any(|entry| entry.path.as_str() == "transient"),
        "a directory deleted before the restart came back with the index"
    );
    assert!(!root.path().join("transient").exists());
}
