//! An Aura plugin (`aura-plugin.toml`): what it brings, its limits, and the manifests that are
//! refused with every reason.
mod support;
use aura_plugins::{
    DEFAULT_MEMORY_MB, DEFAULT_PROCESSES, PluginAuth, PluginLayout, PluginPackageError,
    PluginTransport,
};
use support::*;

#[test]
fn a_plugin_brings_its_skills_commands_hooks_and_servers() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("review-kit");
    aura_plugin(&root);
    let package = scan(&root).expect("a plugin");
    assert_eq!(package.layout, PluginLayout::Aura);
    assert_eq!(package.manifest.name, "review-kit");
    assert_eq!(package.manifest.version, "1.2.0");
    assert_eq!(package.manifest.author.as_deref(), Some("someone"));
    assert_eq!((package.manifest.limits.memory_mb, package.manifest.limits.processes), (512, 8));
    let skills = package.skill_items();
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name.as_deref(), Some("review-guide"));
    let commands = package.command_items();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "review");
    assert!(commands[0].problem.is_none(), "{commands:?}");
    let hooks = package.hook_items();
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0].definition.as_ref().map(|d| d.name.as_str()), Some("format-check"));
    let counts = package.counts();
    assert_eq!((counts.skills, counts.commands, counts.servers, counts.hooks), (1, 1, 2, 1));
    assert!(package.unsupported.is_empty() && package.warnings.is_empty());
}

#[test]
fn servers_run_under_the_plugins_name_with_its_folder_filled_in() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("review-kit");
    aura_plugin(&root);
    let package = scan(&root).expect("a plugin");
    let files = &package.servers[0];
    assert_eq!(files.id, "review-kit-files");
    assert_eq!(files.display_name, "文件");
    assert!(files.problems.is_empty(), "{:?}", files.problems);
    let PluginTransport::Stdio { command, arguments, environment, secret_environment, .. } =
        &files.transport
    else {
        panic!("a program");
    };
    let folder = root.display().to_string();
    assert_eq!(command, &format!("{folder}\\bin\\server.exe"));
    assert_eq!(arguments, &vec!["--root".to_owned(), folder]);
    assert_eq!(environment, &vec![("LOG_LEVEL".to_owned(), "info".to_owned())]);
    assert_eq!(secret_environment, &vec!["GITHUB_TOKEN".to_owned()]);
    assert_eq!(files.secret_keys(), vec!["GITHUB_TOKEN".to_owned()]);
    let docs = &package.servers[1];
    assert_eq!(docs.id, "review-kit-docs");
    assert_eq!(docs.display_name, "docs");
    assert!(matches!(
        &docs.transport,
        PluginTransport::Http { auth: Some(PluginAuth::OAuth { scopes, .. }), .. } if scopes == &["read"]
    ));
    assert!(docs.secret_keys().is_empty());
}

#[test]
fn the_installations_secret_ids_go_into_the_configuration() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("review-kit");
    aura_plugin(&root);
    let package = scan(&root).expect("a plugin");
    let id = aura_domain_common::CredentialId::new();
    let config = package.servers[0].config(&|key| {
        assert_eq!(key, "GITHUB_TOKEN");
        id
    });
    assert_eq!(config.id, "review-kit-files");
    assert!(!config.enabled && config.trusted_fingerprint.is_none());
    assert_eq!(config.transport.secret_ids(), vec![id]);
}

#[test]
fn limits_default_when_not_declared_and_are_refused_out_of_range() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("plain");
    write(&root, "aura-plugin.toml", "schema_version = 1\nname = \"plain\"\nversion = \"1\"\n");
    write(&root, "skills/review-guide/SKILL.md", SKILL);
    let package = scan(&root).expect("a plugin");
    assert_eq!(
        (package.manifest.limits.memory_mb, package.manifest.limits.processes),
        (DEFAULT_MEMORY_MB, DEFAULT_PROCESSES)
    );
    write(
        &root,
        "aura-plugin.toml",
        "schema_version = 1\nname = \"plain\"\nversion = \"1\"\n[limits]\nmemory_mb = 32\nprocesses = 0\n",
    );
    let problems = problems(scan(&root));
    assert!(
        problems.contains("limits.memory_mb") && problems.contains("limits.processes"),
        "{problems}"
    );
}

#[test]
fn a_manifest_is_refused_with_every_reason() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("bad");
    write(&root, "skills/review-guide/SKILL.md", SKILL);
    write(&root, "aura-plugin.toml", "schema_version = 2\nname = \"Bad_Name\"\nversion = \"\"\n");
    let problems = problems(scan(&root));
    assert!(problems.contains("schema_version"), "{problems}");
    assert!(problems.contains("name（Bad_Name）"), "{problems}");
    assert!(problems.contains("version 不能为空"), "{problems}");
    write(
        &root,
        "aura-plugin.toml",
        "schema_version = 1\nname = \"bad\"\nversion = \"1\"\nruntime = \"node\"\n",
    );
    assert!(problems_of(&root).contains("无法解析"));
}

fn problems_of(root: &std::path::Path) -> String {
    problems(scan(root))
}

#[test]
fn a_folder_without_a_manifest_is_not_a_plugin_and_one_without_content_is_refused() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("empty");
    write(&root, "README.md", "nothing here");
    assert_eq!(scan(&root).expect_err("no manifest"), PluginPackageError::NotAPlugin);
    write(&root, "aura-plugin.toml", "schema_version = 1\nname = \"empty\"\nversion = \"1\"\n");
    assert!(problems_of(&root).contains("没有 Aura 能用的内容"));
}

#[test]
fn a_broken_part_is_shown_and_not_counted() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("review-kit");
    aura_plugin(&root);
    write(
        &root,
        "commands/broken/command.toml",
        "name = \"other\"\ndescription = \"x\"\nversion = 1\n",
    );
    write(&root, "commands/broken/template.md", "text");
    write(
        &root,
        "hooks/no-program/hook.toml",
        "schema_version = 1\nname = \"no-program\"\nversion = \"1\"\nevents = [\"tool.before_call\"]\n[command]\nexecutable = \"missing.exe\"\n",
    );
    let package = scan(&root).expect("still a plugin");
    let commands = package.command_items();
    let broken = commands.iter().find(|c| c.name == "broken").expect("listed");
    assert!(broken.problem.as_deref().is_some_and(|p| p.contains("目录名")), "{broken:?}");
    let hooks = package.hook_items();
    let missing = hooks.iter().find(|h| h.folder == "no-program").expect("listed");
    assert!(missing.problems.iter().any(|p| p.contains("missing.exe")), "{missing:?}");
    let counts = package.counts();
    assert_eq!((counts.commands, counts.hooks), (1, 1));
}

#[test]
fn a_server_list_that_cannot_be_read_makes_the_plugin_unusable() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("review-kit");
    aura_plugin(&root);
    write(
        &root,
        "mcp.json",
        r#"{"servers": [{"name": "x", "transport": {"type": "carrier-pigeon"}}]}"#,
    );
    assert!(problems_of(&root).contains("mcp.json 无法解析"));
}
