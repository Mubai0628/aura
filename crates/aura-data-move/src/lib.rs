//! Moving an installation's data to a folder the user chose, and back to the default places.
//!
//! Core checks the folder and writes the request into the folder that never moves; the launcher,
//! or `aura-core --move-data` in a development run, does the move while Aura is closed; the new
//! place's Core removes the old copy once it is ready. The request doubles as a journal, so a
//! start after a crash or power loss takes back a half-made copy or finishes the records.
mod check;
mod command;
mod contract;
mod copy;
mod failure;
mod journal;
mod perform;
mod plan;
mod removal;
mod tree;

pub use check::{Accepted, Choice, Context, MAX_LOCATION_CHARS, Refusal, check, room_needed};
pub use command::{COMMAND, command};
pub use contract::{
    StorageCheck, StorageChoice, StorageFolderOpened, StorageMoveFailure, StorageMovePart,
    StorageMoveRequest, StorageMovedPart, StorageOpenFolderRequest, StorageOverview, StoragePart,
    StoragePlace, StorageRestart, StorageSelectedFolder, parts_of,
};
pub use failure::{FAILURE_FILE, Failure, dismiss_failure, read_failure};
pub use journal::{Created, JOURNAL_FILE, Journal, State};
pub use perform::{Fault, Hooks, Outcome, Stage, perform, resume};
pub use plan::{Item, MovePlan, Part, data_size, folder_size, plan, within};
pub use removal::{Removal, remove_old_copies};
pub use tree::TreeCopy;

use std::{io, path::PathBuf};

use aura_data_location::Locations;
use aura_runtime_contract::BundleNamespace;

/// Asks for the move `accepted` describes: the next start without Aura running does it.
pub fn request(
    namespace: &BundleNamespace,
    current: &Locations,
    destination: Option<PathBuf>,
) -> io::Result<()> {
    let journal = Journal::requested(namespace.clone(), current.location.clone(), destination);
    journal::write(&current.fixed_root, &journal)
}

/// The move waiting for the next start, if any: where to.
pub fn requested(
    namespace: &BundleNamespace,
    current: &Locations,
) -> io::Result<Option<Option<PathBuf>>> {
    Ok(journal::read(&current.fixed_root, namespace)?
        .filter(|journal| journal.state == State::Requested)
        .map(|journal| journal.destination))
}

/// Takes back a request that has not started.
pub fn withdraw(namespace: &BundleNamespace, current: &Locations) -> io::Result<()> {
    match journal::read(&current.fixed_root, namespace)? {
        Some(journal) if journal.state == State::Requested => journal::remove(&current.fixed_root),
        _ => Ok(()),
    }
}

/// Takes back a request that cannot be done now and keeps `reason` for the storage page.
pub fn decline(namespace: &BundleNamespace, current: &Locations, reason: String) -> io::Result<()> {
    let Some(destination) = requested(namespace, current)? else {
        return Ok(());
    };
    withdraw(namespace, current)?;
    failure::record_failure(&current.data_root, &Failure::now(destination, reason))
}
