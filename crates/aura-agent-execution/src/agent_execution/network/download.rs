use super::super::*;
use anyhow::{Result, ensure};
use aura_fs_tools::download::DownloadTarget;
use aura_tool_registry::agent_release::AgentBuiltinHandler as H;

pub(crate) fn download_target(content: &OperationContent) -> Result<DownloadTarget> {
    Ok(serde_json::from_value(content.action["download_target"].clone())?)
}

impl AgentExecution {
    pub(in crate::agent_execution) fn resolve_download(
        &self,
        action: &mut serde_json::Value,
        capabilities: &mut Vec<ResolvedCapability>,
        batch: &ToolBatchRequestV1,
        snapshot: &PermissionSnapshot,
    ) -> Result<Vec<ResourceDescriptor>> {
        let original = action.clone();
        let input: aura_web_domain::HttpDownloadInput = serde_json::from_value(original.clone())?;
        let supplied = std::path::PathBuf::from(&input.path);
        let absolute = if supplied.is_absolute() {
            supplied
        } else {
            self.workspace.resolver(snapshot.workspace_id)?.canonical_root().join(supplied)
        };
        let creating = match std::fs::symlink_metadata(absolute) {
            Ok(_) => false,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
            Err(e) => return Err(e.into()),
        };
        *capabilities = vec![if creating {
            ResolvedCapability::WorkspaceFileCreate
        } else {
            ResolvedCapability::WorkspaceFileWrite
        }];
        let mut resources =
            self.resolve_file_target(H::FileWrite, action, capabilities, batch, snapshot)?;
        let (path, relative) = self.download_path(&resources, action, snapshot, creating)?;
        let target =
            aura_fs_tools::download::inspect_download_target(&path, relative, input.overwrite)?;
        if target.identity.is_none() {
            *action = original;
            *capabilities = vec![ResolvedCapability::WorkspaceFileCreate];
            resources =
                self.resolve_file_target(H::FileWrite, action, capabilities, batch, snapshot)?;
        }
        for bound in resources.iter().filter_map(file_consent_target_of) {
            let expected = target
                .identity
                .as_ref()
                .map(file_object_key)
                .unwrap_or_else(|| bound.entry_key.clone());
            ensure!(bound.identity_key == expected, "http.resolved_target_changed");
        }
        action["download_target"] = serde_json::to_value(target)?;
        capabilities.push(ResolvedCapability::OrdinaryNetwork);
        Ok(resources)
    }
    fn download_path(
        &self,
        resources: &[ResourceDescriptor],
        action: &serde_json::Value,
        snapshot: &PermissionSnapshot,
        creating: bool,
    ) -> Result<(std::path::PathBuf, Option<WorkspacePath>)> {
        for resource in resources {
            match resource {
                ResourceDescriptor::WorkspacePath {
                    workspace_id, canonical_relative_path, ..
                } => {
                    let resolver = self.workspace.resolver(*workspace_id)?;
                    resolver.verify_root_identity()?;
                    return Ok((
                        resolver.canonical_root().join(canonical_relative_path.as_str()),
                        Some(canonical_relative_path.clone()),
                    ));
                }
                ResourceDescriptor::ExternalPath { canonical_path, .. }
                | ResourceDescriptor::SystemPath { canonical_path, .. }
                | ResourceDescriptor::SensitiveUserPath { canonical_path, .. } => {
                    return Ok((canonical_path.display.clone().into(), None));
                }
                ResourceDescriptor::ExternalGrant { grant_id } => {
                    let relative = WorkspacePath::parse(
                        action["path"]
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("http.path_required"))?,
                    )?;
                    if creating {
                        let parent = self.external_grants.validate_create_parent(
                            *grant_id,
                            snapshot.session_id,
                            &relative,
                            CorrelationId::new(),
                        )?;
                        return Ok((parent.parent.info().final_path.join(parent.leaf_name), None));
                    }
                    let resolved = self.external_grants.resolve_existing(
                        *grant_id,
                        snapshot.session_id,
                        &relative,
                        aura_permission_domain::ExternalGrantCapability::Write,
                        aura_windows_platform::NativeFileAccess::Read,
                        CorrelationId::new(),
                    )?;
                    return Ok((resolved.file.info().final_path.clone(), None));
                }
                _ => (),
            }
        }
        anyhow::bail!("http.target_authority_missing")
    }
}
