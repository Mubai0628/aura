//! Which namespaces the installations Settings > Apps lists use.
use std::{fs, io, path::PathBuf};

use aura_runtime_contract::BundleNamespace;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Installed {
    /// The folder names of the namespaces they use.
    Known(Vec<String>),
    /// One of them cannot be read, so a namespace in use cannot be told from one that is not:
    /// no namespace is found. Says which and why.
    Unknown(String),
}

impl Installed {
    /// From the folders the entries name (`None`: an entry that names none), or the error that
    /// kept them from being read. Each folder's `active.json` names its namespace; a folder that
    /// is not there counts as unreadable too, since an installation moved by hand still uses its
    /// data.
    #[must_use]
    pub fn from_entries(entries: io::Result<Vec<Option<PathBuf>>>) -> Self {
        let entries = match entries {
            Ok(entries) => entries,
            Err(error) => return Self::Unknown(format!("Settings > Apps: {error}")),
        };
        let mut hashes = Vec::new();
        for folder in entries {
            let Some(folder) = folder else {
                return Self::Unknown(
                    "an Aura entry in Settings > Apps names no folder".to_owned(),
                );
            };
            match namespace_of(&folder) {
                Ok(namespace) => hashes.push(namespace.object_name_hash()),
                Err(reason) => return Self::Unknown(format!("{}: {reason}", folder.display())),
            }
        }
        Self::Known(hashes)
    }

    /// Whether an installation uses the namespace in the folder `hash`; `None` when that cannot
    /// be told.
    pub(crate) fn uses(&self, hash: &str) -> Option<bool> {
        match self {
            Self::Known(hashes) => Some(hashes.iter().any(|known| known == hash)),
            Self::Unknown(_) => None,
        }
    }
}

/// Only the one field is read, so a newer version's `active.json` with fields this one does not
/// know still names its namespace.
fn namespace_of(folder: &std::path::Path) -> Result<BundleNamespace, String> {
    let bytes = fs::read(folder.join("active.json")).map_err(|error| error.to_string())?;
    let state: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|error| format!("active.json: {error}"))?;
    state["release_descriptor"]["bundle_namespace"]
        .as_str()
        .ok_or_else(|| "active.json names no namespace".to_owned())?
        .parse()
        .map_err(|error| format!("active.json: {error}"))
}
