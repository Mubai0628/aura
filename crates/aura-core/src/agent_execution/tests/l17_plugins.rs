//! Writing a plugin with the user through the real Agent admission, policy, execution and result
//! path: a workspace folder is checked, installed in development where it is and never trusted
//! by the model; ReadOnly refuses the install; a plugin's errors are read with its secrets
//! hidden. Only the Provider output is a fixture.
use super::*;
use serde_json::Value;
use std::path::Path;

const NAMES: &[&str] =
    &["plugins.validate", "plugins.dev_install", "plugins.logs", "normal.complete"];

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
    std::fs::write(path, text).expect("file");
}

/// A plugin with a panel and one Skill that cannot be read, in the workspace's `plugins` folder.
fn usage_panel(root: &Path) {
    let folder = root.join("plugins").join("usage-panel");
    write(
        &folder.join("aura-plugin.toml"),
        "schema_version = 2\nname = \"usage-panel\"\nversion = \"0.1.0\"\ndescription = \"本会话用量\"\ncapabilities = [\"session.read\"]\n\n[panel]\ntitle = \"本会话用量\"\n",
    );
    write(&folder.join("panel/index.html"), "<!doctype html><title>用量</title>");
    write(&folder.join("skills/broken/SKILL.md"), "no frontmatter\n");
}

async fn call(f: &mut Fixture, name: &str, arguments: Value) -> (ToolCallId, ToolResultV1) {
    let batch = f.admit(&[(name, arguments)]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    let stored = {
        let store = f.service().broker.lock().expect("journal");
        aura_persistence_v2::tool::execution::call(store.database.connection(), id)
            .expect("call exists")
            .result
            .expect("terminal result")
    };
    f.continue_after_tools(batch_id, &group);
    (id, stored)
}

fn refusal(result: &ToolResultV1) -> String {
    match &result.outcome {
        ToolResultOutcomeV1::Failed { error }
        | ToolResultOutcomeV1::OperationRejected { error }
        | ToolResultOutcomeV1::NotExecuted { error: Some(error), .. } => error.message.to_string(),
        other => panic!("the call was not refused: {other:?}"),
    }
}

async fn plugins(f: &Fixture) -> Value {
    crate::r03_tests::success(&f.runtime, "agent.plugins.list", json!({})).await
}

#[tokio::test]
async fn l17_a_workspace_folder_is_checked_and_installed_in_development_and_never_trusted() {
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, NAMES).await;
    usage_panel(&f.root);

    let (checked, _) =
        call(&mut f, "plugins.validate", json!({"path": "plugins/usage-panel"})).await;
    let checked = f.output(checked);
    assert_eq!(checked["name"], "usage-panel");
    assert_eq!(checked["installable"], true);
    assert_eq!(checked["brings"], json!({"panels": 1}), "the broken Skill is not counted");
    let problems = checked["problems"].to_string();
    assert!(problems.contains("Skill skills/broken"), "{problems}");

    let (installed, result) =
        call(&mut f, "plugins.dev_install", json!({"path": "plugins/usage-panel"})).await;
    assert!(matches!(result.outcome, ToolResultOutcomeV1::Succeeded), "asked nothing: {result:?}");
    let installed = f.output(installed);
    assert_eq!(installed["installed"], true);
    assert_eq!(installed["state"], "not_trusted");
    let folder = aura_domain_common::display_path(
        &std::fs::canonicalize(f.root.join("plugins").join("usage-panel")).expect("folder"),
    );
    assert_eq!(installed["folder"], folder.as_str());
    assert!(installed["next"].as_str().expect("next").contains("设置 → 插件"), "{installed}");

    let listed = plugins(&f).await;
    let plugin = &listed["plugins"][0];
    assert_eq!(
        (plugin["name"].clone(), plugin["state"].clone()),
        (json!("usage-panel"), json!("not_trusted"))
    );
    assert_eq!(plugin["development"], folder.as_str());
    assert!(plugin["trusted_fingerprint"].is_null(), "the model never trusts it");

    let (again, _) =
        call(&mut f, "plugins.dev_install", json!({"path": "plugins/usage-panel"})).await;
    assert_eq!(f.output(again)["installed"], true, "the same folder again changes nothing");
    assert_eq!(plugins(&f).await["revision"], listed["revision"]);

    for outside in ["../elsewhere", "C:\\Windows", "plugins/missing"] {
        let (_, refused) = call(&mut f, "plugins.dev_install", json!({"path": outside})).await;
        assert!(refusal(&refused).starts_with("plugins.path_invalid"), "{outside}: {refused:?}");
    }
}

#[tokio::test]
async fn l17_a_plugins_service_channels_and_theme_are_counted_in_what_it_brings() {
    let mut f = Fixture::with_exposure(PermissionMode::ReadOnly, NAMES).await;
    let folder = f.root.join("plugins").join("chat-bot");
    write(
        &folder.join("aura-plugin.toml"),
        "schema_version = 2\nname = \"chat-bot\"\nversion = \"0.1.0\"\ndescription = \"测试机器人\"\n\
         capabilities = [\"session.read\", \"session.send\"]\n\n\
         [service]\ncommand = \"node\"\narguments = [\"${PLUGIN_ROOT}/service.mjs\"]\n\n\
         [[channels]]\nid = \"qq-main\"\nplatform = \"qq\"\ntitle = \"QQ 机器人\"\n",
    );
    write(&folder.join("service.mjs"), "process.stdin.resume();\n");
    write(&folder.join("theme.toml"), "title = \"夜间\"\n[radius]\nmd = 4\n");
    let (checked, _) = call(&mut f, "plugins.validate", json!({"path": "plugins/chat-bot"})).await;
    let checked = f.output(checked);
    assert_eq!(checked["installable"], true, "{checked}");
    assert_eq!(checked["brings"], json!({"services": 1, "channels": 1, "themes": 1}));
}

#[tokio::test]
async fn l17_read_only_checks_a_plugin_and_refuses_installing_it_without_asking() {
    let mut f = Fixture::with_exposure(PermissionMode::ReadOnly, NAMES).await;
    usage_panel(&f.root);
    let (checked, _) =
        call(&mut f, "plugins.validate", json!({"path": "plugins/usage-panel"})).await;
    assert_eq!(f.output(checked)["installable"], true);
    let batch = f.admit(&[("plugins.dev_install", json!({"path": "plugins/usage-panel"}))]);
    assert!(
        !matches!(f.execute_rejecting(batch).await, Some(BatchOutcome::AwaitingApproval { .. })),
        "ReadOnly refuses; it never asks"
    );
    assert_eq!(plugins(&f).await["plugins"], json!([]), "nothing was installed");
}

#[tokio::test]
async fn l17_the_plugin_writing_skill_comes_with_aura_and_is_loaded_by_name() {
    let names = &["skills.search", "skills.load", "fs.read", "normal.complete"];
    let mut f = Fixture::with_exposure(PermissionMode::ReadOnly, names).await;
    f.service().skills.include_builtin_skills();
    let (found, _) = call(&mut f, "skills.search", json!({"query": "插件"})).await;
    assert_eq!(f.output(found)["skills"][0]["name"], "write-aura-plugin");
    let (loaded, _) = call(&mut f, "skills.load", json!({"name": "write-aura-plugin"})).await;
    let loaded = f.output(loaded);
    assert_eq!(
        loaded["files"],
        json!([
            "examples/fake-chat/README.md",
            "examples/fake-chat/platform/fake_platform.py",
            "examples/fake-chat/plugin/aura-plugin.toml",
            "examples/fake-chat/plugin/service/main.py",
            "reference/channels.md",
            "reference/manifest.md",
            "reference/panel.md",
            "reference/parts.md",
            "reference/service.md",
            "reference/theme.md",
            "sdk/node/aura-plugin.mjs",
            "sdk/python/aura_plugin.py"
        ])
    );
    let instructions = loaded["instructions"].as_str().expect("instructions");
    assert!(instructions.contains("plugins.dev_install"), "{loaded}");
    assert!(instructions.contains("设置 → 插件"), "the user trusts it, not the model");
    // Its reference files are read like any usable Skill's, without asking.
    let manifest =
        Path::new(loaded["folder"].as_str().expect("folder")).join("reference").join("manifest.md");
    let (read, result) = call(
        &mut f,
        "fs.read",
        json!({"path": manifest, "range": {"range_kind": "full"}, "explicit_encoding": null}),
    )
    .await;
    assert!(matches!(result.outcome, ToolResultOutcomeV1::Succeeded), "{result:?}");
    assert!(f.output(read)["text"].as_str().expect("text").contains("schema_version = 2"));
}

#[tokio::test]
async fn l17_a_plugins_errors_are_read_by_name() {
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, NAMES).await;
    usage_panel(&f.root);
    call(&mut f, "plugins.dev_install", json!({"path": "plugins/usage-panel"})).await;
    f.service().plugins.log(
        "usage-panel",
        aura_agent_domain::PluginLogSource::Panel,
        "本会话用量",
        "TypeError: auraPanel.state is not a function",
    );
    let (read, _) = call(&mut f, "plugins.logs", json!({"name": "usage-panel"})).await;
    let read = f.output(read);
    assert_eq!(read["entries"][0]["source"], "panel");
    assert!(read["entries"][0]["text"].as_str().expect("text").contains("TypeError"));
    let (_, refused) = call(&mut f, "plugins.logs", json!({"name": "absent"})).await;
    assert!(refusal(&refused).starts_with("plugins.not_found"), "{refused:?}");
}
