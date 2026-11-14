//! What makes a folder an Aura installation: the marker file naming its namespace, or for an
//! installation made before markers, the layout every installer wrote. Uninstalling and moving
//! remove only what an installation holds of Aura's own, and the folder only once nothing else is
//! left in it.
use std::{
    fs, io,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use aura_runtime_contract::BundleNamespace;
use serde::{Deserialize, Serialize};

use crate::{
    install::UNINSTALLER,
    payload::{ICON, LAUNCHER},
};

pub const INSTALL_MARKER: &str = "aura-install.v1.json";
const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InstallMarker {
    schema_version: u32,
    namespace: BundleNamespace,
}

/// Writes the marker through a file beside it, so a reader never sees part of one.
pub fn write(root: &Path, namespace: &BundleNamespace) -> io::Result<()> {
    let marker = InstallMarker { schema_version: SCHEMA_VERSION, namespace: namespace.clone() };
    let bytes = serde_json::to_vec_pretty(&marker).map_err(io::Error::other)?;
    let path = root.join(INSTALL_MARKER);
    let temporary = root.join(format!("{INSTALL_MARKER}.writing"));
    let mut file = fs::File::create(&temporary)?;
    io::Write::write_all(&mut file, &bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)
}

/// The namespace the marker in `root` names; `None` without a readable marker.
fn marked(root: &Path) -> Option<BundleNamespace> {
    let bytes = fs::read(root.join(INSTALL_MARKER)).ok()?;
    let marker: InstallMarker = serde_json::from_slice(&bytes).ok()?;
    (marker.schema_version == SCHEMA_VERSION).then_some(marker.namespace)
}

/// Whether `root` is an installation of `namespace`: its marker names it, or it has no marker and
/// the layout installers wrote before markers.
#[must_use]
pub fn is_installation_of(root: &Path, namespace: &BundleNamespace) -> bool {
    if root.join(INSTALL_MARKER).exists() {
        return marked(root).as_ref() == Some(namespace);
    }
    root.join("active.json").is_file()
        && root.join("versions").is_dir()
        && root.join(LAUNCHER).is_file()
}

/// Whether `name`, directly in an installation's folder, is Aura's own.
fn own(name: &str) -> bool {
    let writing = format!("{INSTALL_MARKER}.writing");
    let fixed = [
        "versions",
        "active.json",
        "active.tmp",
        LAUNCHER,
        UNINSTALLER,
        ICON,
        INSTALL_MARKER,
        writing.as_str(),
    ];
    let lower = name.to_ascii_lowercase();
    fixed.iter().any(|own| own.eq_ignore_ascii_case(name))
        || lower.starts_with(".replaced-")
        || lower.starts_with(".staging-")
}

/// What `root` holds besides Aura's own entries.
#[must_use]
pub fn others(root: &Path) -> Vec<PathBuf> {
    fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| !own(&entry.file_name().to_string_lossy()))
        .map(|entry| entry.path())
        .collect()
}

/// Removes Aura's own entries from `root`, the marker last, and `root` itself when nothing else is
/// left in it; returns what it left. While Aura is closed Chrome starts the relay from the
/// installation for about two seconds at a time to look for it, so a refused removal is tried
/// again for a few seconds.
pub fn remove_own(root: &Path) -> io::Result<Vec<PathBuf>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let until = Instant::now() + Duration::from_secs(5);
    let mut entries: Vec<PathBuf> = fs::read_dir(root)?
        .flatten()
        .filter(|entry| own(&entry.file_name().to_string_lossy()))
        .map(|entry| entry.path())
        .collect();
    // The marker goes last, so a removal cut short still reads as the installation it was.
    entries.sort_by_key(|path| path.file_name().is_some_and(|name| name == INSTALL_MARKER));
    for entry in entries {
        loop {
            let removed =
                if entry.is_dir() { fs::remove_dir_all(&entry) } else { fs::remove_file(&entry) };
            match removed {
                Ok(()) => break,
                Err(error) if error.kind() == io::ErrorKind::NotFound => break,
                Err(_) if Instant::now() < until => thread::sleep(Duration::from_millis(100)),
                Err(error) => {
                    return Err(io::Error::new(
                        error.kind(),
                        format!("{}: {error}", entry.display()),
                    ));
                }
            }
        }
    }
    let left = others(root);
    if left.is_empty() {
        fs::remove_dir(root)?;
    }
    Ok(left)
}
