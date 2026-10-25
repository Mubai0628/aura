//! `fs.delete`. A file or a
//! folder goes to the Recycle Bin, where the user can restore it; one the bin cannot take is left
//! where it is. Deleting for good is asked every time.
use crate::AgentWorkspacePath;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentFileDeleteInput {
    /// The file or folder to delete; a folder goes with everything in it.
    pub path: AgentWorkspacePath,
    /// Default false: the Recycle Bin. True deletes for good and asks the user every time; use it
    /// only when the user asked for that, or the Recycle Bin cannot take the item.
    #[serde(default)]
    pub permanent: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentFileDeleteOutcome {
    /// In the Recycle Bin; the user can restore it.
    Recycled,
    /// Deleted for good, as the user approved.
    Deleted,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentFileDeleteResult {
    /// Where it was, as Core found it.
    pub path: String,
    pub was_folder: bool,
    pub outcome: AgentFileDeleteOutcome,
}
