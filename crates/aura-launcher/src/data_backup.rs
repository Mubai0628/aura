//! The data an update could change, kept from before a newer version's first start. Only a store
//! the new version will migrate, or will write records into that the version it replaces refuses,
//! is copied: its own Core says which (`--store-migration-check … --replacing <version>`), and an
//! answer that cannot be had copies them all. A trial that fails puts the copy back before
//! the version it replaced starts again; going back to an older version puts it back only when
//! the user asked for that (`launcher/restore.v1.json`), because it drops what was written since.
use std::{
    collections::BTreeSet,
    fs, io,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use aura_domain_common::UtcTimestamp;
use serde::{Deserialize, Serialize};

/// Aura's two databases, each with its journals: a group is copied and put back as a whole, so a
/// database never meets a journal from another moment.
const GROUPS: &[(&str, &[&str])] = &[
    ("data/aura.db", &["data/aura.db", "data/aura.db-wal", "data/aura.db-shm"]),
    (
        "data/v2-execution/aura-v2.db",
        &[
            "data/v2-execution/aura-v2.db",
            "data/v2-execution/aura-v2.db-wal",
            "data/v2-execution/aura-v2.db-shm",
        ],
    ),
];
pub const RESTORE_FILE: &str = "launcher/restore.v1.json";
/// The new version's Core answers in well under a second; this only bounds a hung one.
const CHECK_TIMEOUT: Duration = Duration::from_secs(20);
/// Room beyond the copy itself, in percent of it.
const SPACE_MARGIN_PERCENT: u64 = 10;
const CHUNK: usize = 4 * 1024 * 1024;

fn backups(data_root: &Path) -> PathBuf {
    data_root.join("backups")
}

/// The folder holding the copy taken before `version` first started.
pub fn backup_folder(data_root: &Path, version: &str) -> PathBuf {
    backups(data_root).join(format!("before-{version}"))
}

/// What happened before a newer version's first start.
#[derive(Debug, Eq, PartialEq)]
pub enum Taken {
    Copied {
        stores: usize,
        bytes: u64,
    },
    /// The new version neither migrates a store nor writes records the version before it refuses,
    /// so its data stays readable by that version.
    NothingToCopy,
    /// The disk lacks room for the copy; nothing was copied.
    NoRoom {
        needed: u64,
        free: u64,
    },
}

/// The stores the new version would migrate or leave unreadable to `replacing`, the version it
/// replaces, as its Core at `core` answers for `data_root`; `None` when it could not answer, which
/// copies every store.
pub fn pending_stores(
    core: &Path,
    data_root: &Path,
    replacing: &str,
) -> Option<BTreeSet<&'static str>> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut child = Command::new(core)
        .arg("--store-migration-check")
        .arg(data_root)
        .arg("--replacing")
        .arg(replacing)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(None) if started.elapsed() < CHECK_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut answer = String::new();
    child.stdout.take()?.read_to_string(&mut answer).ok()?;
    parse_pending(&answer)
}

/// A store is left out only when the answer says plainly that it will not be migrated.
fn parse_pending(answer: &str) -> Option<BTreeSet<&'static str>> {
    let answer: serde_json::Value = serde_json::from_str(answer.trim()).ok()?;
    Some(GROUPS.iter().map(|(store, _)| *store).filter(|store| answer[*store] != false).collect())
}

/// Copies the stores `pending` names (every store when it is `None`) before `version` first
/// starts; earlier copies go. Nothing has the stores open while the launcher runs no desktop.
pub fn take(
    data_root: &Path,
    version: &str,
    pending: Option<&BTreeSet<&'static str>>,
) -> Result<Taken> {
    take_with(data_root, version, pending, aura_windows_platform::free_disk_space)
}

fn take_with(
    data_root: &Path,
    version: &str,
    pending: Option<&BTreeSet<&'static str>>,
    free_space: impl Fn(&Path) -> io::Result<u64>,
) -> Result<Taken> {
    let folder = backup_folder(data_root, version);
    let files: Vec<&str> = GROUPS
        .iter()
        .filter(|(store, _)| pending.is_none_or(|pending| pending.contains(store)))
        .flat_map(|(_, files)| files.iter().copied())
        .filter(|file| data_root.join(file).is_file())
        .collect();
    if files.is_empty() {
        // An older copy belongs to an older version's start and could only put back older data.
        remove_folders(data_root, "before-", &folder);
        return Ok(Taken::NothingToCopy);
    }
    let bytes = files
        .iter()
        .map(|file| fs::metadata(data_root.join(file)).map(|m| m.len()).unwrap_or(0))
        .sum::<u64>();
    fs::create_dir_all(backups(data_root))?;
    let needed = bytes + bytes * SPACE_MARGIN_PERCENT / 100;
    if let Ok(free) = free_space(&backups(data_root))
        && free < needed
    {
        return Ok(Taken::NoRoom { needed, free });
    }
    let staging = backups(data_root).join(format!(".taking-{version}"));
    let _ = fs::remove_dir_all(&staging);
    let copied = aura_windows_platform::with_progress_window(
        "Aura",
        "正在备份数据，准备启动新版本",
        |progress| {
            progress.set_total(bytes);
            files.iter().try_for_each(|file| {
                copy_counted(&data_root.join(file), &staging.join(file), |n| progress.advance(n))
            })
        },
    );
    if let Err(error) = copied {
        let _ = fs::remove_dir_all(&staging);
        return Err(error).context("cannot copy Aura's data before the update");
    }
    let _ = fs::remove_dir_all(&folder);
    fs::rename(&staging, &folder)?;
    remove_folders(data_root, "before-", &folder);
    Ok(Taken::Copied { stores: files.len(), bytes })
}

/// Copies one file in chunks, reporting each chunk, so a large store shows its progress.
fn copy_counted(from: &Path, to: &Path, mut advance: impl FnMut(u64)) -> io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut source = fs::File::open(from)?;
    let mut target = fs::File::create(to)?;
    let mut buffer = vec![0_u8; CHUNK];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        target.write_all(&buffer[..read])?;
        advance(read as u64);
    }
    target.sync_all()
}

fn remove_folders(data_root: &Path, prefix: &str, keep: &Path) {
    for entry in fs::read_dir(backups(data_root)).into_iter().flatten().flatten() {
        let path = entry.path();
        if path != keep && entry.file_name().to_string_lossy().starts_with(prefix) {
            let _ = fs::remove_dir_all(&path);
        }
    }
}

/// Puts the copy taken before `version` back: each store the copy holds, with its journals, as a
/// whole; a store the copy does not hold is left as it is. What it replaces is kept beside it, so
/// nothing written since is lost from disk.
pub fn restore(data_root: &Path, version: &str) -> Result<bool> {
    let folder = backup_folder(data_root, version);
    if !folder.is_dir() {
        return Ok(false);
    }
    let kept = backups(data_root)
        .join(format!("replaced-{}", UtcTimestamp::now().to_string().replace([':', '.'], "-")));
    for (store, files) in GROUPS {
        if !folder.join(store).is_file() {
            continue;
        }
        for file in *files {
            let current = data_root.join(file);
            if current.is_file() {
                let target = kept.join(file);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::rename(&current, &target)?;
            }
        }
        for file in *files {
            let copy = folder.join(file);
            if copy.is_file() {
                copy_counted(&copy, &data_root.join(file), |_| {})
                    .context("cannot put Aura's earlier data back")?;
            }
        }
    }
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
#[path = "data_backup_tests.rs"]
mod tests;
