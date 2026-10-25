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
}

pub fn inspect_download_target(
    path: &Path,
    workspace_path: Option<WorkspacePath>,
    overwrite: bool,
) -> Result<DownloadTarget, FsToolError> {
    let parent_path = path.parent().ok_or_else(|| invalid("http.target_parent"))?;
    let parent =
        aura_windows_platform::open_verified_publish_parent(parent_path).map_err(invalid)?;
    let leaf = path.file_name().ok_or_else(|| invalid("http.target_leaf"))?;
    let (identity, blake3) =
        match aura_windows_platform::open_verified_existing(path, NativeFileAccess::Read) {
            Ok(mut file) => {
                if !overwrite {
                    return Err(invalid("http.target_exists"));
                }
                if file.info().is_directory
                    || !aura_windows_platform::final_path_within(parent.info(), file.info())
                {
                    return Err(invalid("http.target_not_file"));
                }
                let mut hash = blake3::Hasher::new();
                let mut buffer = [0u8; 64 * 1024];
                loop {
                    let n = file.file_mut().read(&mut buffer).map_err(invalid)?;
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
            Err(error) => return Err(invalid(error)),
        };
    Ok(DownloadTarget {
        path: parent.info().final_path.join(leaf),
        parent_identity: aura_workspace::domain_identity(&parent.info().identity),
        identity,
        blake3,
        workspace_path,
    })
}

impl DownloadTarget {
    pub(super) fn parent(&self) -> Result<VerifiedFile, FsToolError> {
        let parent = aura_windows_platform::open_verified_publish_parent(
            self.path.parent().ok_or_else(|| invalid("http.target_parent"))?,
        )
        .map_err(invalid)?;
        if aura_workspace::domain_identity(&parent.info().identity) != self.parent_identity {
            return Err(invalid("http.parent_changed"));
        }
        Ok(parent)
    }
    pub fn verify(&self) -> Result<(), FsToolError> {
        let current = inspect_download_target(
            &self.path,
            self.workspace_path.clone(),
            self.identity.is_some(),
        )?;
        if current != *self {
            return Err(invalid("http.target_changed"));
        }
        Ok(())
    }
}
pub(super) fn invalid(value: impl std::fmt::Display) -> FsToolError {
    FsToolError::InvalidRequest(value.to_string())
}
