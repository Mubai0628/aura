//! The data an update could change, kept from before a newer version's first start. A trial that
//! fails puts it back before the version it replaced starts again; going back to an older version
//! puts it back only when the user asked for that (`launcher/restore.v1.json`), because it drops
//! what was written since.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use aura_domain_common::UtcTimestamp;
use serde::{Deserialize, Serialize};

/// The stores, relative to the data folder: Aura's two databases with their journals.
const STORES: &[&str] = &[
    "data/aura.db",
    "data/aura.db-wal",
    "data/aura.db-shm",
    "data/v2-execution/aura-v2.db",
    "data/v2-execution/aura-v2.db-wal",
    "data/v2-execution/aura-v2.db-shm",
];
pub const RESTORE_FILE: &str = "launcher/restore.v1.json";

fn backups(data_root: &Path) -> PathBuf {
    data_root.join("backups")
}

/// The folder holding the copy taken before `version` first started.
pub fn backup_folder(data_root: &Path, version: &str) -> PathBuf {
    backups(data_root).join(format!("before-{version}"))
}

fn copy_stores(from: &Path, to: &Path) -> io::Result<usize> {
    let mut copied = 0;
    for store in STORES {
        let source = from.join(store);
        if !source.is_file() {
            continue;
        }
        let target = to.join(store);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&source, &target)?;
        copied += 1;
    }
    Ok(copied)
}

fn remove_folders(data_root: &Path, prefix: &str, keep: &Path) {
    for entry in fs::read_dir(backups(data_root)).into_iter().flatten().flatten() {
        let path = entry.path();
        if path != keep && entry.file_name().to_string_lossy().starts_with(prefix) {
            let _ = fs::remove_dir_all(&path);
        }
    }
}

/// Copies the stores before `version` first starts; earlier copies go. Nothing has the stores
/// open while the launcher runs no desktop.
pub fn take(data_root: &Path, version: &str) -> Result<usize> {
    let folder = backup_folder(data_root, version);
    let staging = backups(data_root).join(format!(".taking-{version}"));
    let _ = fs::remove_dir_all(&staging);
    let copied =
        copy_stores(data_root, &staging).context("cannot copy Aura's data before the update")?;
    let _ = fs::remove_dir_all(&folder);
    fs::rename(&staging, &folder)?;
    remove_folders(data_root, "before-", &folder);
    Ok(copied)
}

/// Puts the copy taken before `version` back. The stores it replaces are kept beside it, so
/// nothing written since is lost from disk.
pub fn restore(data_root: &Path, version: &str) -> Result<bool> {
    let folder = backup_folder(data_root, version);
    if !folder.is_dir() {
        return Ok(false);
    }
    let kept = backups(data_root)
        .join(format!("replaced-{}", UtcTimestamp::now().to_string().replace([':', '.'], "-")));
    for store in STORES {
        let current = data_root.join(store);
        if current.is_file() {
            let target = kept.join(store);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&current, &target)?;
        }
    }
    copy_stores(&folder, data_root).context("cannot put Aura's earlier data back")?;
    remove_folders(data_root, "replaced-", &kept);
    Ok(true)
}

/// The user's request to put the copy from before `version` back before the next start.
#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreRequest {
    pub before_version: String,
}

pub fn take_request(data_root: &Path) -> Option<RestoreRequest> {
    let path = data_root.join(RESTORE_FILE);
    let request = fs::read(&path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let _ = fs::remove_file(&path);
    request
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stores(data: &Path, text: &str) {
        for store in
            ["data/aura.db", "data/v2-execution/aura-v2.db", "data/v2-execution/aura-v2.db-wal"]
        {
            let path = data.join(store);
            fs::create_dir_all(path.parent().expect("parent")).expect("folder");
            fs::write(path, format!("{text} {store}")).expect("store");
        }
    }

    fn read(data: &Path, store: &str) -> String {
        fs::read_to_string(data.join(store)).expect("store")
    }

    #[test]
    fn a_copy_taken_before_a_version_puts_the_stores_back_and_keeps_what_it_replaced() {
        let data = tempfile::tempdir().expect("data");
        stores(data.path(), "old");
        assert_eq!(take(data.path(), "0.1.1").expect("take"), 3);
        stores(data.path(), "migrated");
        assert!(restore(data.path(), "0.1.1").expect("restore"));
        assert_eq!(read(data.path(), "data/aura.db"), "old data/aura.db");
        assert_eq!(
            read(data.path(), "data/v2-execution/aura-v2.db-wal"),
            "old data/v2-execution/aura-v2.db-wal"
        );
        let kept: Vec<_> = fs::read_dir(data.path().join("backups"))
            .expect("backups")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("replaced-"))
            .collect();
        assert_eq!(kept.len(), 1);
        let replaced = data.path().join("backups").join(&kept[0]);
        assert_eq!(read(&replaced, "data/aura.db"), "migrated data/aura.db");
    }

    #[test]
    fn only_the_latest_copy_stays_and_a_version_without_one_restores_nothing() {
        let data = tempfile::tempdir().expect("data");
        stores(data.path(), "first");
        take(data.path(), "0.1.1").expect("take");
        take(data.path(), "0.1.2").expect("take again");
        assert!(!backup_folder(data.path(), "0.1.1").exists());
        assert!(backup_folder(data.path(), "0.1.2").is_dir());
        assert!(!restore(data.path(), "0.1.1").expect("nothing to restore"));
        assert_eq!(read(data.path(), "data/aura.db"), "first data/aura.db");
    }

    #[test]
    fn a_restore_request_is_read_once() {
        let data = tempfile::tempdir().expect("data");
        let path = data.path().join(RESTORE_FILE);
        fs::create_dir_all(path.parent().expect("parent")).expect("folder");
        fs::write(&path, r#"{"before_version":"0.1.1"}"#).expect("request");
        assert_eq!(
            take_request(data.path()),
            Some(RestoreRequest { before_version: "0.1.1".into() })
        );
        assert_eq!(take_request(data.path()), None);
    }
}
