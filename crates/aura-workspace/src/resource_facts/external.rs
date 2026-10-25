//! Complete external ancestry from verified native handles. Missing facts refuse admission.
use std::{io, path::Path};

use super::*;

pub fn external_ancestry(parent: &VerifiedFile) -> Result<ResourceLocation, WorkspaceError> {
    ancestry_with(parent, |directory| {
        aura_windows_platform::open_verified_existing(directory, NativeFileAccess::Metadata)
    })
}

fn ancestry_with(
    parent: &VerifiedFile,
    mut open: impl FnMut(&Path) -> io::Result<VerifiedFile>,
) -> Result<ResourceLocation, WorkspaceError> {
    let identity = domain_identity(&parent.info().identity);
    let mut chain = Vec::new();
    let mut current = parent.info().final_path.parent().map(Path::to_path_buf);
    while let Some(directory) = current {
        let opened = open(&directory).map_err(|error| {
            WorkspaceError::Watcher(format!(
                "file.resource_facts_unavailable: ancestor {} could not be verified: {error}",
                directory.display(),
            ))
        })?;
        if !opened.info().is_directory {
            return Err(WorkspaceError::Watcher(format!(
                "file.resource_facts_unavailable: ancestor {} is not a directory",
                directory.display(),
            )));
        }
        chain.push(domain_identity(&opened.info().identity));
        current = directory.parent().map(Path::to_path_buf);
    }
    let Some(anchor) = chain.pop() else {
        return Ok(ResourceLocation::at_anchor(identity));
    };
    chain.reverse();
    Ok(ResourceLocation { anchor, ancestry: chain })
}

pub fn external_slot(
    parent: &VerifiedFile,
    leaf_name: &str,
    owner: ResourceOwner,
) -> Result<FileResource, WorkspaceError> {
    let case_rule = match directory_case_rule(parent) {
        VolumeCaseRule::InsensitivePreserving => SlotCaseRule::InsensitivePreserving,
        VolumeCaseRule::Sensitive => SlotCaseRule::Sensitive,
        VolumeCaseRule::Unknown => SlotCaseRule::Unknown,
    };
    Ok(FileResource::Slot {
        parent: SlotParent {
            identity: domain_identity(&parent.info().identity),
            location: external_ancestry(parent)?,
            case_rule,
            name_comparison: aura_windows_platform::directory_name_comparison(parent),
        },
        leaf_name: leaf_name.to_owned(),
        owner,
    })
}

pub fn external_object(
    parent: &VerifiedFile,
    identity: FileIdentity,
    owner: ResourceOwner,
) -> Result<FileResource, WorkspaceError> {
    let mut location = external_ancestry(parent)?;
    let parent_id = domain_identity(&parent.info().identity);
    // The object's location includes its immediate parent. A slot carries that
    // parent separately, while an Object has no other place for this fact.
    if parent_id != location.anchor {
        location.ancestry.push(parent_id);
    }
    Ok(FileResource::Object { identity, location, owner })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_workspace_domain::{ResourceAccess, ResourceRequest, conflict_between};

    #[test]
    fn missing_external_ancestor_never_becomes_an_unrelated_target() {
        let root = tempfile::tempdir().expect("test root");
        let deep = root.path().join("one/two");
        std::fs::create_dir_all(&deep).expect("deep directory");
        let parent =
            aura_windows_platform::open_verified_existing(&deep, NativeFileAccess::Metadata)
                .expect("parent");
        let missing = std::fs::canonicalize(root.path().join("one")).expect("one");
        let result = ancestry_with(&parent, |path| {
            if path == missing {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "controlled ancestry failure"))
            } else {
                aura_windows_platform::open_verified_existing(path, NativeFileAccess::Metadata)
            }
        });
        assert!(result.is_err(), "a truncated chain must never acquire a file qualification");
        assert!(
            external_ancestry(&parent).is_ok(),
            "the valid control still derives all ancestors"
        );
    }

    #[test]
    fn external_object_intersects_its_immediate_parent_scope() {
        let root = tempfile::tempdir().expect("root");
        let path = root.path().join("note.txt");
        std::fs::write(&path, b"data").expect("file");
        let parent =
            aura_windows_platform::open_verified_existing(root.path(), NativeFileAccess::Metadata)
                .expect("parent");
        let file = aura_windows_platform::open_verified_existing(&path, NativeFileAccess::Metadata)
            .expect("file");
        let owner = ResourceOwner {
            workspace_id: WorkspaceId::new(),
            session_id: SessionId::new(),
            path: WorkspacePath::parse("note.txt").expect("path"),
        };
        let object =
            external_object(&parent, domain_identity(&file.info().identity), owner.clone())
                .expect("object facts");
        let subtree = FileResource::Subtree {
            directory_identity: domain_identity(&parent.info().identity),
            location: external_ancestry(&parent).expect("complete ancestry"),
            owner,
        };
        assert!(
            conflict_between(
                &ResourceRequest::new(object, ResourceAccess::Write),
                &ResourceRequest::new(subtree, ResourceAccess::SubtreeWrite)
            )
            .is_some()
        );
    }
}
