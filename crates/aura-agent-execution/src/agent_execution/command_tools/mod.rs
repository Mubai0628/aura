//! `commands.*`: this workspace's prompt commands in `.aura/commands`. Reads take the
//! file path without effects. A save or removal is checked completely before anyone is
//! asked, binds its exact files to one Operation, asks the user every time, and is written
//! through `apply_changeset`. The user's own commands and settings are never reachable.
mod resolve;
mod run;
pub(super) use resolve::admission_feedback;
pub(super) use run::run;

use aura_agent_domain::{CommandsDeleteInput, CommandsSaveInput};
use aura_workspace_domain::FsExpectedState;
use serde::{Deserialize, Serialize};

pub(super) fn is_command_tool(action_kind: &str) -> bool {
    matches!(
        action_kind,
        "commands.list" | "commands.describe" | "commands.save" | "commands.delete"
    )
}

/// The workspace-relative folder of one command.
fn folder(name: &str) -> String {
    format!("{}/{name}", aura_commands::WORKSPACE_COMMANDS_DIRECTORY)
}

/// One file a save writes: the exact bytes, what they replace, and the state they expect.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BoundFile {
    path: String,
    content: String,
    previous: Option<String>,
    expected: FsExpectedState,
}

/// The Operation's action for a save: the request and the files it renders to. The
/// approval card previews these bytes and the write publishes these bytes.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SaveAction {
    input: CommandsSaveInput,
    created: bool,
    files: Vec<BoundFile>,
}

/// The Operation's action for a removal: the folder and what it held when it was read.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DeleteAction {
    input: CommandsDeleteInput,
    folder: String,
    manifest: Option<String>,
    template: Option<String>,
}
