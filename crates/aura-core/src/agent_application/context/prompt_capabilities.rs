//! The prompt capabilities a loaded tool brings: its own name, and the capability module of the
//! group it belongs to. Every `capability:` module in the prompt bundle needs a tool that brings
//! it, or it is never sent.

/// The names a loaded tool adds to the prompt activation input.
pub(super) fn capabilities_of(tool: &str) -> Vec<String> {
    let group = if tool.starts_with("memory.") {
        Some("memory")
    } else if tool.starts_with("workspace_init.") {
        Some("workspace_init")
    } else if tool.starts_with("commands.") {
        Some("command_authoring")
    } else if tool.starts_with("web.") || tool.starts_with("http.") {
        Some("web")
    } else if tool.starts_with("shell.") || tool.starts_with("terminal.") {
        Some("shell_terminal_process")
    } else if tool.starts_with("git.") {
        Some("git")
    } else if tool.starts_with("browser.") {
        Some("browser")
    } else if tool.starts_with("mcp.") {
        Some("mcp")
    } else if tool.starts_with("skills.") {
        Some("skills")
    } else {
        None
    };
    std::iter::once(tool.to_owned()).chain(group.map(str::to_owned)).collect()
}

#[cfg(test)]
mod tests {
    use super::capabilities_of;
    use aura_context_domain::PromptActivationCondition;
    use aura_prompt_runtime::ValidatedPromptBundle;
    use std::collections::BTreeSet;

    /// Modules no tool brings: tool drafting has no tool (UNWIRED), and the compaction command
    /// module follows the run's purpose instead.
    const NOT_BROUGHT_BY_A_TOOL: [&str; 2] = ["tool_authoring", "compaction_command"];

    #[test]
    fn every_capability_module_has_a_tool_that_brings_it() {
        let release = aura_tool_registry::agent_release::release().expect("built-in tools");
        let mut tools: Vec<String> = release
            .manifest()
            .entries
            .iter()
            .map(|entry| entry.handler.name().to_owned())
            .collect();
        // A trusted MCP server's tool, as `aura_mcp` names it.
        tools.push("mcp.github.search_repositories".to_owned());
        let brought: BTreeSet<String> =
            tools.iter().flat_map(|tool| capabilities_of(tool)).collect();
        let bundle = ValidatedPromptBundle::embedded().expect("the embedded prompt bundle");
        for module in &bundle.modules {
            if let PromptActivationCondition::Capability { capability } = &module.activation
                && !NOT_BROUGHT_BY_A_TOOL.contains(&capability.as_str())
            {
                assert!(
                    brought.contains(capability),
                    "{} waits for `{capability}`, which no tool brings",
                    module.module_id
                );
            }
        }
    }

    #[test]
    fn each_tool_group_brings_its_module_and_keeps_its_own_name() {
        for (tool, capability) in [
            ("shell.powershell", "shell_terminal_process"),
            ("shell.cmd", "shell_terminal_process"),
            ("terminal.open", "shell_terminal_process"),
            ("git.push", "git"),
            ("browser.open", "browser"),
            ("mcp.github.search_repositories", "mcp"),
            ("skills.load", "skills"),
            ("memory.read", "memory"),
            ("workspace_init.complete", "workspace_init"),
            ("commands.save", "command_authoring"),
            ("web.fetch", "web"),
            ("http.request", "web"),
        ] {
            let brought = capabilities_of(tool);
            assert!(brought.iter().any(|name| name == capability), "{tool} -> {brought:?}");
            assert!(brought.iter().any(|name| name == tool), "{tool} keeps its own name");
        }
        assert_eq!(capabilities_of("fs.read"), vec!["fs.read".to_owned()]);
    }
}
