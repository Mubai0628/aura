use crate::journal_stage::{self, quarantine_name};
use anyhow::{Result, ensure};
use aura_domain_common::ContentHash;
use aura_windows_platform::{NativeFileAccess, VerifiedFile, execution_maintenance as native};
use aura_windows_user_contract::{MaintenanceConfig, MaintenanceJournal};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};
pub struct ProtectedStorage {
    pub root: PathBuf,
    _root: VerifiedFile,
}
impl ProtectedStorage {
    pub fn open() -> Result<Self> {
        let root = native::data_directory()?;
        native::verify_protected_path(&root)?;
        let retained = aura_windows_platform::open_verified_publish_parent(&root)?;
        Ok(Self { root, _root: retained })
    }
    pub fn read<T: DeserializeOwned>(
        &self,
        name: &str,
        max: u64,
    ) -> Result<(T, Vec<u8>, VerifiedFile)> {
        ensure!(matches!(name, "config.json" | "journal.json"), "maintenance.fixed_input");
        let path = self.root.join(name);
        native::verify_protected_path(&path)?;
        let mut file =
            aura_windows_platform::open_verified_import_source(&path, NativeFileAccess::Read)?;
        ensure!(
            !file.info().is_reparse_point && file.info().number_of_links == 1,
            "maintenance.input_alias"
        );
        let mut bytes = Vec::new();
        file.file_mut().take(max + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() as u64 <= max, "maintenance.input_size");
        Ok((serde_json::from_slice(&bytes)?, bytes, file))
    }
    pub fn has_journal(&self) -> bool {
        self.root.join("journal.json").exists() || self.root.join("journal.next").exists()
    }
    pub fn load_journal(
        &self,
        config: &MaintenanceConfig,
        config_hash: &ContentHash,
    ) -> Result<(MaintenanceJournal, Vec<u8>)> {
        let path = self.root.join("journal.json");
        let stage = self.root.join("journal.next");
        for existing in [&path, &stage] {
            if existing.exists() {
                native::verify_protected_path(existing)?;
            }
        }
        let committed = read_optional(&path)?;
        let staged = read_optional(&stage)?;
        let staged_invalid = staged
            .as_ref()
            .is_some_and(|bytes| serde_json::from_slice::<MaintenanceJournal>(bytes).is_err());
        let (journal, promote) = journal_stage::load_journal_bytes(
            committed.as_deref(),
            staged.as_deref(),
            config,
            config_hash,
        )?;
        if staged_invalid && let Some(bytes) = &staged {
            quarantine(&stage, "journal.next", bytes)?;
        }
        if promote {
            aura_windows_platform::atomic_replace_file(&stage, &path)?;
            let bytes = staged.ok_or_else(|| anyhow::anyhow!("maintenance.journal_corrupt"))?;
            return Ok((journal, bytes));
        }
        let bytes = committed.ok_or_else(|| anyhow::anyhow!("maintenance.journal_corrupt"))?;
        Ok((journal, bytes))
    }
    pub fn save<T: Serialize>(&self, value: &T) -> Result<()> {
        let path = self.root.join("journal.json");
        let stage = self.root.join("journal.next");
        for existing in [&path, &stage] {
            if existing.exists() {
                native::verify_protected_path(existing)?;
                let info = aura_windows_platform::inspect_existing_path(existing, false)?;
                ensure!(
                    !info.is_reparse_point && !info.is_directory && info.number_of_links == 1,
                    "maintenance.journal_alias"
                );
            }
        }
        let bytes = serde_json::to_vec(value)?;
        ensure!(bytes.len() <= 8 * 1024 * 1024, "maintenance.journal_limit");
        if stage.exists() {
            let existing = std::fs::read(&stage)?;
            if existing != bytes {
                quarantine(&stage, "journal.next", &existing)?;
            }
        }
        let mut file =
            std::fs::OpenOptions::new().create(true).truncate(true).write(true).open(&stage)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        native::verify_protected_path(&stage)?;
        aura_windows_platform::atomic_replace_file(&stage, &path)?;
        Ok(())
    }
}
fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    if !path.exists() {
        return Ok(None);
    }
    let mut file =
        aura_windows_platform::open_verified_import_source(path, NativeFileAccess::Read)?;
    ensure!(
        !file.info().is_reparse_point && file.info().number_of_links == 1,
        "maintenance.journal_alias"
    );
    let mut bytes = Vec::new();
    file.file_mut().take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= 8 * 1024 * 1024, "maintenance.journal_limit");
    Ok(Some(bytes))
}
fn quarantine(path: &Path, kind: &str, bytes: &[u8]) -> Result<()> {
    native::verify_protected_path(path)?;
    let dest = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("maintenance.journal_alias"))?
        .join(quarantine_name(kind, bytes));
    if dest.exists() {
        std::fs::remove_file(path)?;
        return Ok(());
    }
    std::fs::rename(path, &dest)?;
    native::verify_protected_path(&dest)?;
    Ok(())
}
pub fn verify_image() -> Result<VerifiedFile> {
    let directory = native::program_directory()?;
    native::verify_protected_path(&directory)?;
    let image = directory.join("aura-execution-maintainer.exe");
    native::verify_protected_path(&image)?;
    ensure!(
        std::fs::canonicalize(std::env::current_exe()?)? == std::fs::canonicalize(&image)?,
        "maintenance.image_location"
    );
    let file = aura_windows_platform::open_verified_executable(Path::new(&image))?;
    Ok(file)
}
