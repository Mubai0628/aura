//! A Claude Code plugin: the parts Aura uses, how they are read, and the parts named as not
//! supported.
mod support;
use aura_plugins::{PluginAuth, PluginLayout, PluginTransport};
use support::*;

#[test]
fn the_name_and_details_come_from_plugin_json() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("commit-tools");
    claude_plugin(&root);
    let package = scan(&root).expect("a plugin");
    assert_eq!(package.layout, PluginLayout::ClaudeCode);
    assert_eq!(package.manifest.name, "commit-tools");
    assert_eq!(package.manifest.version, "0.3.0");
    assert_eq!(package.manifest.author.as_deref(), Some("Someone"));
    assert_eq!(package.skill_items().len(), 1);
}

#[test]
fn markdown_commands_become_prompt_commands_and_say_what_is_ignored() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("commit-tools");
    claude_plugin(&root);
    let package = scan(&root).expect("a plugin");
    let commands = package.command_items();
    let names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["quick-fix", "commit"]);
    let commit = commands.iter().find(|c| c.name == "commit").expect("commit");
    assert_eq!(commit.description.as_deref(), Some("Write a commit message"));
    assert_eq!(commit.argument_hint.as_deref(), Some("[scope]"));
    assert!(commit.problem.is_none(), "{commit:?}");
    let warnings = commit.warnings.join("；");
    for expected in ["allowed-tools 不生效", "model 不生效", "!`…` 不会执行", "$1…$9 不会替换"]
    {
        assert!(warnings.contains(expected), "{expected}: {warnings}");
    }
    let quick = commands.iter().find(|c| c.name == "quick-fix").expect("quick-fix");
    assert_eq!(quick.description.as_deref(), Some("Fix the smallest thing first."));
    assert!(quick.warnings.iter().any(|w| w.contains("/quick-fix")), "{quick:?}");
    let template = &package.commands.iter().find(|c| c.command.name == "commit").expect("commit");
    assert!(
        template
            .command
            .template
            .as_deref()
            .is_some_and(|t| t.starts_with("Write a commit message for $ARGUMENTS.")),
        "{:?}",
        template.command.template
    );
    assert!(
        package.unsupported.iter().any(|u| u.contains("commands/git")),
        "{:?}",
        package.unsupported
    );
}

#[test]
fn servers_fill_in_the_folder_and_ask_for_referenced_values_in_aura() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("commit-tools");
    claude_plugin(&root);
    let package = scan(&root).expect("a plugin");
    assert_eq!(package.servers.len(), 2, "sse is not one of them");
    let local = package.servers.iter().find(|s| s.name == "local").expect("local");
    assert_eq!(local.id, "commit-tools-local");
    let PluginTransport::Stdio { command, environment, secret_environment, .. } = &local.transport
    else {
        panic!("a program");
    };
    assert_eq!(command, &format!("{}\\servers\\run.cmd", root.display()));
    assert_eq!(environment, &vec![("MODE".to_owned(), "plain".to_owned())]);
    assert_eq!(secret_environment, &vec!["GITHUB_TOKEN".to_owned()]);
    assert!(local.problems.is_empty(), "{:?}", local.problems);
    let remote = package.servers.iter().find(|s| s.name == "remote").expect("remote");
    let PluginTransport::Http { headers, auth, .. } = &remote.transport else { panic!("http") };
    assert_eq!(headers, &vec![("X-Team".to_owned(), "core".to_owned())]);
    assert_eq!(auth, &Some(PluginAuth::Secret { header: None }), "Bearer is Aura's default");
    assert!(remote.problems.is_empty(), "{:?}", remote.problems);
    let unsupported = package.unsupported.join("；");
    assert!(unsupported.contains("MCP 服务器 events：sse"), "{unsupported}");
}

#[test]
fn hooks_and_agents_are_named_as_not_supported() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("commit-tools");
    claude_plugin(&root);
    let package = scan(&root).expect("a plugin");
    let unsupported = package.unsupported.join("；");
    assert!(unsupported.contains("Hook（hooks）"), "{unsupported}");
    assert!(unsupported.contains("子 Agent（agents）"), "{unsupported}");
    assert!(package.hooks.is_empty());
}

#[test]
fn a_value_mixed_with_a_reference_is_a_problem_not_a_guess() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("commit-tools");
    claude_plugin(&root);
    write(
        &root,
        ".mcp.json",
        r#"{"mcpServers": {"local": {"command": "node", "args": ["--token=${TOKEN}"],
            "env": {"URL": "https://${HOST}/api"}}}}"#,
    );
    let package = scan(&root).expect("a plugin");
    let problems = package.servers[0].problems.join("；");
    assert!(problems.contains("${TOKEN}") && problems.contains("${HOST}"), "{problems}");
    assert_eq!(package.counts().servers, 0);
}

#[test]
fn inline_servers_and_extra_command_paths_are_read() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("inline");
    write(
        &root,
        ".claude-plugin/plugin.json",
        r#"{"name": "inline", "commands": ["./extra/one.md"],
            "mcpServers": {"tool": {"command": "uvx", "args": ["some-server"]}}}"#,
    );
    write(&root, "extra/one.md", "---\ndescription: One\n---\nDo one thing.\n");
    let package = scan(&root).expect("a plugin");
    assert_eq!(package.manifest.version, "未标明");
    assert_eq!(
        package.command_items().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["one"]
    );
    assert_eq!(package.servers[0].id, "inline-tool");
    write(&root, ".claude-plugin/plugin.json", r#"{"name": "inline", "commands": ["../outside"]}"#);
    assert!(problems(scan(&root)).contains("不是插件内的相对路径"));
}
