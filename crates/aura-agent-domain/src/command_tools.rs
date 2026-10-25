//! `commands.*` model tools: read and author this workspace's prompt commands in
//! `.aura/commands`. The user's own commands and the on/off settings are not reachable.
use crate::CommandKind;
use aura_domain_common::WorkspacePath;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One command as `commands.list` reports it.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandSummary {
    pub name: String,
    pub title: String,
    pub description: String,
    pub kind: CommandKind,
    /// True for this workspace's commands; built-ins cannot be changed.
    pub workspace: bool,
    /// Why the command is not offered, when it is not.
    pub issue: Option<String>,
    /// For a workspace command: pass it as `expected.revision` to replace or remove it.
    pub revision: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandsListResult {
    pub commands: Vec<CommandSummary>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandsDescribeInput {
    /// A workspace command's name.
    pub name: String,
}

/// A workspace command's files exactly as they are on disk.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandDefinitionView {
    pub name: String,
    /// The workspace-relative folder.
    pub path: WorkspacePath,
    pub manifest: Option<String>,
    pub template: Option<String>,
    pub revision: Option<String>,
    pub issue: Option<String>,
}

/// The command's state before a save: absent, or the revision you read.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandExpectation {
    Absent,
    Revision { revision: String },
}

/// Create or replace one workspace prompt command. Core writes `command.toml` from these
/// fields and `template.md` from `template`.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandsSaveInput {
    /// Lowercase letters, digits and `-`, starting with a letter; not a built-in's name.
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    pub description: String,
    /// What the text typed after the command means.
    #[serde(default)]
    pub argument_hint: Option<String>,
    /// Message text; `$ARGUMENTS` marks where the user's text goes.
    pub template: String,
    pub expected: CommandExpectation,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandsSaveResult {
    pub name: String,
    pub created: bool,
    pub files: Vec<WorkspacePath>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandsDeleteInput {
    pub name: String,
    /// The revision from `commands.list` or `commands.describe`.
    pub revision: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandsDeleteResult {
    pub name: String,
    pub removed: WorkspacePath,
}
