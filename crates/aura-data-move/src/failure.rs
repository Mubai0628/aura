//! `launcher/move-failed.v1.json` in the data folder: why the last move did not happen. The data
//! stayed where it was, so the record is written there; the storage page shows it until the user
//! dismisses it or a move succeeds.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

use aura_domain_common::UtcTimestamp;
use serde::{Deserialize, Serialize};

pub const FAILURE_FILE: &str = "launcher/move-failed.v1.json";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Failure {
    pub at: String,
    /// Where the data was to go; `None`: back to the default places.
    pub destination: Option<PathBuf>,
    pub reason: String,
}

impl Failure {
    #[must_use]
    pub fn now(destination: Option<PathBuf>, reason: String) -> Self {
        Self { at: UtcTimestamp::now().to_string(), destination, reason }
    }
}

pub fn record_failure(data_root: &Path, failure: &Failure) -> io::Result<()> {
    let path = data_root.join(FAILURE_FILE);
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    fs::write(path, serde_json::to_vec_pretty(failure).map_err(io::Error::other)?)
}

/// The last failure, `None` when there is none or it cannot be read.
#[must_use]
pub fn read_failure(data_root: &Path) -> Option<Failure> {
    serde_json::from_slice(&fs::read(data_root.join(FAILURE_FILE)).ok()?).ok()
}

pub fn dismiss_failure(data_root: &Path) -> io::Result<()> {
    match fs::remove_file(data_root.join(FAILURE_FILE)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}
