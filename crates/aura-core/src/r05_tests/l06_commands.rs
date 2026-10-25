//! Settings → 命令 through the real authenticated routes: the user's commands live in
//! the fixture's own configuration directory and the workspace's in its `.aura/commands`.
use super::*;

fn command(root: &Path, name: &str) {
    let directory = root.join(name);
    fs::create_dir_all(&directory).expect("command directory");
    fs::write(
        directory.join("command.toml"),
        format!("name = \"{name}\"\ndescription = \"处理 {name}\"\nversion = 1\n"),
    )
    .expect("manifest");
    fs::write(directory.join("template.md"), format!("请处理 $ARGUMENTS（{name}）\n"))
        .expect("template");
}

fn entry(catalog: &Value, source: &str, name: &str) -> Value {
    catalog["commands"]
        .as_array()
        .expect("commands")
        .iter()
        .find(|e| e["source"] == source && e["name"] == name)
        .cloned()
        .unwrap_or_else(|| panic!("{source}/{name} in {catalog}"))
}

#[tokio::test]
async fn the_catalog_reads_my_and_the_workspace_commands_from_their_own_folders() {
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let (root, w, _) = workspace(&fixture, &state).await;
    let user_root = fixture.root.path().join("config").join("commands");
    command(&user_root, "review");
    command(&user_root, "notes");
    command(&root.join(".aura").join("commands"), "review");
    let catalog =
        success(&state, "agent.commands.catalog", json!({"workspace_id": w.workspace_id})).await;
    assert_eq!(
        catalog["user_directory"],
        json!(user_root.display().to_string()),
        "the user's commands are read from the fixture's configuration, never the real profile"
    );
    assert_eq!(entry(&catalog, "user", "review")["issue"], "被当前工作区的同名命令覆盖");
    assert_eq!(entry(&catalog, "workspace", "review")["issue"], Value::Null);
    assert_eq!(entry(&catalog, "user", "notes")["template"], "请处理 $ARGUMENTS（notes）\n");
    assert_eq!(entry(&catalog, "builtin", "init")["kind"], "block");
    let mine = success(&state, "agent.commands.catalog", json!({"workspace_id": null})).await;
    assert!(
        mine["commands"].as_array().expect("commands").iter().all(|e| e["source"] != "workspace")
    );
    assert_eq!(mine["workspace_directory"], Value::Null);
    assert_eq!(entry(&mine, "user", "review")["issue"], Value::Null, "no workspace, no shadow");
}

/// The workspace's folders were named as Core holds them, in Windows' verbatim spelling
/// (`\\?\C:\…`). People read that, and the shell does not open it: "打开命令目录" failed.
#[tokio::test]
async fn the_catalog_names_folders_in_their_ordinary_spelling() {
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let (root, w, _) = workspace(&fixture, &state).await;
    command(&root.join(".aura").join("commands"), "review");
    let catalog =
        success(&state, "agent.commands.catalog", json!({"workspace_id": w.workspace_id})).await;
    let folder = catalog["workspace_directory"].as_str().expect("workspace folder").to_owned();
    let location = entry(&catalog, "workspace", "review")["location"]
        .as_str()
        .expect("command folder")
        .to_owned();
    for path in [&folder, &location] {
        assert!(!path.starts_with(r"\\?\"), "{path}");
        assert!(Path::new(path).is_dir(), "{path} names the folder");
    }
    assert!(folder.ends_with(r".aura\commands"), "{folder}");
}

#[tokio::test]
async fn turning_a_command_off_is_kept_per_workspace_across_a_restart() {
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let (root, w, _) = workspace(&fixture, &state).await;
    command(&root.join(".aura").join("commands"), "deploy");
    let off = success(
        &state,
        "agent.commands.set_enabled",
        json!({"workspace_id": w.workspace_id, "name": "deploy", "source": "workspace", "enabled": false}),
    )
    .await;
    assert_eq!(entry(&off, "workspace", "deploy")["enabled"], false);
    let state = fixture.restart(state).await;
    let again =
        success(&state, "agent.commands.catalog", json!({"workspace_id": w.workspace_id})).await;
    assert_eq!(entry(&again, "workspace", "deploy")["enabled"], false);
    for (params, code) in [
        (
            json!({"workspace_id": null, "name": "help", "source": "builtin", "enabled": false}),
            "commands.builtin_always_enabled",
        ),
        (
            json!({"workspace_id": null, "name": "missing", "source": "user", "enabled": false}),
            "commands.not_found",
        ),
        (
            json!({"workspace_id": null, "name": "deploy", "source": "workspace", "enabled": true}),
            "commands.workspace_required",
        ),
    ] {
        let refused = request(&state, "agent.commands.set_enabled", params).await;
        assert!(refused.to_string().contains(code), "{refused}");
    }
    let on = success(
        &state,
        "agent.commands.set_enabled",
        json!({"workspace_id": w.workspace_id, "name": "deploy", "source": "workspace", "enabled": true}),
    )
    .await;
    assert_eq!(entry(&on, "workspace", "deploy")["enabled"], true);
}

#[tokio::test]
async fn only_my_own_command_is_removed_and_only_at_the_revision_shown() {
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let user_root = fixture.root.path().join("config").join("commands");
    command(&user_root, "review");
    let shown = success(&state, "agent.commands.catalog", json!({"workspace_id": null})).await;
    let revision = entry(&shown, "user", "review")["revision"].clone();
    fs::write(user_root.join("review").join("template.md"), "改过的模板 $ARGUMENTS\n")
        .expect("edited after the user looked");
    let stale = request(
        &state,
        "agent.commands.delete",
        json!({"workspace_id": null, "name": "review", "revision": revision}),
    )
    .await;
    assert!(stale.to_string().contains("commands.revision_changed"), "{stale}");
    assert!(user_root.join("review").exists());
    let current = success(&state, "agent.commands.catalog", json!({"workspace_id": null})).await;
    let removed = success(
        &state,
        "agent.commands.delete",
        json!({"workspace_id": null, "name": "review", "revision": entry(&current, "user", "review")["revision"]}),
    )
    .await;
    assert!(!user_root.join("review").exists());
    assert!(
        removed["commands"].as_array().expect("commands").iter().all(|e| e["name"] != "review")
    );
}
