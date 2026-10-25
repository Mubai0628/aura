//! A program from the user's machine — Git, an MCP
//! server's program, a package's or a hook's program, the system CMD — is checked in full, its
//! identity and content hash, the first time this process uses it. A later launch holds the file
//! against writes and compares only its identity, size and write time, without reading it. A
//! difference, or a launch that failed, checks it in full once more and leaves a log record.
//! Aura's own PowerShell runtime is checked the same
//! way. The runner of the Native backend still checks its images in full: it is one process per
//! operation, so there is no later launch to spare.
use crate::file_binding;
use anyhow::{Result, ensure};
use aura_domain_common::ContentHash;
use aura_windows_platform::VerifiedFile;
use aura_windows_user_contract::ExecutionFileBinding;
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Mutex, MutexGuard, OnceLock, PoisonError},
    time::SystemTime,
};

/// Whether the program may have other hard links (Git for Windows' launchers, an MCP server's
/// program under cargo's `deps`) or must have none (the system CMD's WinSxS links aside).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Links {
    Allowed,
    Single,
}

/// What a file looked like when it was last checked in full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    length: u64,
    written: SystemTime,
}

/// The programs checked in full in this process, as they were then.
static CHECKED: Mutex<Vec<(ExecutionFileBinding, Stamp)>> = Mutex::new(Vec::new());

type Log = fn(&'static str, Value);
static LOG: OnceLock<Log> = OnceLock::new();

/// Core sends these records to its own log.
pub fn route_log(log: Log) {
    let _ = LOG.set(log);
}

/// Binds the program at `path`. A file this process has checked in full and finds as it was is
/// not read again; any other is read in full and remembered.
pub fn capture(path: &Path, links: Links) -> Result<(ExecutionFileBinding, VerifiedFile)> {
    let mut file = file_binding::open_inner(path, true, links == Links::Allowed)?;
    let seen = file_binding::binding_of(&file, None);
    let stamp = stamp_of(&file)?;
    let known = checked()
        .iter()
        .find(|(binding, at)| same_file(binding, &seen) && *at == stamp)
        .map(|(binding, _)| binding.clone());
    if let Some(binding) = known {
        return Ok((binding, file));
    }
    let hash = full_check(&mut file)?;
    let binding = ExecutionFileBinding { sha256: Some(hash), ..seen };
    counted(&binding);
    remember(&binding, stamp);
    Ok((binding, file))
}

/// Holds a bound program against writes until it has started. It is not read while it is as it
/// was checked in full in this process; otherwise it is checked in full once more, and a record
/// says what was found.
pub fn hold(binding: &ExecutionFileBinding, links: Links) -> Result<VerifiedFile> {
    ensure!(binding.sha256.is_some(), "runner.program_hash_required");
    let mut file =
        file_binding::open_inner(Path::new(&binding.path), true, links == Links::Allowed)?;
    let seen = file_binding::binding_of(&file, None);
    let stamp = stamp_of(&file)?;
    let known = recorded(binding);
    if same_file(&seen, binding) && known == Some(stamp) {
        return Ok(file);
    }
    let hash = full_check(&mut file)?;
    counted(binding);
    let unchanged = same_file(&seen, binding) && binding.sha256.as_ref() == Some(&hash);
    if unchanged {
        remember(binding, stamp);
    } else {
        forget(binding);
    }
    // The first check of an unchanged program is routine; anything else is worth a record.
    if known.is_some() || !unchanged {
        log(
            "program.checked_again",
            json!({"program": binding.path, "result": if unchanged { "unchanged" } else { "changed" }}),
        );
    }
    ensure!(unchanged, "runner.file_binding_changed");
    Ok(file)
}

/// After a launch of `binding` failed: the program is checked in full once more, and a record
/// says why and what was found. The caller still reports its own failure.
pub fn check_after_failure(binding: &ExecutionFileBinding, links: Links, failure: &str) {
    let found = file_binding::open_inner(Path::new(&binding.path), true, links == Links::Allowed)
        .and_then(|mut file| {
            let seen = file_binding::binding_of(&file, None);
            let stamp = stamp_of(&file)?;
            let hash = full_check(&mut file)?;
            Ok((same_file(&seen, binding) && binding.sha256.as_ref() == Some(&hash), stamp))
        });
    let result = match found {
        Ok((true, stamp)) => {
            counted(binding);
            remember(binding, stamp);
            "unchanged"
        }
        Ok((false, _)) => {
            counted(binding);
            forget(binding);
            "changed"
        }
        Err(_) => {
            forget(binding);
            "unavailable"
        }
    };
    log(
        "program.checked_after_failure",
        json!({"program": binding.path, "failure": failure, "result": result}),
    );
}

/// A Windows error status as an exit code — the program crashed or could not load (0xC0000005,
/// 0xC0000135, 0xC000007B …) — rather than a failure the program reported itself.
pub fn is_error_status(code: u32) -> bool {
    code >= 0xC000_0000
}

/// Whether the program is still as it was checked in full in this process; nothing is read.
#[cfg(test)]
pub(crate) fn is_unchanged(binding: &ExecutionFileBinding) -> bool {
    let Some(known) = recorded(binding) else {
        return false;
    };
    file_binding::open_inner(Path::new(&binding.path), false, true).is_ok_and(|file| {
        same_file(&file_binding::binding_of(&file, None), binding)
            && stamp_of(&file).is_ok_and(|stamp| stamp == known)
    })
}

fn full_check(file: &mut VerifiedFile) -> Result<ContentHash> {
    file_binding::hash_held(file)
}

fn stamp_of(file: &VerifiedFile) -> Result<Stamp> {
    let metadata = file.file().metadata()?;
    Ok(Stamp { length: metadata.len(), written: metadata.modified()? })
}

fn same_file(left: &ExecutionFileBinding, right: &ExecutionFileBinding) -> bool {
    left.path == right.path
        && left.volume_serial == right.volume_serial
        && left.file_id == right.file_id
}

fn checked() -> MutexGuard<'static, Vec<(ExecutionFileBinding, Stamp)>> {
    CHECKED.lock().unwrap_or_else(PoisonError::into_inner)
}

fn recorded(binding: &ExecutionFileBinding) -> Option<Stamp> {
    checked().iter().find(|(known, _)| known == binding).map(|(_, stamp)| *stamp)
}

fn remember(binding: &ExecutionFileBinding, stamp: Stamp) {
    let mut checked = checked();
    checked.retain(|(known, _)| !same_file(known, binding));
    checked.push((binding.clone(), stamp));
}

fn forget(binding: &ExecutionFileBinding) {
    checked().retain(|(known, _)| known != binding);
}

/// A record about the user's program from elsewhere in this crate (Git's detection).
pub(crate) fn note(event: &'static str, fields: Value) {
    log(event, fields);
}

fn log(event: &'static str, fields: Value) {
    #[cfg(test)]
    LOGGED.lock().unwrap_or_else(PoisonError::into_inner).push((event.to_owned(), fields.clone()));
    if let Some(log) = LOG.get() {
        log(event, fields);
    }
}

#[cfg(test)]
static LOGGED: Mutex<Vec<(String, Value)>> = Mutex::new(Vec::new());
#[cfg(test)]
static FULL_CHECKS: Mutex<Vec<(ExecutionFileBinding, u32)>> = Mutex::new(Vec::new());

#[cfg(not(test))]
fn counted(_: &ExecutionFileBinding) {}

#[cfg(test)]
fn counted(binding: &ExecutionFileBinding) {
    let mut counts = FULL_CHECKS.lock().unwrap_or_else(PoisonError::into_inner);
    match counts.iter_mut().find(|(known, _)| known == binding) {
        Some((_, count)) => *count += 1,
        None => counts.push((binding.clone(), 1)),
    }
}

/// How many times this process read `binding`'s program in full.
/// Tests that count one program's full checks take turns: the record is the whole process's.
#[cfg(test)]
pub(crate) static TEST_SERIAL: Mutex<()> = Mutex::new(());

#[cfg(test)]
pub(crate) fn full_checks(binding: &ExecutionFileBinding) -> u32 {
    FULL_CHECKS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .find(|(known, _)| known == binding)
        .map_or(0, |(_, count)| *count)
}

#[cfg(test)]
pub(crate) fn logged_for_tests() -> Vec<(String, Value)> {
    LOGGED.lock().unwrap_or_else(PoisonError::into_inner).clone()
}
