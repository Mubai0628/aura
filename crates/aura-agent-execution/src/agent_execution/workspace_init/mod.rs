//! `/init`: `workspace_init.complete` checks the reviewed AGENTS.md against the
//! workspace before anyone is asked to approve it, binds exactly the files it writes to one
//! Operation, and writes them through the ordinary file path as one journaled changeset:
//! every file or none.
mod evidence;
mod resolve;
mod write;
pub(super) use resolve::admission_feedback;
pub(super) use write::write;

use aura_workspace_domain::WorkspaceInitCompleteInput;
use serde::{Deserialize, Serialize};

pub(super) const ACTION: &str = "workspace_init.complete";

/// The file-path calls whose effect is a workspace write: `fs.write`, `fs.patch`, `/init`
/// and saving or removing a workspace command.
pub(super) fn writes_files(action_kind: &str) -> bool {
    matches!(action_kind, "fs.write" | "fs.patch" | ACTION | "commands.save" | "commands.delete")
}
/// Refused submissions accepted in one task; after that the model has to stop and explain.
const MAX_ATTEMPTS: usize = 3;

/// The Operation's action: the submission and the exact files it renders to. The approval
/// card previews these bytes and the write publishes these bytes, so they cannot differ.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InitAction {
    input: WorkspaceInitCompleteInput,
    files: Vec<InitActionFile>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InitActionFile {
    path: String,
    content: String,
    /// The text being replaced, as the session saw it; absent for a new file.
    previous: Option<String>,
}
