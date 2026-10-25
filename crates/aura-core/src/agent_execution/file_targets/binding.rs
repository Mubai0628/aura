use super::*;
use aura_security_contract::{
    FileConsentProtection, FileConsentTarget, file_entry_key, file_object_key,
};
use aura_workspace_domain::{FileResource, ResourceIdentity};

pub(super) fn target(
    resource: &FileResource,
    display: String,
    recursive: bool,
    protection: FileConsentProtection,
) -> anyhow::Result<FileConsentTarget> {
    let identity_key = match resource.identity_of() {
        ResourceIdentity::Object(id) | ResourceIdentity::Subtree(id) => file_object_key(&id),
        ResourceIdentity::Slot { parent_identity, leaf_name } => {
            file_entry_key(&parent_identity, &leaf_name)
        }
        ResourceIdentity::ProspectiveEntry { parent_identity, relative_path } => {
            file_entry_key(&parent_identity, &relative_path)
        }
    };
    let location =
        resource.location_of().ok_or_else(|| anyhow::anyhow!("agent.file_location_unverified"))?;
    let mut ancestors = vec![file_object_key(&location.anchor)];
    ancestors.extend(location.ancestry.iter().map(file_object_key));
    if let FileResource::Slot { parent, .. } | FileResource::ProspectiveEntry { parent, .. } =
        resource
    {
        ancestors.push(file_object_key(&parent.identity));
    }
    ancestors.dedup();
    let entry_key = match resource {
        FileResource::Slot { parent, leaf_name, .. } => {
            let key = parent
                .comparison_key(leaf_name)
                .ok_or_else(|| anyhow::anyhow!("agent.native_name_unverified"))?;
            file_entry_key(&parent.identity, &key)
        }
        FileResource::ProspectiveEntry { .. } => identity_key.clone(),
        _ => String::new(),
    };
    Ok(FileConsentTarget {
        canonical_path: display,
        recursive,
        identity_key,
        entry_key,
        ancestors,
        protection,
    })
}

pub(super) fn external(
    path: &std::path::Path,
    info: Option<&aura_windows_platform::NativePathInfo>,
    workspace: WorkspaceId,
    session: SessionId,
    recursive: bool,
    protection: FileConsentProtection,
) -> anyhow::Result<FileConsentTarget> {
    use aura_windows_platform::{NativeFileAccess, open_verified_existing};
    use aura_workspace_domain::ResourceOwner;
    let display = path.display().to_string();
    let Some(parent_path) = path.parent() else {
        let info = info.ok_or_else(|| anyhow::anyhow!("agent.file_root_unverified"))?;
        return Ok(FileConsentTarget {
            canonical_path: display,
            recursive,
            identity_key: file_object_key(&aura_workspace::domain_identity(&info.identity)),
            protection,
            ..Default::default()
        });
    };
    let parent = open_verified_existing(parent_path, NativeFileAccess::Metadata)?;
    let leaf = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("agent.file_leaf_unverified"))?
        .to_string_lossy();
    let owner = ResourceOwner {
        workspace_id: workspace,
        session_id: session,
        path: WorkspacePath::parse(leaf.as_ref())?,
    };
    let slot = aura_workspace::external_slot(&parent, &leaf, owner.clone())?;
    let mut bound = match info {
        Some(info) => target(
            &aura_workspace::external_object(
                &parent,
                aura_workspace::domain_identity(&info.identity),
                owner,
            )?,
            display,
            recursive,
            protection,
        )?,
        None => target(&slot, display, recursive, protection)?,
    };
    bound.entry_key = target(&slot, String::new(), false, protection)?.entry_key;
    Ok(bound)
}
