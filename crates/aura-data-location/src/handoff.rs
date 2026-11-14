//! How the locations travel: the launcher gives the desktop its three, the desktop gives Core and
//! Core the Sandbox Host the data folder, and each process that is given the data folder works it
//! out again and refuses a different one. Command-line arguments, never environment variables, so
//! the commands a model runs do not see them.
use std::path::{Path, PathBuf};

use aura_runtime_contract::BundleNamespace;

use crate::{InstallCopy, LocationError, Locations, Roots};

/// The desktop's arguments from the launcher.
pub const DESKTOP_DATA_ROOT: &str = "--aura-data-root=";
pub const DESKTOP_UI_PROFILE: &str = "--aura-ui-profile=";
pub const DESKTOP_CHROME_ROOT: &str = "--aura-chrome-root=";
/// Core's and the Sandbox Host's argument.
pub const DATA_ROOT: &str = "--data-root=";

impl Locations {
    /// What the launcher passes the desktop.
    #[must_use]
    pub fn desktop_arguments(&self) -> Vec<String> {
        vec![
            format!("{DESKTOP_DATA_ROOT}{}", self.data_root.display()),
            format!("{DESKTOP_UI_PROFILE}{}", self.ui_profile.display()),
            format!("{DESKTOP_CHROME_ROOT}{}", self.chrome_root.display()),
        ]
    }

    /// What Core passes the processes it starts.
    #[must_use]
    pub fn data_root_argument(&self) -> String {
        format!("{DATA_ROOT}{}", self.data_root.display())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfirmError {
    #[error(transparent)]
    Location(#[from] LocationError),
    #[error(
        "started for the data folder {} but this installation's data is in {}",
        .passed.display(),
        .resolved.display()
    )]
    Mismatch { passed: PathBuf, resolved: PathBuf },
}

/// The locations of a process that was given `passed` as its data folder: worked out again the
/// same way, and refused when they name another folder.
pub fn confirm(
    roots: &Roots,
    namespace: &BundleNamespace,
    install: &InstallCopy,
    passed: &Path,
) -> Result<Locations, ConfirmError> {
    let locations = Locations::resolve(roots, namespace, install)?;
    if locations.data_root.as_path() != passed {
        return Err(ConfirmError::Mismatch {
            passed: passed.to_path_buf(),
            resolved: locations.data_root,
        });
    }
    Ok(locations)
}
