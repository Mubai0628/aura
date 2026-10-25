//! Workspace prompt commands: read them, and create, replace or remove one with the
//! user's approval each time. The user's own commands and settings are not reachable.
use super::definitions::entry;
use super::{AgentBuiltinEntry, AgentBuiltinHandler as H, BuiltinToolDefinitionSetError};
use aura_agent_domain::*;
use aura_domain_common::UtcTimestamp;
use aura_security_contract::ResolvedCapability as C;

pub(super) fn entries(
    at: UtcTimestamp,
) -> Result<Vec<AgentBuiltinEntry>, BuiltinToolDefinitionSetError> {
    vec![
        entry::<EmptyToolInput, CommandsListResult>(57, H::CommandsList, "List workspace prompt commands", "List this workspace's prompt commands in .aura/commands with their title, any problem that keeps one from being offered, and the revision needed to change it, plus the built-in command names, which cannot be reused. The user's own commands and the on/off settings are the user's and are not shown.", &[C::WorkspaceFileRead], false, at),
        entry::<CommandsDescribeInput, CommandDefinitionView>(58, H::CommandsDescribe, "Read a workspace prompt command", "Read one workspace prompt command's command.toml and template.md exactly as stored, with its revision for commands.save or commands.delete.", &[C::WorkspaceFileRead], false, at),
        entry::<CommandsSaveInput, CommandsSaveResult>(59, H::CommandsSave, "Create or replace a workspace prompt command", "Create or replace .aura/commands/<name>/ in this workspace. Give name (lowercase letters, digits, -), description, optional title and argument_hint, and the template text; $ARGUMENTS marks where the user's text goes. Core writes command.toml and template.md. expected is {kind: absent} for a new command or {kind: revision, revision} from commands.list or commands.describe; a changed command is never overwritten. A prompt command is only message text the user chooses to send and grants nothing; never put secrets in it. Every save asks the user once in every mode, allow once or deny, showing the full files. A refusal lists every problem.", &[C::WorkspaceFileWrite, C::WorkspaceFileCreate], true, at),
        entry::<CommandsDeleteInput, CommandsDeleteResult>(60, H::CommandsDelete, "Remove a workspace prompt command", "Remove .aura/commands/<name>/ from this workspace at the revision from commands.list or commands.describe. Asks the user once in every mode, allow once or deny.", &[C::WorkspaceFileDelete], true, at),
    ]
    .into_iter()
    .collect()
}
