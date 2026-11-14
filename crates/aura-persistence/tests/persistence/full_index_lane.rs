//! Index work with a budget of its own waits for room when the index lane is full, as the online
//! workspaces' index work fills it right after Core starts: deleting an archived workspace and
//! removing an archived workspace's stored index are not refused for a full lane.
use crate::index_support;

use std::{
    sync::Arc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

use aura_domain_common::{
    CorrelationId, DeleteConfirmationId, RendererInstanceId, Revision, WorkspaceId,
    WorkspaceLifecycleOperationId, WorkspacePath,
};
use aura_persistence::{
    INDEX_QUEUE_CAPACITY, PersistenceCoordinator, WorkspaceLifecycleRecord,
    barrier::{Barrier, Point},
};
use aura_workspace_domain::{
    CatalogFreshness, WorkspaceLifecycleOperation, WorkspaceLifecycleOperationKind,
    WorkspaceLifecycleState, WorkspaceLifecycleSummary, WorkspaceState,
};
use index_support::{entries, partition, start, workspace};

/// The writer held inside a synchronous write, with index rebuilds filling the lane behind it.
struct FullLane {
    hold: Barrier,
    holder: JoinHandle<()>,
    rebuilds: Vec<JoinHandle<()>>,
}

fn fill_the_index_lane(persistence: &Arc<PersistenceCoordinator>) -> FullLane {
    let busy = workspace(persistence, "busy");
    let hold = persistence.arm_writer_hold_once(Point::WriterExecution, Some("sync"), 0);
    let holding = Arc::clone(persistence);
    let holder = std::thread::spawn(move || {
        let path = WorkspacePath::parse("held.txt").expect("path");
        holding.record_path_use(busy, &path).expect("the held write");
    });
    assert!(hold.wait_for_arrivals(1, Duration::from_secs(10)), "the writer was never held");
    let rebuilds = (0..INDEX_QUEUE_CAPACITY)
        .map(|_| {
            let rebuilding = Arc::clone(persistence);
            std::thread::spawn(move || {
                rebuilding
                    .replace_workspace_catalog(busy, entries(1), partition(1), CorrelationId::new())
                    .expect("a rebuild");
            })
        })
        .collect();
    wait_until("the lane to fill", || {
        persistence.writer_health().index_queued >= INDEX_QUEUE_CAPACITY
    });
    FullLane { hold, holder, rebuilds }
}

impl FullLane {
    /// Lets the writer go on once one more job than before has found the lane full.
    fn release_after_a_refusal(self, persistence: &PersistenceCoordinator, refused_before: u64) {
        wait_until("a job to find the lane full", || {
            persistence.writer_health().refused_full > refused_before
        });
        self.hold.release_all();
        self.holder.join().expect("holder");
        for rebuild in self.rebuilds {
            rebuild.join().expect("rebuild");
        }
    }
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "waited too long for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn archive(persistence: &PersistenceCoordinator, workspace_id: WorkspaceId) {
    let mut archived = persistence.workspace(workspace_id).expect("workspace");
    archived.state = WorkspaceState::Archived;
    archived.catalog_freshness = CatalogFreshness::Unavailable;
    archived.revision = Revision::new(archived.revision.value() + 1);
    persistence
        .update_workspace(archived, "workspace.archived", CorrelationId::new(), true)
        .expect("archive");
}

#[test]
fn deleting_an_archived_workspace_waits_for_room_in_a_full_index_lane() {
    let directory = tempfile::tempdir().expect("data root");
    let persistence = Arc::new(start(directory.path()));
    let doomed = workspace(&persistence, "doomed");
    archive(&persistence, doomed);
    let committing = WorkspaceLifecycleRecord {
        summary: WorkspaceLifecycleSummary {
            approval_follow_up: Vec::new(),
            operation_id: WorkspaceLifecycleOperationId::new(),
            workspace_id: doomed,
            renderer_instance_id: RendererInstanceId::new(),
            operation: WorkspaceLifecycleOperationKind::PermanentDelete,
            lifecycle_revision: Revision::INITIAL,
            state: WorkspaceLifecycleState::Committing,
            ack_deadline_at: None,
            preflight: None,
            error_code: None,
        },
        operation: WorkspaceLifecycleOperation::PermanentDelete {
            delete_confirmation_id: DeleteConfirmationId::new(),
        },
    };
    persistence.create_workspace_lifecycle_operation(committing.clone()).expect("lifecycle");
    let mut completed = committing;
    completed.summary.lifecycle_revision = Revision::new(2);
    completed.summary.state = WorkspaceLifecycleState::Completed;

    let lane = fill_the_index_lane(&persistence);
    let refused_before = persistence.writer_health().refused_full;
    let deleting = Arc::clone(&persistence);
    let deletion =
        std::thread::spawn(move || deleting.permanently_delete_workspace(doomed, completed));
    lane.release_after_a_refusal(&persistence, refused_before);

    let deleted = deletion.join().expect("deletion");
    assert!(deleted.is_ok(), "the deletion was refused: {deleted:?}");
    assert!(persistence.workspace(doomed).is_err(), "the workspace is still there");
}

#[test]
fn removing_an_archived_workspaces_index_waits_for_room_in_a_full_index_lane() {
    let directory = tempfile::tempdir().expect("data root");
    let persistence = Arc::new(start(directory.path()));
    let indexed = workspace(&persistence, "indexed");
    persistence
        .replace_workspace_catalog(indexed, entries(10), partition(10), CorrelationId::new())
        .expect("the index");
    archive(&persistence, indexed);
    assert_eq!(persistence.archived_workspaces_with_index().expect("leftovers"), vec![indexed]);

    let lane = fill_the_index_lane(&persistence);
    let refused_before = persistence.writer_health().refused_full;
    let clearing = Arc::clone(&persistence);
    let clearing = std::thread::spawn(move || clearing.clear_archived_workspace_index(indexed));
    lane.release_after_a_refusal(&persistence, refused_before);

    let removed = clearing.join().expect("clean-up");
    assert!(removed.is_ok(), "the clean-up was refused: {removed:?}");
    assert!(persistence.archived_workspaces_with_index().expect("leftovers").is_empty());
}
