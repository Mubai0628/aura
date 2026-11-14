//! A command a chat message starts with, as Aura reads it: only `/name` at the start of the
//! message; names from the composer's own commands, where Aura's own that work from a chat and the
//! user's, the plugins' and the workspace's prompt commands are found; the menu a platform shows,
//! with the decisions on approvals only while the plugin's chats may decide them; and each
//! command message counted once.
use aura_agent_domain::{CommandEntry, CommandKind, CommandSource, builtin_commands};
use aura_plugin_host::MenuCommand;
use aura_plugin_session::{
    Remote, RemoteCommand, RemoteDecision, SEEN_COMMAND_TIME, SEEN_COMMANDS_MAX, SeenCommands,
    Typed, expanded, help_text, menu, remote_command, typed,
};
use aura_plugins::PluginCapability::{SessionControl, SessionRead, SessionSend};
use std::time::{Duration, Instant};

fn prompt(name: &str, source: CommandSource, template: &str) -> CommandEntry {
    CommandEntry {
        name: name.into(),
        title: format!("{name} 的标题"),
        description: format!("{name} 的说明"),
        kind: CommandKind::Prompt,
        source,
        plugin: (source == CommandSource::Plugin).then(|| "weekly-bot".into()),
        argument_hint: Some("参数".into()),
        template: Some(template.into()),
        enabled: true,
        issue: None,
        location: None,
        revision: None,
    }
}

/// The composer's catalog with a prompt command of each kind, one turned off, and one of the
/// user's named like a command Aura has only for chats.
fn catalog() -> Vec<CommandEntry> {
    let mut catalog = builtin_commands();
    catalog.push(prompt("review", CommandSource::User, "审查 $ARGUMENTS 的改动"));
    catalog.push(prompt("status", CommandSource::User, "说说现状"));
    catalog.push(prompt("deny", CommandSource::Plugin, "拒绝所有"));
    let mut off = prompt("off", CommandSource::User, "关掉的");
    off.enabled = false;
    catalog.push(off);
    catalog.push(prompt("weekly", CommandSource::Plugin, "写周报"));
    catalog.push(prompt("deploy", CommandSource::Workspace, "部署"));
    catalog
}

#[test]
fn only_a_name_at_the_start_of_the_message_is_a_command() {
    let command = |name, arguments| Some(Typed { name, arguments });
    assert_eq!(typed("/plan 写一个方案"), command("plan", "写一个方案"));
    assert_eq!(typed("  /compact\n只留结论\n"), command("compact", "只留结论"));
    assert_eq!(typed("/help"), command("help", ""));
    // Telegram adds the bot's name in groups.
    assert_eq!(typed("/model@AuraBot 2"), command("model", "2"));
    for plain in
        ["看看 /plan", "/Plan 大写", "/计划", "/", "//plan", "/plan文字", "/2 号", "", "plan"]
    {
        assert_eq!(typed(plain), None, "{plain:?}");
    }
}

#[test]
fn a_chat_uses_the_composers_commands_by_name() {
    let catalog = catalog();
    let all = [SessionSend, SessionRead, SessionControl];
    let builtin = |remote| Some(RemoteCommand::Builtin(remote));
    assert_eq!(remote_command("plan", &catalog, &all), builtin(Remote::Plan));
    assert_eq!(remote_command("help", &catalog, &all), builtin(Remote::Help));
    assert_eq!(remote_command("sessions", &catalog, &all), builtin(Remote::Sessions));
    let status = remote_command("status", &catalog, &all);
    assert_eq!(status, builtin(Remote::Status), "Aura's own wins");
    // A chat's decisions are read whether or not its plugin's chats may decide now: the answer
    // says why when they may not.
    for (name, decision) in [
        ("approve", RemoteDecision::Approve),
        ("approve-session", RemoteDecision::ApproveSession),
        ("deny", RemoteDecision::Deny),
    ] {
        assert_eq!(remote_command(name, &catalog, &all), builtin(Remote::Decide(decision)));
    }
    for desktop in ["context", "tools", "memory", "files", "diff", "terminal", "process"] {
        let found = remote_command(desktop, &catalog, &all);
        assert_eq!(found, Some(RemoteCommand::Desktop), "{desktop}");
    }
    let template = |text: &str| Some(RemoteCommand::Prompt { template: text.into() });
    assert_eq!(remote_command("review", &catalog, &all), template("审查 $ARGUMENTS 的改动"));
    assert_eq!(remote_command("weekly", &catalog, &all), template("写周报"));
    assert_eq!(remote_command("deploy", &catalog, &all), template("部署"));
    assert_eq!(remote_command("off", &catalog, &all), None, "turned off");
    assert_eq!(remote_command("nothing", &catalog, &all), None);
}

#[test]
fn a_command_needs_what_its_plugin_declares() {
    let catalog = catalog();
    let undeclared = |capability| Some(RemoteCommand::Undeclared(capability));
    let sending = [SessionSend];
    for name in [
        "model",
        "permission",
        "stop",
        "compact",
        "switch",
        "new",
        "approve",
        "approve-session",
        "deny",
    ] {
        assert_eq!(remote_command(name, &catalog, &sending), undeclared(SessionControl), "{name}");
    }
    for name in ["sessions", "status"] {
        assert_eq!(remote_command(name, &catalog, &sending), undeclared(SessionRead), "{name}");
    }
    for name in ["plan", "goal", "init", "help"] {
        let found = remote_command(name, &catalog, &sending);
        assert!(matches!(found, Some(RemoteCommand::Builtin(_))), "{name}: {found:?}");
    }
    let review = remote_command("review", &catalog, &sending);
    assert!(matches!(review, Some(RemoteCommand::Prompt { .. })), "{review:?}");
    let reading = [SessionSend, SessionRead];
    assert_eq!(
        remote_command("status", &catalog, &reading),
        Some(RemoteCommand::Builtin(Remote::Status))
    );
    assert_eq!(remote_command("stop", &catalog, &reading), undeclared(SessionControl));
}

#[test]
fn a_prompt_commands_text_goes_where_its_template_says() {
    assert_eq!(expanded("审查 $ARGUMENTS 的改动", "src/lib.rs"), "审查 src/lib.rs 的改动");
    assert_eq!(expanded("先 $ARGUMENTS，再 $ARGUMENTS", "测"), "先 测，再 测");
    assert_eq!(expanded("总结今天的提交", "只看 main"), "总结今天的提交\n\n只看 main");
    assert_eq!(expanded("审查 $ARGUMENTS", ""), "审查");
    assert_eq!(expanded("总结\n", ""), "总结");
}

#[test]
fn the_menu_lists_what_a_chat_can_use() {
    let catalog = catalog();
    let names = |listed: &[MenuCommand]| {
        listed.iter().map(|command| command.name.clone()).collect::<Vec<_>>()
    };
    let declared = [SessionSend, SessionRead, SessionControl];
    let all = menu(&catalog, &declared, false);
    let listed = [
        "plan",
        "goal",
        "init",
        "model",
        "permission",
        "stop",
        "compact",
        "help",
        "sessions",
        "switch",
        "new",
        "status",
        "review",
        "weekly",
    ];
    assert_eq!(names(&all), listed, "a plugin's prompt named like a decision is not listed");
    // While the plugin's chats may decide approvals, the decisions follow Aura's chat commands.
    let deciding = menu(&catalog, &declared, true);
    let mut with_decisions = listed[..12].to_vec();
    with_decisions.extend(["approve", "approve-session", "deny", "review", "weekly"]);
    assert_eq!(names(&deciding), with_decisions);
    let approve = deciding.iter().find(|command| command.name == "approve").expect("approve");
    assert_eq!(
        (approve.title.as_str(), approve.argument_hint.as_deref()),
        ("批准", Some("编号（可选）"))
    );
    assert_eq!(names(&menu(&catalog, &[SessionSend, SessionRead], true)).len(), 8);
    let plan = all.iter().find(|command| command.name == "plan").expect("plan");
    assert_eq!(
        (plan.title.as_str(), plan.argument_hint.as_deref()),
        ("Plan 模式", Some("要做的事"))
    );
    let help = all.iter().find(|command| command.name == "help").expect("help");
    assert_eq!(help.argument_hint, None);
    // A plugin that may only send: what sending allows; one that may also read: the lists too.
    assert_eq!(
        names(&menu(&catalog, &[SessionSend], false)),
        ["plan", "goal", "init", "help", "review", "weekly"]
    );
    assert_eq!(
        names(&menu(&catalog, &[SessionSend, SessionRead], false)),
        ["plan", "goal", "init", "help", "sessions", "status", "review", "weekly"]
    );
    // `/help` names the conversation's workspace's commands too.
    let help = help_text(&catalog, &declared, false);
    for listed in ["/plan 要做的事", "/sessions", "/review 参数", "/weekly", "/deploy"] {
        assert!(help.contains(listed), "{listed}: {help}");
    }
    for missing in ["/context", "/off", "/status 参数", "/approve", "/deny"] {
        assert!(!help.contains(missing), "{missing}: {help}");
    }
    let help = help_text(&catalog, &declared, true);
    for listed in
        ["/approve 编号（可选） — 批准", "/approve-session 编号（可选）", "/deny 编号（可选）"]
    {
        assert!(help.contains(listed), "{listed}: {help}");
    }
    assert!(!help_text(&catalog, &[SessionSend, SessionRead], true).contains("/approve"));
}

#[test]
fn a_command_message_counts_once_for_a_bounded_time() {
    let mut seen = SeenCommands::default();
    let now = Instant::now();
    assert!(seen.first("bot", "m-1", now));
    assert!(!seen.first("bot", "m-1", now));
    assert!(seen.first("other-bot", "m-1", now), "each plugin counts its own");
    let later = now + SEEN_COMMAND_TIME + Duration::from_secs(1);
    assert!(seen.first("bot", "m-1", later), "forgotten after a day");
    for at in 0..SEEN_COMMANDS_MAX {
        assert!(seen.first("many", &format!("m-{at}"), now));
    }
    assert!(!seen.first("many", &format!("m-{}", SEEN_COMMANDS_MAX - 1), now));
    assert!(seen.first("many", "one more", now));
    assert!(seen.first("many", "m-0", now), "the oldest went first");
    seen.forget("many");
    assert!(seen.first("many", "one more", now), "nothing is kept for a plugin gone");
}
