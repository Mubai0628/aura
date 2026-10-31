//! Working-directory claims. A program tool may run in a directory its arguments
//! name; that directory is resolved per call inside the workspace, never outside it.
use anyhow::{Result, ensure};
use aura_domain_common::WorkspacePath;
use aura_tool_contract::{
    CommandWorkingDirectoryV1, DeclarativeCommandV1, PackageDeclaredResourceClaimV1,
    PackageResourceClaimKeyV1, PackageWorkspacePathSelectorV1, ToolRuntimeDescriptorV1,
};
use aura_tool_domain::CanonicalToolPackageToolV1;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// The workspace-relative directory a tool's declared working directory names for
/// these arguments, or `None` when it does not name a claim.
pub fn working_directory_claim(
    tool: &CanonicalToolPackageToolV1,
    arguments: &Value,
) -> Result<Option<(PackageResourceClaimKeyV1, WorkspacePath)>> {
    let directory = match tool.runtime.to_runtime_descriptor_v1() {
        ToolRuntimeDescriptorV1::AuraStdio { working_directory, .. } => working_directory,
        ToolRuntimeDescriptorV1::DeclarativeCommand { command } => match command {
            DeclarativeCommandV1::Argv { working_directory, .. }
            | DeclarativeCommandV1::PowershellScript { working_directory, .. } => working_directory,
        },
        _ => return Ok(None),
    };
    let CommandWorkingDirectoryV1::ResolvedDirectoryClaim { claim_key } = directory else {
        return Ok(None);
    };
    let claim = tool
        .resource_claims
        .iter()
        .find(|claim| claim.claim_key() == &claim_key)
        .ok_or_else(|| anyhow::anyhow!("package.directory_claim_missing"))?;
    let PackageDeclaredResourceClaimV1::WorkspacePath { selector, .. } = claim else {
        anyhow::bail!("package.directory_claim_unsupported: 工作目录只能是工作区内的目录");
    };
    let path = match selector {
        PackageWorkspacePathSelectorV1::WorkspaceRoot { .. } => WorkspacePath::root(),
        PackageWorkspacePathSelectorV1::ArgumentJsonPointer { pointer, .. } => {
            let value = aura_tool_runtime::optional_pointer(arguments, pointer)
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("package.directory_argument_missing"))?;
            WorkspacePath::parse(value)
                .map_err(|_| anyhow::anyhow!("package.directory_outside_workspace"))?
        }
    };
    Ok(Some((claim_key, path)))
}

/// Conformance only: the run's own empty scratch workspace may gain the directory a
/// case names. The user's workspace is never touched by Conformance.
pub fn scratch_directory_claims(
    tool: &CanonicalToolPackageToolV1,
    arguments: &Value,
    scratch_workspace: Option<&Path>,
) -> Result<BTreeMap<PackageResourceClaimKeyV1, PathBuf>> {
    let Some((key, relative)) = working_directory_claim(tool, arguments)? else {
        return Ok(BTreeMap::new());
    };
    let root = scratch_workspace.ok_or_else(|| anyhow::anyhow!("package.workspace_required"))?;
    let path = if relative.is_root() { root.to_owned() } else { root.join(relative.as_str()) };
    ensure!(path.starts_with(root), "package.directory_outside_workspace");
    std::fs::create_dir_all(&path)?;
    Ok([(key, path)].into())
}
