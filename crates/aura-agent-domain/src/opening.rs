//! A file or a link in the conversation opens outside Aura. A workspace file
//! opens with the application Windows gives its kind, or is shown in the file manager; a web page
//! opens in the default browser. Main does it after its own checks; these are the requests and
//! the answer.
use aura_domain_common::{SessionId, WorkspaceId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where a workspace file opens outside Aura.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceFileOpener {
    /// The application Windows opens this kind of file with. Never a program or a script: Aura
    /// does not run them.
    DefaultApplication,
    /// The file manager, with the file selected.
    FileManager,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceFileOpenRequest {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    /// Workspace-relative, `/` between its parts.
    pub path: String,
    pub opener: WorkspaceFileOpener,
}

/// A web page the conversation links to; `http` and `https` only.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExternalLinkOpenRequest {
    pub url: String,
}

/// Whether it opened, and what the user is told either way.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OpenedOutside {
    pub completed: bool,
    pub message: String,
}

/// Walks a workspace's folder again for its file index, after a build that failed.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceIndexRebuildRequest {
    pub workspace_id: WorkspaceId,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceIndexRebuildStarted {
    pub workspace_id: WorkspaceId,
    /// False when a walk of the folder was already running; nothing more was started.
    pub started: bool,
}
