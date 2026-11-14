//! Folders the setup program makes for an installation or its data: private before anything is
//! written into them, made with the missing folders above them, and taken back as far as nothing
//! else got into them.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Makes `folder` private. A new one is created that way, after the missing folders above it; an
/// existing one gets its permissions replaced. Returns the highest folder made, if any.
pub fn make(folder: &Path) -> io::Result<Option<PathBuf>> {
    if folder.exists() {
        aura_windows_platform::make_folder_private(folder)?;
        return Ok(None);
    }
    let highest =
        folder.ancestors().take_while(|above| !above.exists()).last().map(Path::to_path_buf);
    if let Some(parent) = folder.parent() {
        fs::create_dir_all(parent)?;
    }
    aura_windows_platform::create_private_folder(folder)?;
    Ok(highest)
}

/// Removes `folder` and the folders above it up to `highest`, the ones [`make`] made, as far as
/// they are empty. Returns whether all of them are gone.
#[must_use]
pub fn take_back(folder: &Path, highest: &Path) -> bool {
    for made in folder.ancestors() {
        if fs::remove_dir(made).is_err() && made.exists() {
            return false;
        }
        if made == highest {
            break;
        }
    }
    true
}
