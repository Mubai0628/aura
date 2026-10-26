//! Archiving a workspace whose watcher thread is inside an index commit: the archive does not wait
//! for that thread, the other workspaces are projected meanwhile, and the archived workspace's
//! stored index is removed afterwards in the background.
mod runtime_support;

use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

use aura_domain_common::{CorrelationId, RendererInstanceId, WorkspaceId};
use aura_workspace::{
    CatalogCommitHold, CatalogOptions, WorkspaceRuntime, WorkspaceRuntimeOptions,
};
use aura_workspace_domain::{
    WorkspaceCreateCommand, WorkspaceLifecycleOperation, WorkspaceLifecycleState, WorkspaceState,
};
use runtime_support::{begin_and_ack, persistence};

/// Parks the next commit of a catalog until released. Dropping it releases it too, so a failed
/// assertion never leaves the watcher thread parked.
struct Gate {
    released: Arc<(Mutex<bool>, Condvar)>,
}

impl Gate {
    fn new() -> (CatalogCommitHold, mpsc::Receiver<()>, Self) {
        let (entered, arrivals) = mpsc::channel();
        let released = Arc::new((Mutex::new(false), Condvar::new()));
        let waiting = Arc::clone(&released);
        let hold = CatalogCommitHold::new(move || {
            let _ = entered.send(());
            let (released, wake) = &*waiting;
            let mut open = released.lock().expect("gate");
            while !*open {
                open = wake.wait(open).expect("gate");
            }
        });
        (hold, arrivals, Self { released })
    }

    fn release(&self) {
        let (released, wake) = &*self.released;
        *released.lock().expect("gate") = true;
        wake.notify_all();
    }
}

impl Drop for Gate {
    fn drop(&mut self) {
        self.release();
    }
}

fn add(runtime: &Arc<WorkspaceRuntime>, root: &std::path::Path, name: &str) -> WorkspaceId {
    std::fs::write(root.join("a.txt"), "a").expect("file");
    runtime
        .create(
            WorkspaceCreateCommand {
                display_name: name.to_owned(),
                selected_root: root.display().to_string(),
            },
            CorrelationId::new(),
        )
        .expect("workspace")
        .workspace
        .workspace_id
}

#[test]
fn archiving_does_not_wait_for_a_watcher_thread_inside_an_index_commit() {
    let data = tempfile::tempdir().expect("data root");
    let busy_root = tempfile::tempdir().expect("busy root");
    let other_root = tempfile::tempdir().expect("other root");
    let persistence = persistence(&data);
    let runtime = WorkspaceRuntime::open(
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
    let busy = add(&runtime, busy_root.path(), "Busy");
    add(&runtime, other_root.path(), "Other");
    runtime.wait_until_indexed(Duration::from_secs(60)).expect("indexed");

    // The busy workspace's watcher thread parks inside its next index commit.
    let (hold, arrivals, gate) = Gate::new();
    runtime.catalog(busy).expect("catalog").arm_commit_hold(hold);
    std::fs::write(busy_root.path().join("b.txt"), "b").expect("new file");
    arrivals.recv_timeout(Duration::from_secs(30)).expect("the watcher reached its commit");

    let revision = persistence.workspace(busy).expect("workspace").revision;
    let (archived, archive_done) = mpsc::channel();
    let archiving = Arc::clone(&runtime);
    let started = Instant::now();
    std::thread::spawn(move || {
        let summary = begin_and_ack(
            &archiving,
            busy,
            revision,
            RendererInstanceId::new(),
            WorkspaceLifecycleOperation::Archive,
        );
        let _ = archived.send((summary, started.elapsed()));
    });
    // While the archive runs, the workspaces are projected.
    std::thread::sleep(Duration::from_millis(200));
    let (projected, projection_done) = mpsc::channel();
    let projecting = Arc::clone(&runtime);
    std::thread::spawn(move || {
        let started = Instant::now();
        let result = projecting.workspace_files_projection().map(|_| ());
        let _ = projected.send((result, started.elapsed()));
    });
    let (projection, projection_took) =
        projection_done.recv_timeout(Duration::from_secs(2)).expect("the projection was read");
    projection.expect("projection");
    let (summary, took) =
        archive_done.recv_timeout(Duration::from_secs(5)).expect("the archive finished");
    eprintln!("archived in {took:?}; the projection read in {projection_took:?}");
    assert_eq!(summary.state, WorkspaceLifecycleState::Completed);
    assert_eq!(persistence.workspace(busy).expect("workspace").state, WorkspaceState::Archived);

    // Once the parked commit goes on it is refused, and the stored index is removed.
    gate.release();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !persistence.archived_workspaces_with_index().expect("leftovers").is_empty() {
        assert!(Instant::now() < deadline, "the archived workspace's index was never removed");
        std::thread::sleep(Duration::from_millis(20));
    }
}
