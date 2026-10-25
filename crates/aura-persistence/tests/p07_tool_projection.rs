use aura_domain_common::{Revision, RuntimeInstanceId, WorkspaceId};
use aura_persistence::{
    PersistenceCoordinator, PersistenceOptions, PersistenceStartup, ToolProjectionDraft,
};
use aura_projection_contract::{ProjectionChange, ProjectionKind, ProjectionScope};
use aura_tool_projection::{
    ToolDraftListProjection, ToolPackageListProjection, WorkspaceToolPinsProjection,
};
use tempfile::TempDir;

fn start(directory: &TempDir, runtime_instance_id: RuntimeInstanceId) -> PersistenceCoordinator {
    let options = PersistenceOptions::new(
        directory.path().join("data/aura.db"),
        directory.path().join("data/artifact-objects"),
        runtime_instance_id,
    );
    match PersistenceCoordinator::start(options).expect("start persistence") {
        PersistenceStartup::Ready(coordinator) => coordinator,
        PersistenceStartup::RecoveryRequired(recovery) => {
            panic!("unexpected recovery requirement: {}", recovery.reason)
        }
    }
}

#[test]
fn current_global_lists_commit_explicit_empty_snapshots_for_a_new_runtime() {
    let directory = tempfile::tempdir().expect("temporary root");
    let runtime_instance_id = RuntimeInstanceId::new();
    let coordinator = start(&directory, runtime_instance_id);

    let changes = coordinator
        .commit_current_tool_global_list_projections()
        .expect("commit current global Tool lists");

    // Retired the P07 Draft list with the Draft commands; only the package list remains.
    assert_eq!(changes.len(), 1);
    assert!(matches!(
        &changes[0],
        ProjectionChange::ToolPackageList { payload, revision, .. }
            if payload.packages.is_empty() && *revision == Revision::INITIAL
    ));
}

#[test]
fn tool_projection_revisions_are_independent_per_kind_and_scope() {
    let directory = tempfile::tempdir().expect("temporary root");
    let runtime_instance_id = RuntimeInstanceId::new();
    let coordinator = start(&directory, runtime_instance_id);
    let workspace_id = WorkspaceId::new();

    let first = coordinator
        .commit_tool_projection_snapshots(vec![
            ToolProjectionDraft::ToolPackageList(ToolPackageListProjection {
                packages: Vec::new(),
            }),
            ToolProjectionDraft::WorkspaceToolPins(WorkspaceToolPinsProjection {
                workspace_id,
                pin_set_revision: Revision::INITIAL,
                pins: Vec::new(),
            }),
        ])
        .expect("commit initial tool projections");
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].revision(), Revision::INITIAL);
    assert_eq!(first[1].revision(), Revision::INITIAL);

    let second = coordinator
        .commit_tool_projection_snapshots(vec![
            ToolProjectionDraft::ToolPackageList(ToolPackageListProjection {
                packages: Vec::new(),
            }),
            ToolProjectionDraft::ToolDraftList(ToolDraftListProjection { drafts: Vec::new() }),
        ])
        .expect("commit subsequent tool projections");
    assert_eq!(second[0].revision(), Revision::new(2));
    assert_eq!(second[1].revision(), Revision::INITIAL);

    let latest_global = coordinator
        .latest_tool_projection(ProjectionKind::ToolPackageList, ProjectionScope::Global)
        .expect("read package list projection")
        .expect("package list projection exists");
    assert_eq!(latest_global.runtime_instance_id(), runtime_instance_id);
    assert_eq!(latest_global.revision(), Revision::new(2));
    assert!(matches!(latest_global, ProjectionChange::ToolPackageList { .. }));

    let latest_workspace = coordinator
        .latest_tool_projection(
            ProjectionKind::WorkspaceToolPins,
            ProjectionScope::Workspace { workspace_id },
        )
        .expect("read workspace pins projection")
        .expect("workspace pins projection exists");
    assert_eq!(latest_workspace.revision(), Revision::INITIAL);
    assert!(matches!(latest_workspace, ProjectionChange::WorkspaceToolPins { .. }));
}

#[test]
fn committed_tool_projection_changes_are_published_only_when_requested() {
    let directory = tempfile::tempdir().expect("temporary root");
    let runtime_instance_id = RuntimeInstanceId::new();
    let coordinator = start(&directory, runtime_instance_id);
    let updates = coordinator.subscribe_tool_projections().expect("subscribe to Tool changes");

    let committed = coordinator
        .commit_tool_projection_snapshots(vec![ToolProjectionDraft::ToolPackageList(
            ToolPackageListProjection { packages: Vec::new() },
        )])
        .expect("commit Tool projection");
    assert!(updates.try_recv().is_err(), "a committed transaction is not implicitly published");

    coordinator
        .publish_tool_projection_changes(&committed)
        .expect("publish committed Tool projection");
    assert_eq!(updates.recv().expect("published Tool projection"), committed[0]);
}
