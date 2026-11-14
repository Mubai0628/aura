//! rustc's incremental state: `incremental/<unit>/s-<time>-<random>-<hash>/` with a
//! `s-<time>-<random>.lock` file beside each session. rustc reads only the newest session of a
//! unit and deletes older ones the next time it compiles that unit, which never comes for a
//! configuration no longer built.
use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::Result;

use super::Removal;

pub(super) const SUPERSEDED: &str = "incremental sessions older than the newest of their unit";
pub(super) const IDLE: &str = "incremental folders not compiled within the idle period";

pub(super) fn superseded_and_idle(debug: &Path, idle_before: SystemTime) -> Result<Vec<Removal>> {
    let incremental = debug.join("incremental");
    let mut removals = Vec::new();
    let Ok(units) = fs::read_dir(&incremental) else { return Ok(removals) };
    for unit in units {
        let unit = unit?.path();
        if !unit.is_dir() {
            continue;
        }
        let mut sessions: Vec<(SystemTime, PathBuf)> = Vec::new();
        for entry in fs::read_dir(&unit)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                sessions.push((newest_time(&entry.path())?, entry.path()));
            }
        }
        sessions.sort();
        let Some((newest, _)) = sessions.last() else { continue };
        if *newest < idle_before {
            removals.push(Removal { bytes: size(&unit)?, path: unit, reason: IDLE });
            continue;
        }
        for (_, session) in &sessions[..sessions.len() - 1] {
            removals.push(Removal {
                bytes: size(session)?,
                path: session.clone(),
                reason: SUPERSEDED,
            });
            let lock = lock_of(session);
            if lock.is_file() {
                removals.push(Removal { bytes: 0, path: lock, reason: SUPERSEDED });
            }
        }
    }
    Ok(removals)
}

/// The lock file of a session folder: its name without the last part.
pub(super) fn lock_of(session: &Path) -> PathBuf {
    let name =
        session.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = name.rsplit_once('-').map_or(name.as_str(), |(stem, _)| stem);
    session.with_file_name(format!("{stem}.lock"))
}

fn newest_time(path: &Path) -> Result<SystemTime> {
    let mut newest = fs::metadata(path)?.modified()?;
    for entry in fs::read_dir(path)? {
        newest = newest.max(entry?.metadata()?.modified()?);
    }
    Ok(newest)
}

pub(super) fn size(path: &Path) -> Result<u64> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() {
        return Ok(metadata.len());
    }
    let mut total = 0;
    for entry in fs::read_dir(path)? {
        total += size(&entry?.path())?;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    #[test]
    fn a_session_folder_is_locked_by_the_lock_file_without_its_last_part() {
        assert_eq!(
            super::lock_of(Path::new(r"D:\t\incremental\a-1\s-hny11vjxte-0qhhdig-7106rysbxyudo")),
            PathBuf::from(r"D:\t\incremental\a-1\s-hny11vjxte-0qhhdig.lock")
        );
        assert_eq!(
            super::lock_of(Path::new(r"D:\t\incremental\a-1\s-hny11vjxte-0qhhdig-working")),
            PathBuf::from(r"D:\t\incremental\a-1\s-hny11vjxte-0qhhdig.lock")
        );
    }
}
