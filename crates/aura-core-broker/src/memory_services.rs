//! Process composition only; Memory owns files/indexing, Core owns authority.
use aura_memory::{MemoryService, VaultRoots};
#[cfg(all(not(any(test, feature = "test-hooks")), feature = "memory-test-profile"))]
use std::path::PathBuf;
use std::{path::Path, sync::Arc};

pub(crate) fn open(data_dir: &Path) -> anyhow::Result<Arc<MemoryService>> {
    let cache = data_dir.join("memory-vaults");
    #[cfg(any(test, feature = "test-hooks"))]
    let roots = {
        anyhow::ensure!(
            data_dir.starts_with(std::env::temp_dir()),
            "memory.unit_test_requires_injected_TEMP_root"
        );
        VaultRoots::new(data_dir.join("test-global-memories"), cache)
    };
    #[cfg(all(not(any(test, feature = "test-hooks")), feature = "memory-test-profile"))]
    let roots = {
        let root = PathBuf::from(
            std::env::var_os("AURA_MEMORY_TEST_ROOT")
                .ok_or_else(|| anyhow::anyhow!("memory.test_profile_required"))?,
        );
        validate_test_root(&root)?;
        VaultRoots::new(root, cache)
    };
    #[cfg(all(not(any(test, feature = "test-hooks")), not(feature = "memory-test-profile")))]
    let roots = VaultRoots::production(cache)?;
    Ok(Arc::new(MemoryService::new(roots)))
}

#[cfg(all(not(any(test, feature = "test-hooks")), feature = "memory-test-profile"))]
fn validate_test_root(root: &Path) -> anyhow::Result<()> {
    let raw_temp = std::env::temp_dir();
    let temp = std::fs::canonicalize(&raw_temp)?;
    anyhow::ensure!(
        root.is_absolute() && root.starts_with(&raw_temp) && root != raw_temp,
        "memory.test_root_must_be_inside_TEMP"
    );
    let mut ancestor = root;
    while !ancestor.try_exists()? {
        ancestor = ancestor.parent().ok_or_else(|| anyhow::anyhow!("memory.test_root_parent"))?;
    }
    anyhow::ensure!(
        std::fs::canonicalize(ancestor)?.starts_with(temp),
        "memory.test_root_reparse_escape"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn process_composition_injects_temp_without_creating_a_vault_or_using_real_profile() {
        let temp = tempfile::tempdir().expect("TEMP");
        let memory = super::open(temp.path()).expect("injected roots");
        assert_eq!(memory.roots().global, temp.path().join("test-global-memories"));
        assert!(!memory.roots().global.exists());
        assert!(super::open(std::path::Path::new("C:/work/project")).is_err());
    }
}
