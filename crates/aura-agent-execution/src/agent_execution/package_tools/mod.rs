//! Package tools in the V2 Agent: installed, trusted and enabled package tools
//! become Registry candidates, exact operations and executions. HTTP tools go
//! through Core's controlled HTTP; programs run in the user's Job.
mod candidates;
pub(crate) mod http;
mod operation;
pub mod program;
mod registry;
mod run;
pub(crate) use candidates::{CHANGED_DURING_CALL, changed_during_call, installed_tool_for};
pub use candidates::{InstalledPackageTool, installed_tools};
pub(super) use operation::resources;
pub use registry::{SnapshotPackageTools, named_by, store};

/// Where package program calls materialize their content: the package owner's own
/// namespace, whose startup recovery removes anything a crash left behind.
pub fn scratch_root(service: &super::AgentExecution) -> anyhow::Result<std::path::PathBuf> {
    let database = service.journal.database_path()?;
    let data = database
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or_else(|| anyhow::anyhow!("package.data_root_unavailable"))?;
    Ok(data.join("v2-packages"))
}
