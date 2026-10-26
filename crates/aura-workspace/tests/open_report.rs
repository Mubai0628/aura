//! Startup attribution: what opening the workspace runtime cost, by stage.
//!
//! A cold Core start with existing development data once exceeded the Main
//! connect budget with no way to tell the catalog load from the rebuild scan
//! from the watcher spawn. These cases pin the report's shape and its honesty:
//! counters describe the work the open actually did, and the stage spans add
//! up inside the total instead of overlapping or vanishing.
use std::sync::Arc;

use aura_domain_common::{CorrelationId, RuntimeInstanceId};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::{WorkspaceCreateCommand, WorkspaceCreateDisposition};

fn persistence(data: &tempfile::TempDir) -> Arc<PersistenceCoordinator> {
    match PersistenceCoordinator::start(PersistenceOptions::new(
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
    }
}

fn create_workspace(runtime: &WorkspaceRuntime, root: &std::path::Path) {
    let created = runtime
        .create(
            WorkspaceCreateCommand {
                selected_root: root.display().to_string(),
                display_name: "Open report".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("create workspace");
    assert_eq!(created.disposition, WorkspaceCreateDisposition::Created);
    runtime.wait_until_indexed(std::time::Duration::from_secs(60)).expect("the first walk");
}

#[test]
fn an_open_without_workspaces_reports_zero_installs() {
    let data = tempfile::tempdir().expect("data root");
    let runtime = WorkspaceRuntime::open(persistence(&data), WorkspaceRuntimeOptions::default())
        .expect("runtime open");
    let report = runtime.open_report().expect("the open that built it reports");
    assert_eq!(report.online_workspaces, 0);
    assert_eq!(report.catalog_entries, 0);
    assert_eq!(report.catalog_partitions, 0);
    assert_eq!(report.repaired_paths, 0);
    assert_eq!(report.catalog_load_ms, 0);
    assert_eq!(report.catalog_repair_ms, 0);
    assert_eq!(report.catalog_rebuild_ms, 0);
    assert_eq!(report.watcher_spawn_ms, 0);
    // The runtime itself was still opened: the total exists even when no stage ran.
    let _ = report.total_ms;
    // The report is the open's own fact; reading it again does not consume it.
    assert_eq!(runtime.open_report(), Some(report));
}

#[test]
fn a_cold_open_with_an_online_workspace_attributes_each_stage() {
    let data = tempfile::tempdir().expect("data root");
    let root = tempfile::tempdir().expect("workspace root");
    let persistence = persistence(&data);
    let files = 150;
    for index in 0..files {
        std::fs::write(root.path().join(format!("file_{index:03}.txt")), "cold start content")
            .expect("workspace file");
    }
    let runtime =
        WorkspaceRuntime::open(Arc::clone(&persistence), WorkspaceRuntimeOptions::default())
            .expect("first runtime");
    create_workspace(&runtime, root.path());
    drop(runtime);

    // The cold open: the store already holds the catalog, so the open must
    // load it, repair nothing, and rescan — each as its own number.
    let runtime =
        WorkspaceRuntime::open(Arc::clone(&persistence), WorkspaceRuntimeOptions::default())
            .expect("cold runtime open");
    let report = runtime.open_report().expect("the cold open reports");
    assert_eq!(report.online_workspaces, 1);
    assert!(
        report.catalog_entries >= files as u64,
        "the persisted catalog loaded at open: {} entries for {files} files",
        report.catalog_entries
    );
    assert_eq!(report.repaired_paths, 0, "no unproven text exists in this store");
    // The stage spans are sequential inside the open, so their sum can never
    // exceed the total; a report that violates this is measuring wrong.
    let staged = report
        .catalog_load_ms
        .saturating_add(report.catalog_repair_ms)
        .saturating_add(report.catalog_rebuild_ms)
        .saturating_add(report.watcher_spawn_ms);
    assert!(
        staged <= report.total_ms,
        "stage spans {staged} must fit inside the open total {} ms",
        report.total_ms
    );
}
