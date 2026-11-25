//! A sub-agent's tool table: every tool of the release but the main agent's own, and only the
//! tools the main agent named when it narrowed the table.
use aura_tool_registry::{agent_release::release, subagent_keeps};

/// What only the main agent has: the tools that end, plan or reconcile the user's turn, that ask
/// the user, the team tools, and the user's own Chrome and desktop.
fn main_only(name: &str) -> bool {
    [
        "agents.",
        "peers.",
        "chrome.",
        "computer.",
        "task_lineage.",
        "goal.",
        "plugin.chrome-control.",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
        || [
            "normal.complete",
            "mode.disposition",
            "query.user_question",
            "session.reply",
            "plan.complete",
            "workspace_init.complete",
            "terminal.request_sensitive_input",
            "browser.request_user",
        ]
        .contains(&name)
}

/// Skill tools the release keeps for old records; no conversation is offered them.
fn retired(name: &str) -> bool {
    ["skills.describe", "skills.unload", "skills.list_loaded", "skills.read_resource"]
        .contains(&name)
}

#[test]
fn a_sub_agent_has_every_tool_but_the_main_agents_own() {
    let release = release().expect("verified current release");
    for entry in &release.manifest().entries {
        let name = entry.handler.name();
        let expected = name == "agent.report" || !(main_only(name) || retired(name));
        assert_eq!(subagent_keeps(&entry.definition, None), expected, "{name}");
    }
    let table: Vec<&str> = release
        .manifest()
        .entries
        .iter()
        .filter(|entry| subagent_keeps(&entry.definition, None))
        .map(|entry| entry.handler.name())
        .collect();
    for name in [
        "agent.report",
        "fs.write",
        "fs.delete",
        "shell.powershell",
        "browser.open",
        "browser.click",
        "history.read",
        "tools.search",
        "tools.load",
        "tools.call",
        "memory.create",
        "web.fetch",
        "skills.load",
    ] {
        assert!(table.contains(&name), "{name} is not in a sub-agent's table");
    }
}

#[test]
fn a_narrowed_table_holds_the_named_tools_a_sub_agent_may_have_and_its_report_tool() {
    let release = release().expect("verified current release");
    let narrowed: Vec<String> = ["fs.read", "web.fetch", "agents.spawn", "computer.act", "no.such"]
        .map(str::to_owned)
        .to_vec();
    let table: Vec<&str> = release
        .manifest()
        .entries
        .iter()
        .filter(|entry| subagent_keeps(&entry.definition, Some(&narrowed)))
        .map(|entry| entry.handler.name())
        .collect();
    let mut table = table;
    table.sort_unstable();
    assert_eq!(table, ["agent.report", "fs.read", "web.fetch"]);
    let bare: Vec<&str> = release
        .manifest()
        .entries
        .iter()
        .filter(|entry| subagent_keeps(&entry.definition, Some(&[])))
        .map(|entry| entry.handler.name())
        .collect();
    assert_eq!(bare, ["agent.report"]);
}
