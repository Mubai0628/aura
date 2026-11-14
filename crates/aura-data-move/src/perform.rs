//! Doing a requested move while Aura is closed: copy, check the copy, check the stores, check the
//! access, then write the marker, the installation's copy and the location record, the record last.
//! A move that stops before the records takes back what it made and leaves the data where it was;
//! one that stops while they are written is finished by the next start.
use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
};

use aura_data_location::{
    DataMarker, LocationRecord, Locations, Roots, current_record, store_record, write_marker,
};
use aura_runtime_contract::BundleNamespace;

use crate::{
    check::{self, Refusal},
    copy::{self, CopyError, Copying, Written},
    failure::{Failure, dismiss_failure, record_failure},
    journal::{self, Created, Journal, State},
    plan::{MovePlan, plan, within},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    Copying,
    Verifying,
    CheckingStores,
    SettingAccess,
    Recording,
}

/// A failure a test puts in on purpose.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fault {
    CopyFails,
    CopyDiffers,
    StoresFail,
    AccessWrong,
    /// Stops as a power loss would after the copy checked out, before any record is written.
    StopBeforeRecords,
    /// Stops as a power loss would after the installation's copy, before the location record.
    StopBeforeLocationRecord,
}

pub struct Hooks<'a> {
    /// The stage and its bytes done out of its total; a total of 0 cannot be measured.
    pub progress: &'a dyn Fn(Stage, u64, u64),
    pub cancelled: &'a dyn Fn() -> bool,
    /// SQLite's quick check of the stores in the data folder given.
    pub check_stores: &'a dyn Fn(&Path) -> Result<(), String>,
    /// Writes the installation's copy of the location (`active.json`); a run without an
    /// installation has none to write.
    pub install_copy: &'a dyn Fn(Option<&Path>) -> io::Result<()>,
    /// The released product's global memory moves with its data.
    pub production: bool,
    pub fault: Option<Fault>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    NothingAsked,
    Moved {
        from: Option<PathBuf>,
        to: Option<PathBuf>,
    },
    Failed {
        reason: String,
    },
    Cancelled,
    /// Stopped where a crash would have; only a fault does this.
    Interrupted,
}

enum Stop {
    Cancelled,
    Failed(String),
    Interrupted,
}

impl From<CopyError> for Stop {
    fn from(error: CopyError) -> Self {
        match error {
            CopyError::Cancelled => Self::Cancelled,
            other => Self::Failed(other.to_string()),
        }
    }
}

/// Does the move `current`'s journal asks for, if any. `current` is the namespace's locations as
/// they resolve now, with global memory.
pub fn perform(
    roots: &Roots,
    namespace: &BundleNamespace,
    current: &Locations,
    hooks: &Hooks<'_>,
) -> Outcome {
    let mut journal = match journal::read(&current.fixed_root, namespace) {
        Ok(Some(journal)) if journal.state == State::Requested => journal,
        Ok(_) => return Outcome::NothingAsked,
        Err(error) => {
            return Outcome::Failed { reason: format!("{}: {error}", journal::JOURNAL_FILE) };
        }
    };
    let destination = journal.destination.clone();
    let give_up = |reason: String| {
        let _ = journal::remove(&current.fixed_root);
        let _ =
            record_failure(&current.data_root, &Failure::now(destination.clone(), reason.clone()));
        Outcome::Failed { reason }
    };
    if !same(journal.from.as_deref(), current.location.as_deref()) {
        return give_up("请求之后数据的位置已经变了".to_owned());
    }
    let target = match Locations::for_place(roots, namespace, destination.as_deref())
        .with_global_memory(roots, namespace)
    {
        Ok(target) => target,
        Err(error) => return give_up(error.to_string()),
    };
    let plan = match plan(current, &target, hooks.production) {
        Ok(plan) => plan,
        Err(error) => return give_up(error.to_string()),
    };
    let refusals = check::recheck(&target, &plan, hooks.production);
    if !refusals.is_empty() {
        return give_up(refusals.iter().map(Refusal::to_string).collect::<Vec<_>>().join("；"));
    }
    journal.state = State::Copying;
    if let Err(error) = journal::write(&current.fixed_root, &journal) {
        return give_up(error.to_string());
    }
    match copy_and_check(current, &target, &plan, &mut journal, hooks) {
        Ok(()) => {}
        Err(Stop::Interrupted) => return Outcome::Interrupted,
        Err(stop) => {
            let left = copy::undo(&journal.created);
            if left.is_empty() {
                let _ = journal::remove(&current.fixed_root);
            }
            return match stop {
                Stop::Cancelled if left.is_empty() => Outcome::Cancelled,
                Stop::Cancelled => give_up_after_undo(current, &destination, "已停止", &left),
                Stop::Failed(reason) => give_up_after_undo(current, &destination, &reason, &left),
                Stop::Interrupted => Outcome::Interrupted,
            };
        }
    }
    journal.state = State::Committing;
    if let Err(error) = journal::write(&current.fixed_root, &journal) {
        let left = copy::undo(&journal.created);
        return give_up_after_undo(current, &destination, &error.to_string(), &left);
    }
    (hooks.progress)(Stage::Recording, 0, 0);
    match commit(namespace, current, &target, &plan, hooks) {
        Ok(true) => {
            let _ = journal::remove(&current.fixed_root);
            Outcome::Moved { from: plan.from, to: plan.destination }
        }
        Ok(false) => Outcome::Interrupted,
        // The copy checked out; the next start finishes the records from the journal.
        Err(error) => Outcome::Failed { reason: error.to_string() },
    }
}

fn give_up_after_undo(
    current: &Locations,
    destination: &Option<PathBuf>,
    reason: &str,
    left: &[String],
) -> Outcome {
    let reason = if left.is_empty() {
        reason.to_owned()
    } else {
        format!("{reason}；没删掉的：{}", left.join("、"))
    };
    let _ = record_failure(&current.data_root, &Failure::now(destination.clone(), reason.clone()));
    Outcome::Failed { reason }
}

fn copy_and_check(
    current: &Locations,
    target: &Locations,
    plan: &MovePlan,
    journal: &mut Journal,
    hooks: &Hooks<'_>,
) -> Result<(), Stop> {
    if hooks.fault == Some(Fault::CopyFails) {
        return Err(Stop::Failed("复制出错（测试）".to_owned()));
    }
    let fixed_root = current.fixed_root.clone();
    let mut folders: HashSet<PathBuf> = HashSet::new();
    let mut record = |made: Created, journal: &mut Journal| -> io::Result<()> {
        let path = match &made {
            Created::Folder { path } | Created::File { path } | Created::Appended { path, .. } => {
                path.clone()
            }
        };
        // What is made inside a folder the move made goes with that folder.
        if path.ancestors().skip(1).any(|above| folders.contains(above)) {
            return Ok(());
        }
        if matches!(made, Created::Folder { .. }) {
            folders.insert(path);
        }
        journal.created.push(made);
        journal::write(&fixed_root, journal)
    };
    if let Some(location) = &plan.destination {
        if location.exists() {
            aura_windows_platform::make_folder_private(location)
                .map_err(|error| Stop::Failed(format!("{}: {error}", location.display())))?;
        } else {
            record(Created::Folder { path: location.clone() }, journal)
                .map_err(|error| Stop::Failed(error.to_string()))?;
            aura_windows_platform::create_private_folder(location)
                .map_err(|error| Stop::Failed(format!("{}: {error}", location.display())))?;
        }
    }
    let total = plan.bytes;
    let mut done = 0_u64;
    let mut written: Vec<Written> = Vec::new();
    for item in &plan.items {
        let mut made = |created: Created| record(created, journal);
        let mut copied = |bytes: u64| {
            done += bytes;
            (hooks.progress)(Stage::Copying, done, total);
        };
        let mut copying = Copying {
            made: &mut made,
            copied: &mut copied,
            cancelled: hooks.cancelled,
            not_indexed: plan.destination.is_some(),
        };
        written.extend(copy::copy_item(item, &mut copying)?);
    }
    if hooks.fault == Some(Fault::CopyDiffers)
        && let Some(file) = written.first_mut()
    {
        file.hash = blake3::hash(b"not what was copied");
    }
    let mut checked = 0_u64;
    let check_total: u64 = written.iter().map(|file| file.bytes).sum();
    copy::verify(
        &written,
        &mut |bytes| {
            checked += bytes;
            (hooks.progress)(Stage::Verifying, checked, check_total);
        },
        hooks.cancelled,
    )?;
    (hooks.progress)(Stage::CheckingStores, 0, 0);
    if hooks.fault == Some(Fault::StoresFail) {
        return Err(Stop::Failed("数据库检查没有通过（测试）".to_owned()));
    }
    (hooks.check_stores)(&target.data_root)
        .map_err(|reason| Stop::Failed(format!("数据库检查没有通过：{reason}")))?;
    (hooks.progress)(Stage::SettingAccess, 0, 0);
    if let Some(location) = &plan.destination {
        let problem = if hooks.fault == Some(Fault::AccessWrong) {
            Ok(Some("权限不对（测试）".to_owned()))
        } else {
            aura_windows_platform::private_folder_problem(location)
        };
        match problem {
            Ok(None) => {}
            Ok(Some(problem)) => return Err(Stop::Failed(format!("权限没有设好：{problem}"))),
            Err(error) => return Err(Stop::Failed(format!("读不出权限：{error}"))),
        }
    }
    if hooks.fault == Some(Fault::StopBeforeRecords) {
        return Err(Stop::Interrupted);
    }
    Ok(())
}

/// Writes the marker, the installation's copy and the location record, in that order; each step
/// can be done again. `false` when a fault stopped it part way.
fn commit(
    namespace: &BundleNamespace,
    current: &Locations,
    target: &Locations,
    plan: &MovePlan,
    hooks: &Hooks<'_>,
) -> io::Result<bool> {
    if let Some(location) = &plan.destination {
        write_marker(location, &DataMarker::new(namespace.clone()))?;
    }
    (hooks.install_copy)(plan.destination.as_deref())?;
    if hooks.fault == Some(Fault::StopBeforeLocationRecord) {
        return Ok(false);
    }
    let earlier = current_record(&current.record, namespace)
        .map_err(io::Error::other)?
        .map(|record| record.pending_removal)
        .unwrap_or_default();
    let mut removals = plan.old_copies.clone();
    for copy in earlier {
        if !removals.iter().any(|known| within(known, &copy) && within(&copy, known)) {
            removals.push(copy);
        }
    }
    // A copy left from an earlier move that is now the destination is the data, not a copy.
    removals.retain(|copy| {
        plan.destination.as_deref().is_none_or(|location| !within(location, copy))
            && !within(&target.data_root, copy)
    });
    store_record(
        &target.record,
        &LocationRecord::with_removals(namespace.clone(), plan.destination.clone(), removals),
    )?;
    // An earlier failure's record was copied along with the data.
    let _ = dismiss_failure(&current.data_root);
    let _ = dismiss_failure(&target.data_root);
    Ok(true)
}

/// At a start, before the locations are worked out: a move that stopped while copying is taken
/// back, one that stopped while writing its records is finished.
pub fn resume(roots: &Roots, namespace: &BundleNamespace, hooks: &Hooks<'_>) -> Outcome {
    let fixed_root = Locations::for_place(roots, namespace, None).fixed_root;
    let journal = match journal::read(&fixed_root, namespace) {
        Ok(Some(journal)) => journal,
        Ok(None) => return Outcome::NothingAsked,
        Err(error) => {
            return Outcome::Failed { reason: format!("{}: {error}", journal::JOURNAL_FILE) };
        }
    };
    let place = |location: Option<&Path>| {
        Locations::for_place(roots, namespace, location).with_global_memory(roots, namespace)
    };
    match journal.state {
        State::Requested => Outcome::NothingAsked,
        State::Copying => {
            let left = copy::undo(&journal.created);
            if left.is_empty() {
                let _ = journal::remove(&fixed_root);
            }
            let reason = "上次搬数据没有做完，数据留在原处".to_owned();
            if let Ok(from) = place(journal.from.as_deref()) {
                let _ = record_failure(
                    &from.data_root,
                    &Failure::now(journal.destination.clone(), reason.clone()),
                );
            }
            Outcome::Failed { reason }
        }
        State::Committing => {
            let finished = place(journal.from.as_deref()).and_then(|current| {
                let target = place(journal.destination.as_deref())?;
                Ok((current, target))
            });
            let (current, target) = match finished {
                Ok(places) => places,
                Err(error) => return Outcome::Failed { reason: error.to_string() },
            };
            let plan = match plan(&current, &target, hooks.production) {
                Ok(plan) => plan,
                Err(error) => return Outcome::Failed { reason: error.to_string() },
            };
            match commit(namespace, &current, &target, &plan, &Hooks { fault: None, ..*hooks }) {
                Ok(_) => {
                    let _ = journal::remove(&fixed_root);
                    Outcome::Moved { from: plan.from, to: plan.destination }
                }
                Err(error) => Outcome::Failed { reason: error.to_string() },
            }
        }
    }
}

fn same(left: Option<&Path>, right: Option<&Path>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => within(left, right) && within(right, left),
        _ => false,
    }
}
