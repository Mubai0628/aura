//! What 设置 → 系统 → 存储 shows and sends (`storage.*`).
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where the data and the installation are, how big they are, and what waits.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StorageOverview {
    /// The data's folder: the one it was moved to, or the default data folder.
    pub data: StoragePlace,
    /// The data is in a folder the user chose rather than at the default places.
    pub data_moved: bool,
    /// The installation's folder; absent in a development run.
    pub install: Option<StoragePlace>,
    /// A move asked for and waiting for the next start.
    pub requested: Option<StorageMoveRequest>,
    /// Why the last move did not happen, until it is dismissed or a move succeeds.
    pub failure: Option<StorageMoveFailure>,
    /// The copy the last move left behind is still to be removed.
    pub old_copies: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StoragePlace {
    pub path: String,
    /// What a move would copy from it, or what the folder holds, in bytes.
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StorageMoveRequest {
    /// Where the data goes; absent: back to the default places.
    pub destination: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StorageMoveFailure {
    pub at: String,
    /// Where the data was to go; absent: back to the default places.
    pub destination: Option<String>,
    pub reason: String,
}

/// A folder the user chose, or the default places.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StorageChoice {
    /// Absent: back to the default places.
    pub folder: Option<String>,
}

/// What checking a choice found.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StorageCheck {
    /// Where the data would go: the folder chosen or an `Aura` folder inside it; absent: the
    /// default places.
    pub destination: Option<String>,
    /// What the move copies, in bytes.
    pub bytes: u64,
    /// What it copies part by part; empty when the data cannot go there.
    pub parts: Vec<StorageMovePart>,
    /// The room the copy needs on the destination's disk, and the room there is.
    pub needed: u64,
    pub free: u64,
    /// Why the data cannot go there, in the user's words; empty when it can.
    pub refusals: Vec<String>,
}

/// One part of what a move copies, and its size.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StorageMovePart {
    pub part: StorageMovedPart,
    pub bytes: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageMovedPart {
    Data,
    CoreLog,
    /// The window profile without the caches Chromium rebuilds.
    WindowProfile,
    /// The released product's global memory.
    GlobalMemory,
}

impl From<crate::Part> for StorageMovedPart {
    fn from(part: crate::Part) -> Self {
        match part {
            crate::Part::Data => Self::Data,
            crate::Part::CoreLog => Self::CoreLog,
            crate::Part::WindowProfile => Self::WindowProfile,
            crate::Part::GlobalMemory => Self::GlobalMemory,
        }
    }
}

/// Each part's size in the order the plan lists them, a part met twice counted once.
#[must_use]
pub fn parts_of(plan: &crate::MovePlan) -> Vec<StorageMovePart> {
    let mut parts: Vec<StorageMovePart> = Vec::new();
    for item in &plan.items {
        let part = StorageMovedPart::from(item.part);
        match parts.iter_mut().find(|known| known.part == part) {
            Some(known) => known.bytes += item.bytes,
            None => parts.push(StorageMovePart { part, bytes: item.bytes }),
        }
    }
    parts
}

/// Main's own folder dialog for choosing where the data goes.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StorageSelectedFolder {
    /// Absent when the user cancelled.
    pub path: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoragePart {
    Data,
    Install,
}

/// Opens the folder of one part the overview names; the window never supplies a path.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StorageOpenFolderRequest {
    pub part: StoragePart,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StorageFolderOpened {
    pub completed: bool,
    pub message: String,
}

/// Aura closes so the launcher starts it again and does the move first; a development run has
/// no launcher and does not close.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StorageRestart {
    pub restarting: bool,
}
