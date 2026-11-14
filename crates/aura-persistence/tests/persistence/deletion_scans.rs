//! A deletion's reads of whole tables get the deletion's own budget, not an ordinary read's. On a
//! large store whose pages are not in memory yet, as right after Core starts, they take longer than
//! that, and a deletion cut off there fails as a whole. Here the read is held past an ordinary
//! read's budget before its first statement, which then has rows enough for the deadline to stop.
use crate::index_support;

use std::time::Duration;

use aura_domain_common::{
    CorrelationId, DeleteConfirmationId, RendererInstanceId, Revision, SessionId,
    WorkspaceLifecycleOperationId,
};
use aura_persistence::{DEFAULT_READ_ACQUIRE, WorkspaceLifecycleRecord, barrier::Point};
use aura_workspace_domain::{
    CatalogFreshness, SessionSummary, WorkspaceLifecycleOperation, WorkspaceLifecycleOperationKind,
    WorkspaceLifecycleState, WorkspaceLifecycleSummary, WorkspaceState,
};
use index_support::{entries, partition, start, workspace};

/// Longer than an ordinary read may take.
fn held_ms() -> u64 {
    u64::try_from((DEFAULT_READ_ACQUIRE + Duration::from_millis(500)).as_millis()).expect("ms")
}

#[test]
fn deleting_a_workspace_reads_past_an_ordinary_reads_budget() {
    let directory = tempfile::tempdir().expect("data root");
    let persistence = start(directory.path());
    let doomed = workspace(&persistence, "doomed");
    persistence
        .replace_workspace_catalog(doomed, entries(3_000), partition(3_000), CorrelationId::new())
        .expect("the index its deletion reads");
    let mut archived = persistence.workspace(doomed).expect("workspace");
    archived.state = WorkspaceState::Archived;
    archived.catalog_freshness = CatalogFreshness::Unavailable;
    archived.revision = Revision::new(archived.revision.value() + 1);
    persistence
        .update_workspace(archived, "workspace.archived", CorrelationId::new(), true)
        .expect("archive");
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

    let hold = persistence.arm_read_hold(Point::ReadConnection, held_ms());
    let deleted = persistence.permanently_delete_workspace(doomed, completed);
    assert!(hold.held_count() > 0, "the deletion's read was never held");
    assert!(deleted.is_ok(), "the deletion was cut off: {deleted:?}");
    assert!(persistence.workspace(doomed).is_err(), "the workspace is still there");
}

#[test]
fn deleting_a_session_reads_past_an_ordinary_reads_budget() {
    let directory = tempfile::tempdir().expect("data root");
    let persistence = start(directory.path());
    // The projection history the session's deletion reads whole.
    for index in 0..300 {
        workspace(&persistence, &format!("other{index}"));
    }
    let host = workspace(&persistence, "host");
    let session_id = SessionId::new();
    persistence
        .upsert_workspace_session(
            persistence.workspace(host).expect("workspace"),
            SessionSummary {
                session_id,
                workspace_id: host,
                title: "deleted".to_owned(),
                state: "idle".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("session");
    persistence.mark_session_deleting(session_id).expect("deleting");

    let hold = persistence.arm_read_hold(Point::ReadConnection, held_ms());
    let purged = persistence.purge_session(session_id);
    assert!(hold.held_count() > 0, "the deletion's read was never held");
    assert!(matches!(purged, Ok(true)), "the deletion was cut off: {purged:?}");
}
