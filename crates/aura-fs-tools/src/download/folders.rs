//! Folders a download into the workspace still needs. The last folder on the file's way that
//! exists is reached from the workspace root one folder at a time by handle, never through a
//! link, and each missing one is created by handle inside the one above it. Outside the workspace
//! a missing folder is named instead of created.
use super::{DownloadTarget, lines, target::invalid};
use crate::FsToolError;
use aura_domain_common::WorkspacePath;
use aura_windows_platform::{HandlePublishError, VerifiedFile};
use std::{ffi::OsStr, io::ErrorKind, path::Path};

/// Every line Core writes about the folders on a download's way starts with this.
pub const FOLDER_LINE_PREFIX: &str = "http.download_folder_";

/// A folder outside the workspace that does not exist: named, with what to do.
#[must_use]
pub fn missing_folder_line(folder: &Path) -> String {
    format!(
        "http.download_folder_missing: 文件夹 {} 不存在。工作区外的文件夹下载时不会自动新建：可以先建好它再下载，或者下载到工作区里。",
        folder.display()
    )
}

/// The target at `path` while its own folder does not exist. Nothing exists below a missing
/// folder, so neither does the file.
pub(super) fn pending(
    path: &Path,
    workspace_path: Option<WorkspacePath>,
    overwrite: bool,
) -> Result<DownloadTarget, FsToolError> {
    let folder = path.parent().ok_or_else(lines::no_parent)?;
    let Some(relative) = workspace_path else {
        return Err(invalid(missing_folder_line(folder)));
    };
    let leaf = path.file_name().ok_or_else(lines::no_leaf)?;
    let (anchor, missing_folders) = anchor(path, &relative)?;
    if missing_folders.is_empty() {
        // Another call made the folder since it was looked for.
        return super::inspect_download_target(path, Some(relative), overwrite);
    }
    let mut full = anchor.info().final_path.clone();
    full.extend(&missing_folders);
    Ok(DownloadTarget {
        path: full.join(leaf),
        parent_identity: aura_workspace::domain_identity(&anchor.info().identity),
        identity: None,
        blake3: None,
        workspace_path: Some(relative),
        missing_folders,
    })
}

/// The last folder on the way to `path` that exists, and the folders below it, outermost first.
fn anchor(
    path: &Path,
    relative: &WorkspacePath,
) -> Result<(VerifiedFile, Vec<String>), FsToolError> {
    let mut folders = relative.as_str().split('/').collect::<Vec<_>>();
    folders.pop();
    let mut root = path.parent().ok_or_else(lines::no_parent)?;
    for _ in &folders {
        root = root.parent().ok_or_else(lines::no_parent)?;
    }
    let mut current = aura_windows_platform::open_verified_publish_parent(root)
        .map_err(|error| lines::unusable(root, &error))?;
    for (index, name) in folders.iter().enumerate() {
        match aura_windows_platform::handle_bound_open_child_directory(&current, OsStr::new(name)) {
            Ok(child) => current = child,
            Err(HandlePublishError::Windows(error)) if error.kind() == ErrorKind::NotFound => {
                let missing = folders[index..].iter().map(|name| (*name).to_owned()).collect();
                return Ok((current, missing));
            }
            Err(error) => return Err(unusable(&folders[..=index].join("/"), &error)),
        }
    }
    Ok((current, Vec::new()))
}

/// A folder on the file's way that is a link, a file, or cannot be opened or made.
fn unusable(folder: &str, error: &HandlePublishError) -> FsToolError {
    let why = match error {
        HandlePublishError::Windows(error) if error.kind() == ErrorKind::PermissionDenied => {
            "没有权限".to_owned()
        }
        HandlePublishError::Windows(error)
            if matches!(error.kind(), ErrorKind::NotADirectory | ErrorKind::Other) =>
        {
            "它不是普通文件夹（是文件或链接）".to_owned()
        }
        other => other.to_string(),
    };
    invalid(format!(
        "http.download_folder_unusable: 工作区里的 {folder} 用不了：{why}，文件没有下载。可以换一个下载位置，或者先处理好这个文件夹。"
    ))
}

/// Folders a download opened on its way and which of them it made, so a download that does not
/// finish takes back the ones that are still empty.
#[derive(Default)]
pub(super) struct DownloadFolders {
    chain: Vec<VerifiedFile>,
    made: Vec<usize>,
}

impl DownloadFolders {
    /// Makes `target`'s missing folders below `anchor`, one inside the other, and points the
    /// target at its own folder, opened by path as an existing folder is. A folder another call
    /// made meanwhile is used when it is an ordinary folder in its place.
    pub(super) fn make(
        &mut self,
        target: &mut DownloadTarget,
        anchor: VerifiedFile,
    ) -> Result<VerifiedFile, FsToolError> {
        if target.missing_folders.is_empty() {
            return Ok(anchor);
        }
        let relative = target.workspace_path.as_ref().map_or("", WorkspacePath::as_str);
        let base = relative.split('/').count().saturating_sub(1 + target.missing_folders.len());
        self.chain.push(anchor);
        for (index, name) in target.missing_folders.iter().enumerate() {
            let parent = self.chain.last().ok_or_else(lines::no_parent)?;
            let folder =
                || relative.split('/').take(base + index + 1).collect::<Vec<_>>().join("/");
            let (next, made) = match aura_windows_platform::handle_bound_create_directory(
                parent,
                OsStr::new(name),
            ) {
                Ok(created) => (created, true),
                Err(HandlePublishError::Windows(error))
                    if error.kind() == ErrorKind::AlreadyExists =>
                {
                    let existing = aura_windows_platform::handle_bound_open_child_directory(
                        parent,
                        OsStr::new(name),
                    )
                    .map_err(|error| unusable(&folder(), &error))?;
                    (existing, false)
                }
                Err(error) => return Err(unusable(&folder(), &error)),
            };
            self.chain.push(next);
            if made {
                self.made.push(self.chain.len() - 1);
            }
        }
        let own = self.chain.last().ok_or_else(lines::no_parent)?;
        let folder = aura_windows_platform::open_verified_publish_parent(&own.info().final_path)
            .map_err(|error| lines::unusable(&own.info().final_path, &error))?;
        if folder.info().identity != own.info().identity {
            return Err(lines::parent_changed());
        }
        target.parent_identity = aura_workspace::domain_identity(&folder.info().identity);
        target.missing_folders.clear();
        Ok(folder)
    }

    /// The download finished: its folders stay.
    pub(super) fn keep(&mut self) {
        self.made.clear();
        self.chain.clear();
    }

    /// Takes back the folders this download made, innermost first, each while it is empty; the
    /// handles below a folder are closed before it is removed.
    pub(super) fn take_back(&mut self) {
        while let Some(index) = self.made.pop() {
            self.chain.truncate(index + 1);
            if let Some(folder) = self.chain.pop() {
                let _ = aura_windows_platform::handle_bound_remove_created_directory(&folder);
            }
        }
        self.chain.clear();
    }
}
