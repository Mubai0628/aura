use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use aura_domain_common::{FileIdentity, WorkspaceId, WorkspacePath};
use aura_windows_platform::{
    NativeFileAccess, NativeFileIdentity, NativePathInfo, SearchSource, VerifiedFile,
    final_path_within, inspect_existing_path, open_verified_existing, open_verified_search_source,
};
use aura_workspace_domain::{WorkspaceState, WorkspaceSummary};

use crate::error::WorkspaceError;

pub struct ResolvedWorkspacePath {
    pub workspace_id: WorkspaceId,
    pub path: WorkspacePath,
    pub absolute_path: PathBuf,
    pub info: NativePathInfo,
}

pub struct VerifiedWorkspaceFile {
    pub workspace_id: WorkspaceId,
    pub path: WorkspacePath,
    pub file: VerifiedFile,
}

pub struct ValidatedParent {
    pub workspace_id: WorkspaceId,
    pub target: WorkspacePath,
    pub parent_path: WorkspacePath,
    pub parent_absolute_path: PathBuf,
    pub leaf_name: OsString,
    pub parent: VerifiedFile,
}

#[derive(Clone)]
pub struct SecurePathResolver {
    workspace_id: WorkspaceId,
    root: NativePathInfo,
}

impl SecurePathResolver {
    pub fn from_workspace(workspace: &WorkspaceSummary) -> Result<Self, WorkspaceError> {
        match workspace.state {
            WorkspaceState::Online | WorkspaceState::ReconciliationRequired => {}
            WorkspaceState::Offline => return Err(WorkspaceError::Offline),
            WorkspaceState::Archived => return Err(WorkspaceError::Archived),
        }
        let canonical_root = PathBuf::from(&workspace.canonical_root);
        let root = inspect_existing_path(&canonical_root, true).map_err(|source| {
            WorkspaceError::PathResolutionFailed { path: canonical_root.clone(), source }
        })?;
        if !root.is_directory {
            return Err(WorkspaceError::InvalidRoot);
        }
        if let Some(expected) = workspace.root_identity.as_ref()
            && !identity_matches(expected, &root.identity)
        {
            return Err(WorkspaceError::RootIdentityChanged);
        }
        Ok(Self { workspace_id: workspace.workspace_id, root })
    }

    pub fn inspect_selected_root(path: &Path) -> Result<NativePathInfo, WorkspaceError> {
        if !path.is_absolute() {
            return Err(WorkspaceError::InvalidRoot);
        }
        let root = inspect_existing_path(path, true).map_err(|source| {
            WorkspaceError::PathResolutionFailed { path: path.to_path_buf(), source }
        })?;
        if !root.is_directory {
            return Err(WorkspaceError::InvalidRoot);
        }
        Ok(root)
    }

    #[must_use]
    pub fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }

    #[must_use]
    pub fn canonical_root(&self) -> &Path {
        &self.root.final_path
    }

    #[must_use]
    pub fn root_identity(&self) -> FileIdentity {
        domain_identity(&self.root.identity)
    }

    pub fn verify_root_identity(&self) -> Result<(), WorkspaceError> {
        let current = inspect_existing_path(&self.root.final_path, true).map_err(|source| {
            WorkspaceError::PathResolutionFailed { path: self.root.final_path.clone(), source }
        })?;
        if current.identity != self.root.identity {
            return Err(WorkspaceError::RootIdentityChanged);
        }
        Ok(())
    }

    pub fn open_existing(
        &self,
        path: &WorkspacePath,
        access: NativeFileAccess,
    ) -> Result<VerifiedWorkspaceFile, WorkspaceError> {
        self.verify_root_identity()?;
        let absolute = self.absolute_path(path);
        let file = open_verified_existing(&absolute, access).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                WorkspaceError::NotFound { path: path.clone() }
            } else {
                WorkspaceError::PathResolutionFailed { path: absolute.clone(), source }
            }
        })?;
        if !final_path_within(&self.root, file.info()) {
            return Err(WorkspaceError::ScopeUnavailable { path: path.clone() });
        }
        Ok(VerifiedWorkspaceFile { workspace_id: self.workspace_id, path: path.clone(), file })
    }

    pub fn open_search_source(&self, path: &WorkspacePath) -> Result<SearchSource, WorkspaceError> {
        self.verify_root_identity()?;
        let absolute = self.absolute_path(path);
        let file = open_verified_search_source(&absolute).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                WorkspaceError::NotFound { path: path.clone() }
            } else {
                WorkspaceError::PathResolutionFailed { path: absolute.clone(), source }
            }
        })?;
        if !final_path_within(&self.root, file.info()) {
            return Err(WorkspaceError::ScopeUnavailable { path: path.clone() });
        }
        Ok(file)
    }

    pub fn open_absolute_existing(
        &self,
        absolute: &Path,
        access: NativeFileAccess,
    ) -> Result<VerifiedFile, WorkspaceError> {
        self.verify_root_identity()?;
        let file = open_verified_existing(absolute, access).map_err(|source| {
            WorkspaceError::PathResolutionFailed { path: absolute.to_path_buf(), source }
        })?;
        if !final_path_within(&self.root, file.info()) {
            return Err(WorkspaceError::ScopeUnavailable {
                path: WorkspacePath::parse(WorkspacePath::ROOT).expect("root path is valid"),
            });
        }
        Ok(file)
    }

    pub fn resolve_existing(
        &self,
        path: &WorkspacePath,
    ) -> Result<ResolvedWorkspacePath, WorkspaceError> {
        let verified = self.open_existing(path, NativeFileAccess::Metadata)?;
        Ok(ResolvedWorkspacePath {
            workspace_id: self.workspace_id,
            path: path.clone(),
            absolute_path: verified.file.info().final_path.clone(),
            info: verified.file.info().clone(),
        })
    }

    pub fn validate_parent(
        &self,
        target: &WorkspacePath,
    ) -> Result<ValidatedParent, WorkspaceError> {
        if target.is_root() {
            return Err(WorkspaceError::WrongKind { path: target.clone() });
        }
        let mut components = target.as_str().split('/').collect::<Vec<_>>();
        let leaf =
            components.pop().ok_or_else(|| WorkspaceError::WrongKind { path: target.clone() })?;
        let parent_path = if components.is_empty() {
            WorkspacePath::parse(WorkspacePath::ROOT).expect("root path is valid")
        } else {
            WorkspacePath::parse(components.join("/")).expect("target parent was validated")
        };
        let parent = self.open_existing(&parent_path, NativeFileAccess::Metadata)?.file;
        if !parent.info().is_directory {
            return Err(WorkspaceError::WrongKind { path: parent_path });
        }
        Ok(ValidatedParent {
            workspace_id: self.workspace_id,
            target: target.clone(),
            parent_path,
            parent_absolute_path: parent.info().final_path.clone(),
            leaf_name: OsString::from(leaf),
            parent,
        })
    }

    pub fn verify_after_mutation(
        &self,
        target: &WorkspacePath,
    ) -> Result<ResolvedWorkspacePath, WorkspaceError> {
        self.resolve_existing(target)
    }

    #[must_use]
    pub fn absolute_path(&self, path: &WorkspacePath) -> PathBuf {
        if path.is_root() {
            return self.root.final_path.clone();
        }
        let mut absolute = self.root.final_path.clone();
        for component in path.as_str().split('/') {
            absolute.push(component);
        }
        absolute
    }
}

#[must_use]
pub fn domain_identity(identity: &NativeFileIdentity) -> FileIdentity {
    FileIdentity {
        volume_serial_number: identity.volume_serial_number.to_string(),
        file_id: identity.file_id.iter().map(|byte| format!("{byte:02x}")).collect(),
    }
}

fn identity_matches(expected: &FileIdentity, actual: &NativeFileIdentity) -> bool {
    expected.volume_serial_number == actual.volume_serial_number.to_string()
        && expected.file_id
            == actual.file_id.iter().map(|byte| format!("{byte:02x}")).collect::<String>()
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{Revision, WorkspaceId, WorkspacePath};
    use aura_workspace_domain::{CatalogFreshness, WorkspaceState, WorkspaceSummary};

    use super::{SecurePathResolver, domain_identity};

    fn resolver_for(root: &std::path::Path) -> SecurePathResolver {
        let info = SecurePathResolver::inspect_selected_root(root).expect("root info");
        let summary = WorkspaceSummary {
            workspace_id: WorkspaceId::new(),
            display_name: "test".to_owned(),
            selected_root: root.display().to_string(),
            canonical_root: info.final_path.display().to_string(),
            root_identity: Some(domain_identity(&info.identity)),
            state: WorkspaceState::Online,
            revision: Revision::INITIAL,
            catalog_freshness: CatalogFreshness::Unavailable,
            authorization_epoch: Revision::INITIAL,
        };
        SecurePathResolver::from_workspace(&summary).expect("resolver")
    }

    #[test]
    fn rejects_parent_traversal_before_joining_the_root() {
        assert!(aura_domain_common::WorkspacePath::parse("../escape.txt").is_err());
    }

    #[test]
    fn reads_existing_files_through_a_verified_handle() {
        let root = tempfile::tempdir().expect("temp root");
        std::fs::create_dir(root.path().join("dir")).expect("directory");
        std::fs::write(root.path().join("dir/file.txt"), b"Aura").expect("file");
        let resolver = resolver_for(root.path());
        let path = WorkspacePath::parse("dir/file.txt").expect("path");
        let file = resolver
            .open_existing(&path, aura_windows_platform::NativeFileAccess::Read)
            .expect("verified file");
        assert_eq!(file.file.file().metadata().expect("metadata").len(), 4);
    }

    #[test]
    fn follows_junctions_only_when_the_final_handle_remains_inside_the_root() {
        let root = tempfile::tempdir().expect("temp root");
        let outside = tempfile::tempdir().expect("outside root");
        std::fs::create_dir(root.path().join("inside-target")).expect("inside target");
        std::fs::write(root.path().join("inside-target/inside.txt"), b"inside")
            .expect("inside file");
        std::fs::write(outside.path().join("outside.txt"), b"outside").expect("outside file");
        junction::create(root.path().join("inside-target"), root.path().join("inside-link"))
            .expect("inside junction");
        junction::create(outside.path(), root.path().join("outside-link"))
            .expect("outside junction");

        let resolver = resolver_for(root.path());
        let inside = WorkspacePath::parse("inside-link/inside.txt").expect("inside path");
        resolver
            .open_existing(&inside, aura_windows_platform::NativeFileAccess::Read)
            .expect("inside junction remains in scope");

        let outside = WorkspacePath::parse("outside-link/outside.txt").expect("outside path");
        assert!(matches!(
            resolver.open_existing(&outside, aura_windows_platform::NativeFileAccess::Read),
            Err(crate::WorkspaceError::ScopeUnavailable { .. })
        ));
    }

    #[test]
    fn resolves_unicode_spaces_and_long_workspace_paths() {
        let root = tempfile::tempdir().expect("temp root");
        let mut relative = std::path::PathBuf::from("包含 空格");
        for index in 0..10 {
            relative.push(format!("segment-{index:02}-abcdefghijklmnop"));
        }
        std::fs::create_dir_all(root.path().join(&relative)).expect("long directory");
        relative.push("文件 名称.txt");
        std::fs::write(root.path().join(&relative), b"long path").expect("long file");
        let logical = WorkspacePath::parse(relative.to_string_lossy().replace('\\', "/"))
            .expect("logical path");
        let resolver = resolver_for(root.path());
        let file = resolver
            .open_existing(&logical, aura_windows_platform::NativeFileAccess::Read)
            .expect("verified long path");
        assert_eq!(file.file.file().metadata().expect("metadata").len(), 9);
    }
}
