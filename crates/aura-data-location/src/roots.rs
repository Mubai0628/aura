//! The user's folders every location starts from.
use std::{env, path::PathBuf};

use crate::LocationError;

/// `local` and `roaming` follow `LOCALAPPDATA` and `APPDATA`, so a development or test run that
/// sets its own keeps its own data and settings. `profile` and `known_local` are Windows' known
/// folders, which those variables do not change: global memory and the released product's
/// location record are found there from any run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Roots {
    pub local: PathBuf,
    pub roaming: PathBuf,
    pub profile: PathBuf,
    pub known_local: PathBuf,
}

impl Roots {
    pub fn from_system() -> Result<Self, LocationError> {
        let variable = |name: &'static str| {
            env::var_os(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .ok_or(LocationError::Environment(name))
        };
        let known = |found: std::io::Result<PathBuf>| {
            found.map_err(|error| LocationError::KnownFolder(error.to_string()))
        };
        Ok(Self {
            local: variable("LOCALAPPDATA")?,
            roaming: variable("APPDATA")?,
            profile: known(aura_windows_platform::user_profile_directory())?,
            known_local: known(aura_windows_platform::local_app_data_directory())?,
        })
    }

    /// Aura's folder under `LOCALAPPDATA`, which holds every namespace's default data.
    #[must_use]
    pub fn local_aura(&self) -> PathBuf {
        self.local.join("Aura")
    }

    /// Aura's folder under `APPDATA`, which holds every namespace's settings.
    #[must_use]
    pub fn roaming_aura(&self) -> PathBuf {
        self.roaming.join("Aura")
    }

    /// The folders every namespace's own folder is in: `namespaces` under the local and under
    /// the roaming Aura folder.
    #[must_use]
    pub fn namespace_folders(&self) -> [PathBuf; 2] {
        [self.local_aura().join("namespaces"), self.roaming_aura().join("namespaces")]
    }
}
