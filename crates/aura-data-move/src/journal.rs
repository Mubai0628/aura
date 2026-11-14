//! `move.v1.json` in the folder that never moves: a move the user asked for, and how far it got.
//! Core writes it once the destination checked out; the process that moves the data advances it
//! before each step that leaves something behind, so a start after a crash or power loss knows
//! whether to clean up or to finish.
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use aura_domain_common::UtcTimestamp;
use aura_runtime_contract::BundleNamespace;
use serde::{Deserialize, Serialize};

pub const JOURNAL_FILE: &str = "move.v1.json";
const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Asked for; nothing copied yet.
    Requested,
    /// Copying or checking the copy: what `created` lists is removed if the move stops here.
    Copying,
    /// The copy checked out and the records are being written: a stop here is finished.
    Committing,
}

/// Something the move made, and how to take it back.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Created {
    Folder {
        path: PathBuf,
    },
    File {
        path: PathBuf,
    },
    /// A file the copy was added to the end of; it is cut back to `length`.
    Appended {
        path: PathBuf,
        length: u64,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Journal {
    pub schema_version: u32,
    pub namespace: BundleNamespace,
    /// Where the data was when the move was asked for; `None`: the default places.
    pub from: Option<PathBuf>,
    /// Where it goes; `None`: back to the default places.
    pub destination: Option<PathBuf>,
    pub requested_at: String,
    pub state: State,
    #[serde(default)]
    pub created: Vec<Created>,
}

impl Journal {
    #[must_use]
    pub fn requested(
        namespace: BundleNamespace,
        from: Option<PathBuf>,
        destination: Option<PathBuf>,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            namespace,
            from,
            destination,
            requested_at: UtcTimestamp::now().to_string(),
            state: State::Requested,
            created: Vec::new(),
        }
    }
}

/// The journal in `fixed_root`, `None` when there is none. One that cannot be read or belongs to
/// another namespace is an error: it is never guessed at.
pub fn read(fixed_root: &Path, namespace: &BundleNamespace) -> io::Result<Option<Journal>> {
    let bytes = match fs::read(fixed_root.join(JOURNAL_FILE)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let journal: Journal = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if journal.schema_version != SCHEMA_VERSION || &journal.namespace != namespace {
        return Err(io::Error::other(format!(
            "{JOURNAL_FILE} is version {} for {}",
            journal.schema_version,
            journal.namespace.as_str()
        )));
    }
    Ok(Some(journal))
}

/// Writes the journal through a temporary file that is flushed first, so a reader after a power
/// loss sees the old state or the new one.
pub fn write(fixed_root: &Path, journal: &Journal) -> io::Result<()> {
    let path = fixed_root.join(JOURNAL_FILE);
    let temporary = fixed_root.join(format!("{JOURNAL_FILE}.writing"));
    let mut file = fs::File::create(&temporary)?;
    file.write_all(&serde_json::to_vec_pretty(journal).map_err(io::Error::other)?)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)
}

pub fn remove(fixed_root: &Path) -> io::Result<()> {
    match fs::remove_file(fixed_root.join(JOURNAL_FILE)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}
