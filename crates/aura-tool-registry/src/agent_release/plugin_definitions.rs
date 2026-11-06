//! Writing a plugin with the user: check its folder, install it as a plugin in development, and
//! read what went wrong in it. Installing never trusts a plugin or turns on what it asks for.
use super::definitions::entry;
use super::*;
use AgentBuiltinHandler as H;
use aura_agent_domain::*;
use aura_security_contract::ResolvedCapability as C;

pub(super) fn entries(
    at: UtcTimestamp,
) -> Result<Vec<AgentBuiltinEntry>, BuiltinToolDefinitionSetError> {
    [
        entry::<PluginsValidateInput, PluginsValidateResult>(
            107,
            H::PluginsValidate,
            "Check a plugin folder",
            "Check a plugin folder in this workspace before installing it. path is the folder, relative to the workspace root, that holds aura-plugin.toml. The result lists every problem with the part it is in, warnings, whether the plugin can be installed at all and what it brings that works. Nothing in it is run. Fix the problems and check again.",
            &[C::ToolRegistryRead],
            false,
            at,
        ),
        entry::<PluginsDevInstallInput, PluginsDevInstallResult>(
            108,
            H::PluginsDevInstall,
            "Install a plugin in development",
            "Install a plugin folder in this workspace as a plugin in development, used where it is and never copied. It is not trusted: nothing it brings works until the user trusts it in Settings → Plugins, and you cannot trust it, turn on what it asks for or fill in its settings, so tell the user to review it there. Once trusted, changes to its Skills, commands, panel and theme reload by themselves; a change to anything else (the manifest, Hooks, MCP servers, the background service) waits until the user trusts it again. Installing the same folder again changes nothing. Not available in ReadOnly.",
            &[C::PluginDevelopmentInstall],
            true,
            at,
        ),
        entry::<PluginsLogsInput, PluginsLogsResult>(
            109,
            H::PluginsLogs,
            "Read a plugin's errors",
            "Read what went wrong in one of the user's plugins, oldest first: its Hooks' failures, its MCP servers' standard error, its panel's errors and its background service's output, with the secret values it was given taken out. name is the plugin's name as its manifest gives it; limit is at most 200 (default 50).",
            &[C::ToolRegistryRead],
            false,
            at,
        ),
    ]
    .into_iter()
    .collect()
}
