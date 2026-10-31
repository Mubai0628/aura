//! A ready plugin's commands follow the built-in, the user's and the workspace's, lose a
//! name another command already offers, and are switched only with their plugin.
use super::CommandContext;
use aura_agent_domain::{CommandSetEnabledRequest, CommandSource};
use aura_config::{AuraPaths, SettingsFacade};
use aura_extension_service::plugins::{PluginContext, PluginRuntime};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
    std::fs::write(path, text).expect("file");
}

fn command(folder: &Path, name: &str) {
    write(
        &folder.join(name).join("command.toml"),
        &format!("name = \"{name}\"\ndescription = \"{name} 命令\"\nversion = 1\n"),
    );
    write(&folder.join(name).join("template.md"), "做 $ARGUMENTS\n");
}

#[test]
fn a_ready_plugins_commands_come_last_and_lose_a_taken_name() {
    let root = tempfile::tempdir().expect("temporary root");
    let paths = AuraPaths::from_roots(root.path().join("config"), root.path().join("local"));
    let settings = SettingsFacade::open(paths).expect("temporary settings");
    let user_root = settings.paths().config_root.join("commands");
    command(&user_root, "review");
    let source = root.path().join("sources").join("kit");
    write(
        &source.join("aura-plugin.toml"),
        "schema_version = 1\nname = \"kit\"\nversion = \"1\"\n",
    );
    command(&source.join("commands"), "review");
    command(&source.join("commands"), "summarize");
    let plugins = PluginRuntime::new(Arc::clone(&settings), None);
    let manage = PluginContext::new(Arc::clone(&plugins));
    let revision =
        || manage.dispatch("agent.plugins.list", json!({})).expect("list")["revision"].clone();
    manage
        .dispatch(
            "agent.plugins.import",
            json!({"path": source.display().to_string(), "expected_revision": revision()}),
        )
        .expect("imported");
    let preview =
        manage.dispatch("agent.plugins.preview", json!({"name": "kit"})).expect("preview");
    manage
        .dispatch(
            "agent.plugins.trust",
            json!({"name": "kit", "expected_fingerprint": preview["fingerprint"], "expected_revision": revision()}),
        )
        .expect("trusted");
    let context = CommandContext {
        user_root,
        settings: Arc::clone(&settings),
        workspace: None,
        plugins: Some(Arc::clone(&plugins)),
    };
    let catalog = context.catalog(None).expect("catalog");
    let from_plugin: Vec<_> =
        catalog.commands.iter().filter(|c| c.plugin.as_deref() == Some("kit")).collect();
    assert_eq!(from_plugin.len(), 2);
    assert!(from_plugin.iter().all(|c| c.source == CommandSource::Plugin));
    let summarize = from_plugin.iter().find(|c| c.name == "summarize").expect("summarize");
    assert!(summarize.runnable(), "{summarize:?}");
    let review = from_plugin.iter().find(|c| c.name == "review").expect("review");
    assert_eq!(review.issue.as_deref(), Some("与已有的 /review 重名，未启用"));
    let mine =
        catalog.commands.iter().find(|c| c.name == "review" && c.source == CommandSource::User);
    assert!(mine.is_some_and(aura_agent_domain::CommandEntry::runnable), "the user's own keeps it");
    let switch = |enabled: bool| {
        context
            .set_enabled(CommandSetEnabledRequest {
                workspace_id: None,
                name: "summarize".into(),
                source: CommandSource::Plugin,
                enabled,
            })
            .expect("switched")
    };
    let off = switch(false);
    let summarize = |catalog: &aura_agent_domain::CommandCatalog| {
        catalog.commands.iter().find(|c| c.name == "summarize").map(|c| c.enabled)
    };
    assert_eq!(summarize(&off), Some(false), "listed, off");
    assert_eq!(summarize(&switch(true)), Some(true));
}
