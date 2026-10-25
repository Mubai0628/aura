//! `workspace_init.complete` input and result. The model supplies content and evidence;
//! Core renders the fixed structure and decides whether the evidence is real.
use crate::{FsExpectedState, FsMutationDisposition};
use aura_domain_common::{ToolCallId, WorkspacePath};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The root AGENTS.md as five fixed chapters, plus optional pointer documents.
///
/// Chapter titles are fixed English headings rendered by Core; each chapter text is
/// Markdown without level-1 or level-2 headings (use `###` or deeper).
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceInitCompleteInput {
    /// The language of the body text and of the labels Core adds to commands.
    pub language: InitLanguage,
    /// The root AGENTS.md as you last read it: {expected_kind:"absent"} when it does
    /// not exist, otherwise its latest logical_hash from fs.read.
    #[serde(deserialize_with = "crate::agent_write::deserialize_expected")]
    #[schemars(with = "crate::agent_write::AgentFileWriteExpectation")]
    pub expected: FsExpectedState,
    /// "Project Structure & Module Organization".
    pub project_structure: String,
    /// "Build, Test, and Development Commands".
    pub commands: InitCommands,
    /// "Coding Style & Naming Conventions".
    pub coding_style: String,
    /// "Testing Guidelines".
    pub testing: String,
    /// "Commit & Pull Request Guidelines".
    pub commits_and_pull_requests: String,
    /// Optional longer documents under docs/agent-guides/, linked from the chapters.
    #[serde(default)]
    pub guides: Vec<InitGuide>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InitLanguage {
    Chinese,
    English,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InitCommands {
    /// Optional text before the command list.
    #[serde(default)]
    pub introduction: String,
    pub entries: Vec<InitCommand>,
    /// Optional text after the command list.
    #[serde(default)]
    pub notes: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InitCommand {
    /// Exactly as it is typed, on one line.
    pub command: String,
    /// What it is for, stated as fact.
    pub purpose: String,
    /// Workspace-relative directory it runs in; "." is the workspace root.
    #[serde(default = "workspace_root")]
    pub working_directory: String,
    pub shell: InitShell,
    /// What must be true first, if anything.
    #[serde(default)]
    pub prerequisites: String,
    pub evidence: InitEvidence,
}

fn workspace_root() -> String {
    ".".to_owned()
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InitShell {
    #[serde(rename = "powershell")]
    PowerShell,
    Cmd,
    /// The same text works in either shell.
    Any,
}

/// Where a command comes from. Nothing else counts as evidence.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InitEvidence {
    /// A workspace file that contains the command exactly as written.
    RepositoryDefined { path: String },
    /// A successful shell.powershell or shell.cmd call of this turn whose script contains
    /// the command exactly as written.
    ExecutionVerified { tool_call_id: ToolCallId },
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InitGuide {
    /// File name under docs/agent-guides/ without ".md": lowercase letters, digits and
    /// hyphens.
    pub name: String,
    /// One line; rendered as the document's level-1 heading.
    pub title: String,
    /// Markdown without a level-1 heading.
    pub body: String,
    /// The file as you last read it, as for `expected` above.
    #[serde(deserialize_with = "crate::agent_write::deserialize_expected")]
    #[schemars(with = "crate::agent_write::AgentFileWriteExpectation")]
    pub expected: FsExpectedState,
}

/// What was written, in the order it was written. Nothing is written when any file
/// fails; a stale expected state reports a conflict instead.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceInitCompleteResult {
    pub disposition: FsMutationDisposition,
    pub files: Vec<InitWrittenFile>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InitWrittenFile {
    pub path: WorkspacePath,
    /// False when an existing file was replaced; read it again before another change.
    pub created: bool,
}
