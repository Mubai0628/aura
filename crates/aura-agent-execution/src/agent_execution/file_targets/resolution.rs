use super::binding;
use super::*;
use aura_security_contract::FileConsentProtection;
use aura_tool_registry::agent_release::AgentBuiltinHandler as H;

impl AgentExecution {
    pub(in crate::agent_execution) fn resolve_file_target(
        &self,
        handler: H,
        action: &mut serde_json::Value,
        capabilities: &mut Vec<ResolvedCapability>,
        batch: &ToolBatchRequestV1,
        snapshot: &PermissionSnapshot,
    ) -> anyhow::Result<Vec<ResourceDescriptor>> {
        let id = &batch.invocation_identity;
        let workspace = snapshot.workspace_id;
        let resolver = self.workspace.resolver(workspace)?;
        let field = if handler == H::FileSearch { "root" } else { "path" };
        // Search and find look in the workspace root when the model names no directory. The
        // default is written into the action, so the operation, its approval and its
        // fingerprint name the directory they resolve, exactly as if the model had sent ".".
        if matches!(handler, H::FileSearch | H::FileFind)
            && let Some(object) = action.as_object_mut()
        {
            object.entry(field).or_insert_with(|| WorkspacePath::ROOT.into());
        }
        let original = action
            .get(field)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("agent.file_path_missing"))?
            .to_owned();
        let supplied = std::path::Path::new(&original);
        let absolute = if supplied.is_absolute() {
            supplied.to_path_buf()
        } else {
            resolver.canonical_root().join(supplied)
        };
        let native = match aura_windows_platform::inspect_existing_path(&absolute, true) {
            Ok(info) => Some(info),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error)
                if error.kind() == std::io::ErrorKind::PermissionDenied
                    && handler == H::FileRead =>
            {
                return self.denied_workspace_read(action, &absolute, &resolver, snapshot);
            }
            Err(error) => return Err(error.into()),
        };
        // Only what exists can be deleted.
        anyhow::ensure!(
            handler != H::FileDelete || native.is_some(),
            "fs.delete.not_found: 找不到这个文件或文件夹。"
        );
        let final_path = native.as_ref().map_or_else(|| absolute.clone(), |i| i.final_path.clone());
        let mut class = super::super::protected_paths::classify(&final_path);
        // Aura's own data asks every time.
        if class == super::super::protected_paths::ExternalPathClass::Ordinary
            && self.control_resources.in_aura_data(&final_path)
        {
            class = super::super::protected_paths::ExternalPathClass::SensitiveUser;
        }
        let mutating = !matches!(handler, H::FileRead | H::FileList | H::FileFind | H::FileSearch);
        let control = native.as_ref().map_or_else(
            || self.control_resources.contains_missing(&final_path),
            |info| self.control_resources.overlaps(info, mutating),
        );
        if control {
            class = super::super::protected_paths::ExternalPathClass::System;
            if mutating {
                capabilities.push(ResolvedCapability::ConfigurationWrite);
            }
        }
        let root = aura_windows_platform::inspect_existing_path(resolver.canonical_root(), true)?;
        let native_inside =
            native.as_ref().is_none_or(|info| super::native_scope::within(&root, info));
        let fields = if handler == H::FileSearch {
            path_input::ROOT_FIELDS
        } else {
            path_input::PATH_FIELDS
        };
        let normalized =
            path_input::normalize_action_paths(action, fields, resolver.canonical_root());
        if normalized.is_ok()
            && native_inside
            && class == super::super::protected_paths::ExternalPathClass::Ordinary
        {
            let relative: aura_workspace_domain::AgentWorkspacePath =
                serde_json::from_value(action[field].clone())?;
            let relative = relative.into_relative();
            let facts = aura_workspace::ResourceFacts::root(resolver.clone(), id.session_id);
            let resource = match facts.object(&relative)? {
                Some(r) => r,
                None => facts.slot(&relative)?,
            };
            if let Some(native) = &native {
                let expected = aura_workspace::domain_identity(&native.identity);
                anyhow::ensure!(
                    matches!(resource.identity_of(), aura_workspace_domain::ResourceIdentity::Object(id)
                        | aura_workspace_domain::ResourceIdentity::Subtree(id) if id == expected),
                    "agent.normalized_target_replaced"
                );
            }
            let mut target = binding::target(
                &resource,
                final_path.display().to_string(),
                native.as_ref().is_some_and(|info| info.is_directory),
                FileConsentProtection::Ordinary,
            )?;
            if relative.as_str() != WorkspacePath::ROOT {
                target.entry_key = binding::target(
                    &facts.slot(&relative)?,
                    String::new(),
                    false,
                    FileConsentProtection::Ordinary,
                )?
                .entry_key;
            }
            if let Some(object) = action.as_object_mut() {
                path_input::normalize_cursor_paths(
                    object,
                    "cursor",
                    &["directory", "after_path"],
                    resolver.canonical_root(),
                )?;
            }
            return Ok(vec![ResourceDescriptor::WorkspacePath {
                workspace_id: workspace,
                canonical_relative_path: relative,
                file_target: Some(target),
            }]);
        }
        if let Err(error) = normalized.as_ref() {
            anyhow::ensure!(path_input::outside_workspace_field(error).is_some(), "{error}");
        }
        anyhow::ensure!(supplied.is_absolute() || native.is_some(), "agent.path_outside_workspace");
        for capability in capabilities.iter_mut() {
            *capability = external_capability(*capability);
        }
        if !control
            && let Some(covered) = self.external_grants.grant_for_capabilities(
                id.session_id,
                workspace,
                &final_path,
                Some(id.turn_task_id),
                CorrelationId::new(),
                capabilities,
                matches!(handler, H::FileList | H::FileFind | H::FileSearch),
            )?
            && !super::super::protected_paths::escapes_recursive_grant(
                std::path::Path::new(&covered.root_display),
                &final_path,
            )
            && !self
                .control_resources
                .grant_escapes_into_data(std::path::Path::new(&covered.root_display), &final_path)
            && self.external_grants.covers_capabilities(
                covered.grant_id,
                id.session_id,
                capabilities,
            )
        {
            action[field] = serde_json::json!(covered.relative_path.as_str());
            return Ok(vec![ResourceDescriptor::ExternalGrant { grant_id: covered.grant_id }]);
        }
        let (path, parent_identity) = if native.is_some() {
            (final_path, None)
        } else {
            let parent =
                absolute.parent().ok_or_else(|| anyhow::anyhow!("agent.file_parent_missing"))?;
            let info = aura_windows_platform::inspect_existing_path(parent, true)?;
            anyhow::ensure!(info.is_directory, "agent.file_parent_not_directory");
            let leaf =
                absolute.file_name().ok_or_else(|| anyhow::anyhow!("agent.file_leaf_missing"))?;
            (info.final_path.join(leaf), Some(aura_workspace::domain_identity(&info.identity)))
        };
        let directory = native.as_ref().is_some_and(|info| info.is_directory);
        let target = binding::external(
            &path,
            native.as_ref(),
            workspace,
            id.session_id,
            directory,
            if control {
                FileConsentProtection::ControlAuthority
            } else {
                match class {
                    super::super::protected_paths::ExternalPathClass::Ordinary => {
                        FileConsentProtection::Ordinary
                    }
                    super::super::protected_paths::ExternalPathClass::System => {
                        FileConsentProtection::System
                    }
                    super::super::protected_paths::ExternalPathClass::SensitiveUser => {
                        FileConsentProtection::SensitiveUser
                    }
                }
            },
        )?;
        let display = path.display().to_string();
        let canonical_path = CanonicalPath { utf16: display.encode_utf16().collect(), display };
        let identity = native.as_ref().map(|i| aura_workspace::domain_identity(&i.identity));
        action[field] = serde_json::json!(
            path.file_name().map_or_else(|| ".".into(), |n| n.to_string_lossy().into_owned())
        );
        let file_target = Some(target);
        Ok(vec![match class {
            super::super::protected_paths::ExternalPathClass::Ordinary => {
                ResourceDescriptor::ExternalPath {
                    canonical_path,
                    identity,
                    parent_identity,
                    directory,
                    recursive: directory,
                    file_target,
                }
            }
            super::super::protected_paths::ExternalPathClass::System => {
                ResourceDescriptor::SystemPath {
                    canonical_path,
                    identity,
                    parent_identity,
                    file_target,
                }
            }
            super::super::protected_paths::ExternalPathClass::SensitiveUser => {
                ResourceDescriptor::SensitiveUserPath {
                    canonical_path,
                    identity,
                    parent_identity,
                    file_target,
                }
            }
        }])
    }
}

impl AgentExecution {
    /// An inaccessible read has no verified target and cannot mint reusable consent.
    /// Admit only its normal refusal, under a verified workspace parent/entry. Fresh
    /// source verification rejects any replacement or newly accessible target before I/O.
    fn denied_workspace_read(
        &self,
        action: &mut serde_json::Value,
        absolute: &std::path::Path,
        resolver: &std::sync::Arc<aura_workspace::SecurePathResolver>,
        snapshot: &PermissionSnapshot,
    ) -> anyhow::Result<Vec<ResourceDescriptor>> {
        anyhow::ensure!(
            super::super::protected_paths::classify(absolute)
                == super::super::protected_paths::ExternalPathClass::Ordinary
                && !self.control_resources.contains_missing(absolute)
                && !self.control_resources.in_aura_data(absolute),
            "agent.target_identity_unavailable",
        );
        resolver.verify_root_identity()?;
        path_input::normalize_action_paths(
            action,
            path_input::PATH_FIELDS,
            resolver.canonical_root(),
        )?;
        let relative: aura_workspace_domain::AgentWorkspacePath =
            serde_json::from_value(action["path"].clone())?;
        let relative = relative.into_relative();
        // The secure resolver checks this actual parent is inside the workspace.
        aura_workspace::ResourceFacts::root(resolver.clone(), snapshot.session_id)
            .slot(&relative)?;
        Ok(vec![ResourceDescriptor::WorkspacePath {
            workspace_id: snapshot.workspace_id,
            canonical_relative_path: relative,
            file_target: None,
        }])
    }
}

fn external_capability(c: ResolvedCapability) -> ResolvedCapability {
    use ResolvedCapability::*;
    match c {
        WorkspaceFileRead => ExternalFileRead,
        WorkspaceFileWrite => ExternalFileWrite,
        WorkspaceFileCreate => ExternalFileCreate,
        WorkspaceFileDelete => ExternalFileDelete,
        other => other,
    }
}
