//! A plugin runs only its own prompt commands, as the composer offers them now: a same-named
//! command of another source or of another plugin is not its own, and one it brings that is
//! switched off, cannot be offered or is no prompt is refused with why.
use aura_agent_domain::{CommandEntry, CommandKind, CommandSource};
use aura_plugin_session::own_prompt;

const PLUGIN: &str = "chat-bot";

/// A prompt command of `source`, its template naming where it came from.
fn command(name: &str, source: CommandSource, plugin: Option<&str>) -> CommandEntry {
    CommandEntry {
        name: name.to_owned(),
        title: name.to_owned(),
        description: String::new(),
        kind: CommandKind::Prompt,
        source,
        plugin: plugin.map(str::to_owned),
        argument_hint: None,
        template: Some(format!("{source:?} {} $ARGUMENTS", plugin.unwrap_or("-"))),
        enabled: true,
        issue: None,
        location: None,
        revision: None,
    }
}

fn refused(catalog: &[CommandEntry], name: &str) -> String {
    own_prompt(catalog, PLUGIN, name).expect_err("refused").to_string()
}

#[test]
fn a_plugin_runs_the_prompt_command_it_brings_and_no_other_of_that_name() {
    let catalog = [
        command("summary", CommandSource::User, None),
        command("summary", CommandSource::Plugin, Some("other-bot")),
        command("summary", CommandSource::Plugin, Some(PLUGIN)),
    ];
    let template = own_prompt(&catalog, PLUGIN, "summary").expect("its own command");
    assert_eq!(template, "Plugin chat-bot $ARGUMENTS");
}

#[test]
fn a_command_only_another_source_or_plugin_brings_is_unknown() {
    let catalog = [
        command("review", CommandSource::Builtin, None),
        command("review", CommandSource::User, None),
        command("review", CommandSource::Workspace, None),
        command("review", CommandSource::Plugin, Some("other-bot")),
    ];
    let unknown = refused(&catalog, "review");
    assert!(unknown.starts_with("plugin_session.command_unknown"), "{unknown}");
    assert!(unknown.contains("/review"), "{unknown}");
}

#[test]
fn its_own_command_that_cannot_be_offered_now_is_refused_with_why() {
    let mut off = command("off", CommandSource::Plugin, Some(PLUGIN));
    off.enabled = false;
    let mut shadowed = command("shadowed", CommandSource::Plugin, Some(PLUGIN));
    shadowed.issue = Some("和用户的 /shadowed 重名".to_owned());
    let mut block = command("block", CommandSource::Plugin, Some(PLUGIN));
    block.kind = CommandKind::Block;
    let mut empty = command("empty", CommandSource::Plugin, Some(PLUGIN));
    empty.template = None;
    let catalog = [off, shadowed, block, empty];
    for (name, why) in [
        ("off", "已关掉"),
        ("shadowed", "和用户的 /shadowed 重名"),
        ("block", "不是提示命令"),
        ("empty", "不是提示命令"),
    ] {
        let refusal = refused(&catalog, name);
        assert!(refusal.starts_with("plugin_session.command_unusable"), "{name}: {refusal}");
        assert!(refusal.contains(why), "{name}: {refusal}");
    }
}

#[test]
fn a_name_that_is_no_command_name_is_unknown() {
    let catalog = [command("summary", CommandSource::Plugin, Some(PLUGIN))];
    for name in ["Summary", "/summary", "sum mary", "", "1st"] {
        let refusal = refused(&catalog, name);
        assert!(refusal.starts_with("plugin_session.command_unknown"), "{name:?}: {refusal}");
    }
}
