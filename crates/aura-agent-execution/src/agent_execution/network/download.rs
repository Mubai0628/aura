use super::super::*;
use anyhow::{Result, ensure};
use aura_fs_tools::download::DownloadTarget;
use aura_tool_registry::agent_release::AgentBuiltinHandler as H;

pub(crate) fn download_target(content: &OperationContent) -> Result<DownloadTarget> {
    Ok(serde_json::from_value(content.action["download_target"].clone())?)
}

/// A download whose folder does not exist, outside the workspace: the folder is named, with
/// what to do. Inside the workspace missing folders are made, so the target resolves.
fn missing_folder(absolute: &std::path::Path) -> Option<anyhow::Error> {
    let folder = absolute.parent()?;
    match std::fs::symlink_metadata(folder) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Some(anyhow::anyhow!(aura_fs_tools::download::missing_folder_line(folder)))
        }
        _ => None,
    }
}

/// Core's own line about a download's target, its folders or its temporary file, which the model
/// reads as it is. The file tools wrap such a line in their own prefix; this finds it again.
pub(crate) fn download_folder_feedback(error: &anyhow::Error) -> Option<String> {
    error.chain().find_map(|cause| {
        let line = match cause.downcast_ref::<aura_fs_tools::FsToolError>() {
            Some(aura_fs_tools::FsToolError::InvalidRequest(line)) => line.clone(),
            Some(_) => return None,
            None => cause.to_string(),
        };
        aura_fs_tools::download::is_core_line(&line).then_some(line)
    })
}

/// A failure to check or start a download, worded for the model: Core's own line as it is, else
/// a sentence over the typed error, which stays the source for code that tells errors apart.
pub(crate) fn download_folder_error(error: aura_fs_tools::FsToolError) -> anyhow::Error {
    use aura_fs_tools::FsToolError as F;
    match error {
        F::InvalidRequest(line) if aura_fs_tools::download::is_core_line(&line) => {
            anyhow::anyhow!(line)
        }
        busy @ F::ResourceBusy(_) => anyhow::Error::new(busy).context(
            "http.target_busy: 下载位置正被另一个还在进行的调用使用，文件没有下载。等那个调用结束后再下载。",
        ),
        other => {
            let line = format!(
                "http.download_not_started: Aura 没能准备这次下载（{}），文件没有下载。可以稍后重试。",
                detail(&other)
            );
            anyhow::Error::new(other).context(line)
        }
    }
}

/// A system's own words after Core's sentence: the first line, at most 160 bytes, without
/// control characters or braces.
fn detail(error: &dyn std::fmt::Display) -> String {
    let text = error.to_string();
    let mut detail = String::new();
    for c in text.lines().next().unwrap_or_default().trim().chars() {
        if detail.len() + c.len_utf8() > 160 {
            detail.push('…');
            break;
        }
        detail.push(if c.is_control() || c == '{' || c == '}' { ' ' } else { c });
    }
    detail
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
        let creating = match std::fs::symlink_metadata(&absolute) {
            Ok(_) => false,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
            // The file tools' own error, as the target check's are: `download_folder_feedback`
            // finds its line for the model.
            Err(e) => return Err(aura_fs_tools::download::target_unusable(&absolute, &e).into()),
        };
        *capabilities = vec![if creating {
            ResolvedCapability::WorkspaceFileCreate
        } else {
            ResolvedCapability::WorkspaceFileWrite
        }];
        let mut resources = self
            .resolve_file_target(H::FileWrite, action, capabilities, batch, snapshot)
            .map_err(|error| missing_folder(&absolute).unwrap_or(error))?;
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
            ensure!(
                bound.identity_key == expected,
                "http.resolved_target_changed: 下载位置在检查期间变了（文件被新建、替换或删除），文件没有下载。可以重新调用。"
            );
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
                    let relative = WorkspacePath::parse(action["path"].as_str().ok_or_else(|| {
                        anyhow::anyhow!(
                            "http.path_required: 没有给出下载位置，文件没有下载。请在 path 里写出要保存到的文件路径。"
                        )
                    })?)?;
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
        anyhow::bail!(
            "http.target_authority_missing: Aura 没能为这个下载位置确定可以写入的地方，文件没有下载。请换一个路径，例如工作区里的路径。"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{download_folder_error, download_folder_feedback, missing_folder};

    #[test]
    fn a_missing_folder_outside_the_workspace_reaches_the_model_as_core_wrote_it() {
        let outside = tempfile::tempdir().expect("outside");
        let error = missing_folder(&outside.path().join("missing/pkg.zip")).expect("named");
        let line = download_folder_feedback(&error.context("resolution")).expect("feedback");
        assert!(line.starts_with("http.download_folder_missing:"), "{line}");
        assert!(line.contains("missing") && line.contains("先建好"), "{line}");
        assert!(missing_folder(&outside.path().join("pkg.zip")).is_none(), "the folder exists");
        let begin = download_folder_error(aura_fs_tools::FsToolError::InvalidRequest(
            "http.download_folder_unusable: 工作区里的 a/b 用不了".into(),
        ));
        assert!(begin.to_string().starts_with("http.download_folder_unusable:"));
        let other = anyhow::Error::from(aura_fs_tools::FsToolError::InvalidRequest(
            "http.target_exists".into(),
        ));
        assert_eq!(download_folder_feedback(&other), None, "a bare code is not Core's line");
    }

    /// A file that is already there is named with how to replace it, at resolution and when the
    /// download is checked again before it starts.
    #[test]
    fn an_existing_target_reaches_the_model_with_how_to_replace_it() {
        let folder = tempfile::tempdir().expect("folder");
        let file = folder.path().join("pkg.zip");
        std::fs::write(&file, b"old").expect("existing file");
        let exists = || {
            aura_fs_tools::download::inspect_download_target(&file, None, false)
                .expect_err("the file exists")
        };
        let resolved = anyhow::Error::from(exists()).context("resolution");
        let line = download_folder_feedback(&resolved).expect("feedback");
        assert!(line.starts_with("http.target_exists: ") && line.contains("pkg.zip"), "{line}");
        assert!(line.contains("overwrite: true"), "{line}");
        let started = download_folder_error(exists()).to_string();
        assert!(started.starts_with("http.target_exists: "), "{started}");
        let unprepared = download_folder_error(aura_fs_tools::FsToolError::ScopeUnavailable);
        assert!(unprepared.to_string().starts_with("http.download_not_started: "), "{unprepared}");
        assert!(unprepared.downcast_ref::<aura_fs_tools::FsToolError>().is_some(), "typed source");
    }
}
