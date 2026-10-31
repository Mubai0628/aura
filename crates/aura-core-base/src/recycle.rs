//! The Recycle Bin, for deletions the user can take back: the model's file deletions and Skills
//! deleted from the user's own folders.
use std::path::Path;

use aura_windows_platform::RecycleOutcome;

#[cfg(not(any(test, feature = "test-hooks")))]
pub fn move_to_recycle_bin(path: &Path) -> std::io::Result<RecycleOutcome> {
    aura_windows_platform::move_to_recycle_bin(path)
}

/// Tests never touch the user's Recycle Bin: the item moves to a bin folder beside it, and a name
/// with `not-recyclable` in it is one the bin cannot take.
#[cfg(any(test, feature = "test-hooks"))]
pub fn move_to_recycle_bin(path: &Path) -> std::io::Result<RecycleOutcome> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if name.contains("not-recyclable") {
        return Ok(RecycleOutcome::NotRecyclable);
    }
    let bin = path.parent().unwrap_or(path).join(".test-recycle-bin");
    std::fs::create_dir_all(&bin)?;
    std::fs::rename(path, bin.join(name))?;
    Ok(RecycleOutcome::Recycled)
}
