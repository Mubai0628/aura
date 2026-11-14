//! Whether a folder is inside one that a sync client (OneDrive, Dropbox, iCloud and the like)
//! uploads and may replace with placeholders: no place for a database. Clients that use the
//! Windows cloud files API register their folders with it; OneDrive's own variables and
//! Dropbox's marker file name the rest.
use std::{
    env,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use windows_sys::Win32::Storage::CloudFilters::{CF_SYNC_ROOT_INFO_BASIC, CfGetSyncRootInfoByPath};

/// The sync client `path` belongs to, by the name shown to the user; `None` when none, and an
/// empty name for a folder only the cloud files API knows. A path that is not there yet is judged
/// by the nearest folder above it that is.
#[must_use]
pub fn sync_folder_of(path: &Path) -> Option<String> {
    let existing = path.ancestors().find(|folder| folder.exists())?;
    for variable in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
        if let Some(root) = env::var_os(variable).filter(|root| !root.is_empty())
            && inside(existing, Path::new(&root))
        {
            return Some("OneDrive".to_owned());
        }
    }
    if existing.ancestors().any(|folder| folder.join(".dropbox").is_file()) {
        return Some("Dropbox".to_owned());
    }
    registered_sync_root(existing).then(String::new)
}

fn inside(path: &Path, root: &Path) -> bool {
    let plain = |path: &Path| -> PathBuf {
        PathBuf::from(path.to_string_lossy().replace('/', "\\").to_lowercase())
    };
    plain(path).starts_with(plain(root))
}

/// Whether the cloud files API knows a sync root at or above `path`.
fn registered_sync_root(path: &Path) -> bool {
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // The basic information is a few fixed fields followed by the provider's blob; the call
    // succeeds or reports a short buffer either way when a root is there.
    let mut buffer = [0_u8; 4096];
    let mut returned = 0_u32;
    // SAFETY: `name` is NUL-terminated and the buffer is as long as the length passed.
    let result = unsafe {
        CfGetSyncRootInfoByPath(
            name.as_ptr(),
            CF_SYNC_ROOT_INFO_BASIC,
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            &mut returned,
        )
    };
    // ERROR_MORE_DATA as an HRESULT still means a root was found.
    result >= 0 || result == 0x8007_00EA_u32 as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_temporary_folder_is_no_sync_folder_and_a_dropbox_marker_makes_one() {
        let temp = tempfile::tempdir().expect("TEMP");
        assert_eq!(sync_folder_of(&temp.path().join("not-there-yet")), None);
        std::fs::write(temp.path().join(".dropbox"), b"{}").expect("marker");
        assert_eq!(sync_folder_of(&temp.path().join("inside")), Some("Dropbox".to_owned()));
    }
}
