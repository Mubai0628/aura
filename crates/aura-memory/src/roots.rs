use crate::MemoryResult;
use std::path::{Path, PathBuf};

/// Resolved once by process composition and then passed to all Memory services.
/// Production has no environment override. Tests use `new` with TEMP paths.
#[derive(Clone, Debug)]
pub struct VaultRoots {
    pub global: PathBuf,
    pub cache: PathBuf,
}
impl VaultRoots {
    pub fn new(global: PathBuf, cache: PathBuf) -> Self {
        Self { global, cache }
    }
    pub fn production(cache: PathBuf) -> MemoryResult<Self> {
        Ok(Self::new(
            aura_windows_platform::user_profile_directory()?.join(".aura/memories"),
            cache,
        ))
    }
    pub fn project(workspace: &Path) -> PathBuf {
        workspace.join(".memories")
    }
}

pub(crate) fn normalize_root(root: &Path) -> MemoryResult<PathBuf> {
    // Check before canonicalizing: otherwise a junction would disappear into its target.
    aura_fs_tools::vault::VaultFileSystem::new(root.to_path_buf())?;
    let mut existing = root;
    while !existing.try_exists()? {
        existing = existing.parent().ok_or(crate::MemoryError::Invalid("Vault ancestor"))?;
    }
    let tail =
        root.strip_prefix(existing).map_err(|_| crate::MemoryError::Invalid("Vault root"))?;
    if tail.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return Err(crate::MemoryError::Invalid("Vault root components"));
    }
    let canonical = std::fs::canonicalize(existing)?;
    Ok(if tail.as_os_str().is_empty() { canonical } else { canonical.join(tail) })
}
