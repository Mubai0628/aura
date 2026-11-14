//! The workspace runtime this service owns, sized by the Workspace Files settings.
use super::*;

/// Opens the workspace runtime. Its buffer store publishes real commits to the process's activity
/// index, and the lifecycle operations that hit a failure are recorded in Core's own log.
pub(super) fn open_workspace_runtime(
    persistence: &Arc<PersistenceCoordinator>,
    config: &WorkspaceFilesConfig,
    activity_index: aura_activity_runtime::ActivityIndex,
    activity_observer: aura_activity_runtime::ActivityObserverHandle,
    runtime_instance_id: aura_domain_common::RuntimeInstanceId,
) -> Result<Arc<WorkspaceRuntime>> {
    aura_workspace::route_lifecycle_log(append_core_log);
    WorkspaceRuntime::open_with_activity_publisher(
        Arc::clone(persistence),
        WorkspaceRuntimeOptions {
            catalog: CatalogOptions {
                max_partition_files: config.catalog.max_partition_files,
                max_partition_text_bytes: config.catalog.max_partition_text_bytes,
                max_index_file_bytes: config.catalog.max_index_file_bytes,
            },
            watcher_coalesce: Duration::from_millis(config.watcher.coalesce_milliseconds),
            watcher_poll_interval: Duration::from_millis(config.watcher.poll_interval_milliseconds),
            watcher_rescan_attempts: config.watcher.rescan_attempts,
            ..WorkspaceRuntimeOptions::default()
        },
        // The buffer store publishes its **real commits** through this hook: once
        // before its write window opens and once when that window really settled or failed.
        // The hook is a plain closure over the process's one index, so the store gains no
        // dependency on the information module and a broken index still cannot affect a save:
        // the return value is discarded and the closure runs outside every store gate.
        Some(Arc::new(move |fact: aura_workspace::BufferCommitFact<'_>| {
            aura_core_activity::agent_activity::publish_buffer_commit_activity(
                &fact,
                runtime_instance_id,
                &activity_index,
                &activity_observer,
            );
        })),
    )
    .context("workspace.runtime_unavailable: 工作区服务没能启动，工作区与文件工具暂时不能用；请重启 Aura，仍然如此请到设置里查看恢复选项。")
}
