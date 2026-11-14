//! The two files that say where an installation's data was moved to: the location record, kept in
//! the folder at the default place that never moves, and the marker in the moved folder itself.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

use aura_runtime_contract::BundleNamespace;
use serde::{Deserialize, Serialize};

use crate::LocationError;

pub const RECORD_FILE: &str = "location.v1.json";
pub const MARKER_FILE: &str = "aura-data.v1.json";
const SCHEMA_VERSION: u32 = 1;

/// `location.v1.json`: where the namespace's data is now, and the copies of it a move left that
/// are not removed yet.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocationRecord {
    pub schema_version: u32,
    pub namespace: BundleNamespace,
    /// The folder the data was moved to; `None` once it is back in the default places and the
    /// record is kept only for the copies still to be removed.
    pub location: Option<PathBuf>,
    #[serde(default)]
    pub pending_removal: Vec<PathBuf>,
}

impl LocationRecord {
    #[must_use]
    pub fn new(namespace: BundleNamespace, location: PathBuf) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            namespace,
            location: Some(location),
            pending_removal: Vec::new(),
        }
    }

    /// The record of data in `location` (`None`: the default places) with copies to remove.
    #[must_use]
    pub fn with_removals(
        namespace: BundleNamespace,
        location: Option<PathBuf>,
        pending_removal: Vec<PathBuf>,
    ) -> Self {
        Self { schema_version: SCHEMA_VERSION, namespace, location, pending_removal }
    }
}

/// `aura-data.v1.json` in a moved folder: whose data it holds.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DataMarker {
    pub schema_version: u32,
    pub namespace: BundleNamespace,
}

impl DataMarker {
    #[must_use]
    pub fn new(namespace: BundleNamespace) -> Self {
        Self { schema_version: SCHEMA_VERSION, namespace }
    }
}

/// Writes `record` to `path` through a temporary file in the same folder, so a reader sees the old
/// record or the new one and never a part of one.
pub fn write_record(path: &Path, record: &LocationRecord) -> io::Result<()> {
    write_whole(path, &serde_json::to_vec_pretty(record).map_err(io::Error::other)?)
}

/// Writes the marker into `location`.
pub fn write_marker(location: &Path, marker: &DataMarker) -> io::Result<()> {
    write_whole(
        &location.join(MARKER_FILE),
        &serde_json::to_vec_pretty(marker).map_err(io::Error::other)?,
    )
}

fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let name = path.file_name().ok_or_else(|| io::Error::other("no file name"))?;
    let mut temporary = name.to_os_string();
    temporary.push(".writing");
    let temporary = path.with_file_name(temporary);
    // Flushed before the rename, so after a power loss the name holds the old record or all of
    // the new one, never a file whose contents were not written yet.
    let mut file = fs::File::create(&temporary)?;
    io::Write::write_all(&mut file, bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)
}

/// The record at `path`, `None` when there is none. One that cannot be read, is not version 1,
/// names another namespace or a relative folder is broken.
pub(crate) fn read_record(
    path: &Path,
    namespace: &BundleNamespace,
) -> Result<Option<LocationRecord>, LocationError> {
    let broken =
        |reason: String| LocationError::RecordUnreadable { path: path.to_path_buf(), reason };
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(broken(error.to_string())),
    };
    let record: LocationRecord =
        serde_json::from_slice(&bytes).map_err(|error| broken(error.to_string()))?;
    if record.schema_version != SCHEMA_VERSION {
        return Err(broken(format!("schema version {}", record.schema_version)));
    }
    if &record.namespace != namespace {
        return Err(broken(format!("written for {}", record.namespace.as_str())));
    }
    if record.location.as_deref().is_some_and(|location| !location.is_absolute()) {
        return Err(broken("the location is not an absolute path".to_owned()));
    }
    if record.pending_removal.iter().any(|copy| !copy.is_absolute()) {
        return Err(broken("a copy to remove is not an absolute path".to_owned()));
    }
    Ok(Some(record))
}

/// Writes `record`, or removes the record file when it says nothing any more: data in the default
/// places with no copies left to remove is what no record means.
pub fn store_record(path: &Path, record: &LocationRecord) -> io::Result<()> {
    if record.location.is_none() && record.pending_removal.is_empty() {
        return match fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    }
    write_record(path, record)
}

/// The record at `path` as it is now, `None` when there is none.
pub fn current_record(
    path: &Path,
    namespace: &BundleNamespace,
) -> Result<Option<LocationRecord>, LocationError> {
    read_record(path, namespace)
}

/// Whether `location` holds this namespace's marker; the reason when it does not.
pub(crate) fn check_marker(location: &Path, namespace: &BundleNamespace) -> Result<(), String> {
    let bytes = match fs::read(location.join(MARKER_FILE)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(format!("no {MARKER_FILE}"));
        }
        Err(error) => return Err(error.to_string()),
    };
    let marker: DataMarker =
        serde_json::from_slice(&bytes).map_err(|error| format!("{MARKER_FILE}: {error}"))?;
    if marker.schema_version != SCHEMA_VERSION {
        return Err(format!("{MARKER_FILE} has schema version {}", marker.schema_version));
    }
    if &marker.namespace != namespace {
        return Err(format!("{MARKER_FILE} belongs to {}", marker.namespace.as_str()));
    }
    Ok(())
}
