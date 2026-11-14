use super::lines;
use crate::FsToolError;
use aura_domain_common::{FileIdentity, WorkspacePath};
use aura_windows_platform::{NativeFileAccess, VerifiedFile};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadTarget {
    pub path: PathBuf,
    pub parent_identity: FileIdentity,
    pub identity: Option<FileIdentity>,
    pub blake3: Option<String>,
    pub workspace_path: Option<WorkspacePath>,
    /// Folders below the one `parent_identity` names that do not exist yet, outermost first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_folders: Vec<String>,
}

pub fn inspect_download_target(
    path: &Path,
    workspace_path: Option<WorkspacePath>,
    overwrite: bool,
) -> Result<DownloadTarget, FsToolError> {
    let parent_path = path.parent().ok_or_else(lines::no_parent)?;
    let leaf = path.file_name().ok_or_else(lines::no_leaf)?;
    let parent = match aura_windows_platform::open_verified_publish_parent(parent_path) {
        Ok(parent) => parent,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return super::folders::pending(path, workspace_path, overwrite);
        }
        Err(error) => return Err(lines::unusable(parent_path, &error)),
    };
    let (identity, blake3) =
        match aura_windows_platform::open_verified_existing(path, NativeFileAccess::Read) {
            Ok(mut file) => {
                if !overwrite {
                    return Err(lines::exists(path));
                }
                if file.info().is_directory
                    || !aura_windows_platform::final_path_within(parent.info(), file.info())
                {
                    return Err(lines::not_file(path));
                }
                let mut hash = blake3::Hasher::new();
                let mut buffer = [0u8; 64 * 1024];
                loop {
                    let n = file
                        .file_mut()
                        .read(&mut buffer)
                        .map_err(|error| lines::unusable(path, &error))?;
                    if n == 0 {
                        break;
                    }
                    hash.update(&buffer[..n]);
                }
                (
                    Some(aura_workspace::domain_identity(&file.info().identity)),
                    Some(hash.finalize().to_hex().to_string()),
                )
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (None, None),
            Err(error) => return Err(lines::unusable(path, &error)),
        };
    Ok(DownloadTarget {
        path: parent.info().final_path.join(leaf),
        parent_identity: aura_workspace::domain_identity(&parent.info().identity),
        identity,
        blake3,
        workspace_path,
        missing_folders: Vec::new(),
    })
}

impl DownloadTarget {
    /// The folder `parent_identity` names: the file's own, or the last one on its way that
    /// existed when the target was checked.
    pub(super) fn parent(&self) -> Result<VerifiedFile, FsToolError> {
        let mut folder = self.path.parent().ok_or_else(lines::no_parent)?;
        for _ in &self.missing_folders {
            folder = folder.parent().ok_or_else(lines::no_parent)?;
        }
        let parent = aura_windows_platform::open_verified_publish_parent(folder)
            .map_err(|error| lines::unusable(folder, &error))?;
        if aura_workspace::domain_identity(&parent.info().identity) != self.parent_identity {
            return Err(lines::parent_changed());
        }
        Ok(parent)
    }
    pub fn verify(&self) -> Result<(), FsToolError> {
        let current = inspect_download_target(
            &self.path,
            self.workspace_path.clone(),
            self.identity.is_some(),
        )?;
        if current != *self && !current.anchored_deeper_than(self) {
            return Err(lines::target_changed(&self.path));
        }
        Ok(())
    }

    /// Whether this is `checked` with folders it still needed made since (by another call,
    /// typically): the same absent file at the same place, anchored at a deeper folder of its way.
    #[must_use]
    pub fn anchored_deeper_than(&self, checked: &DownloadTarget) -> bool {
        self.path == checked.path
            && self.workspace_path.is_some()
            && self.workspace_path == checked.workspace_path
            && self.identity.is_none()
            && checked.identity.is_none()
            && self.blake3.is_none()
            && checked.blake3.is_none()
            && self.missing_folders.len() < checked.missing_folders.len()
            && checked.missing_folders.ends_with(&self.missing_folders)
    }
}
pub(super) fn invalid(value: impl std::fmt::Display) -> FsToolError {
    FsToolError::InvalidRequest(value.to_string())
}
