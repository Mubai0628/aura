//! A ready plugin's Hooks run after the user's own, under the plugin's limits, and follow
//! the plugin; the settings list shows them as the plugin's.
use super::super::*;
use super::support::Fixture;
use crate::plugins::{PluginContext, PluginRuntime};
use serde_json::json;
use std::path::Path;

const BEFORE: &str = "tool.before_call";

fn copy_folder(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("folder");
    for entry in std::fs::read_dir(from).expect("read") {
        let path = entry.expect("entry").path();
        let target = to.join(path.file_name().expect("name"));
        if path.is_dir() {
            copy_folder(&path, &target);
        } else {
            std::fs::copy(&path, &target).expect("copy");
        }
    }
}

/// A plugin `guard` whose one Hook answers `answer`, imported and trusted.
fn install(f: &Fixture, answer: &str, limits: &str) -> PluginContext {
    let hook = f.package("guard-hook", &[BEFORE], &["answer", answer], "");
    let source = f.root.path().join("sources").join("guard");
    std::fs::create_dir_all(&source).expect("source");
    std::fs::write(
        source.join("aura-plugin.toml"),
        format!("schema_version = 1\nname = \"guard\"\nversion = \"1\"\n{limits}\n"),
    )
    .expect("manifest");
    copy_folder(&hook, &source.join("hooks").join("guard-hook"));
    let plugins = PluginRuntime::new(Arc::clone(&f.runner.settings), None);
    let context = PluginContext::new(plugins);
    let revision = |context: &PluginContext| {
        context.dispatch("agent.plugins.list", json!({})).expect("list")["revision"].clone()
    };
    context
        .dispatch(
            "agent.plugins.import",
            json!({"path": source.display().to_string(), "expected_revision": revision(&context)}),
        )
        .expect("imported");
    let preview =
        context.dispatch("agent.plugins.preview", json!({"name": "guard"})).expect("preview");
    context
        .dispatch(
            "agent.plugins.trust",
            json!({"name": "guard", "expected_fingerprint": preview["fingerprint"], "expected_revision": revision(&context)}),
        )
        .expect("trusted");
    context
}

fn runtime(f: &Fixture) -> Arc<PluginRuntime> {
    PluginRuntime::new(Arc::clone(&f.runner.settings), None)
}

#[test]
fn a_plugins_hook_runs_after_the_users_own_and_follows_the_plugin() {
    let mut f = Fixture::new();
    let mine = f.package("mine", &[BEFORE], &["answer", r#"{"decision":"continue"}"#], "");
    f.trust(&mine);
    let context = install(&f, r#"{"decision":"block","reason":"the plugin says no"}"#, "");
    let plugins = runtime(&f);
    f.runner.plugins = Some(Arc::clone(&plugins));
    let outcome =
        f.run(f.call(HookEvent::ToolBeforeCall, Some("fs.write"), json!({"arguments": {}})));
    let block = outcome.block.expect("blocked by the plugin's Hook");
    assert_eq!(
        (block.hook_name.as_str(), block.reason.as_str()),
        ("guard-hook", "the plugin says no")
    );
    let names: Vec<&str> = outcome.invocations.iter().map(|i| i.hook_name.as_str()).collect();
    assert_eq!(names, ["mine", "guard-hook"], "the user's own first");
    assert!(outcome.invocations[1].path.starts_with(&plugins.library().display().to_string()));
    let listed = registry::plugin_entry_views(&plugins);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].plugin.as_deref(), Some("guard"));
    assert_eq!(listed[0].state, aura_hooks::HookEntryState::Ready);
    let revision =
        context.dispatch("agent.plugins.list", json!({})).expect("list")["revision"].clone();
    context
        .dispatch("agent.plugins.untrust", json!({"name": "guard", "expected_revision": revision}))
        .expect("untrusted");
    plugins.changed();
    let outcome =
        f.run(f.call(HookEvent::ToolBeforeCall, Some("fs.write"), json!({"arguments": {}})));
    assert!(outcome.block.is_none());
    assert_eq!(outcome.invocations.len(), 1, "only the user's own");
}

#[test]
fn a_plugins_hook_runs_under_the_plugins_process_limit() {
    let mut f = Fixture::new();
    let _context = install(&f, r#"{"decision":"continue"}"#, "[limits]\nprocesses = 1");
    let plugins = runtime(&f);
    f.runner.plugins = Some(Arc::clone(&plugins));
    let ready = plugins.ready();
    let limits = ready[0].job_limits();
    assert_eq!(limits.active_processes, 1);
    let candidates =
        registry::plugin_candidates(&plugins, HookEvent::ToolBeforeCall, Some("fs.write"));
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].limits, Some(limits));
    let outcome =
        f.run(f.call(HookEvent::ToolBeforeCall, Some("fs.write"), json!({"arguments": {}})));
    assert_eq!(outcome.invocations.len(), 1);
    assert_eq!(
        outcome.invocations[0].outcome,
        aura_hooks::HookOutcome::Continued,
        "one process is enough"
    );
}
