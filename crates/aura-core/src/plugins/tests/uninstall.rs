//! Uninstalling, and finishing or rolling back a change Core was stopped in the middle of.
use super::support::{Fixture, plugin, write};
use std::os::windows::fs::OpenOptionsExt;

#[test]
fn uninstall_removes_the_folders_and_the_choices() {
    let f = Fixture::new();
    f.import(&plugin(&f.source(), "review-kit", "1.0.0")).expect("imported");
    f.trust("review-kit", false).expect("trusted");
    f.import(&plugin(&f.root.path().join("newer"), "review-kit", "2.0.0")).expect("the update");
    let after = f.change("agent.plugins.uninstall", "review-kit").expect("uninstalled");
    assert!(after["plugins"].as_array().expect("plugins").is_empty());
    assert!(f.saved("review-kit").is_none());
    assert!(!f.library().join("review-kit").exists());
    assert!(!f.runtime.pending_folder("review-kit").exists(), "the waiting copy goes too");
}

#[test]
fn a_folder_another_program_holds_stops_the_uninstall_before_anything_changes() {
    let f = Fixture::new();
    f.import(&plugin(&f.source(), "review-kit", "1.0.0")).expect("imported");
    f.trust("review-kit", false).expect("trusted");
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0x1)
        .open(f.library().join("review-kit/skills/guide/SKILL.md"))
        .expect("held like an editor holds it");
    let refused = f.change("agent.plugins.uninstall", "review-kit").expect_err("busy").to_string();
    assert!(
        refused.starts_with("plugins.folder_busy") && refused.contains("没有改变"),
        "{refused}"
    );
    assert_eq!(f.view("review-kit")["state"], "ready");
    assert!(f.saved("review-kit").is_some());
    drop(held);
    f.change("agent.plugins.uninstall", "review-kit").expect("uninstalled once released");
    assert!(!f.listed("review-kit"));
}

#[test]
fn an_update_stopped_after_the_installed_copy_moved_is_rolled_back_at_start() {
    let mut f = Fixture::new();
    f.import(&plugin(&f.source(), "review-kit", "1.0.0")).expect("imported");
    f.trust("review-kit", false).expect("trusted");
    let previous = f.runtime.root().join(".removed-crashed");
    std::fs::rename(f.library().join("review-kit"), &previous).expect("moved as the update does");
    write(
        &f.runtime.root().join("journal.json"),
        &serde_json::json!({"operation": "update", "name": "review-kit", "previous": previous})
            .to_string(),
    );
    f.restart();
    assert_eq!(f.view("review-kit")["state"], "ready", "the installed copy is back");
    assert!(!previous.exists() && !f.runtime.root().join("journal.json").exists());
}

#[test]
fn an_uninstall_stopped_before_plugins_json_changed_is_rolled_back_at_start() {
    let mut f = Fixture::new();
    f.import(&plugin(&f.source(), "review-kit", "1.0.0")).expect("imported");
    f.trust("review-kit", false).expect("trusted");
    let moved = f.runtime.root().join(".removed-installed");
    std::fs::rename(f.library().join("review-kit"), &moved).expect("moved as uninstall does");
    write(
        &f.runtime.root().join("journal.json"),
        &serde_json::json!({"operation": "uninstall", "name": "review-kit",
            "folders": [moved, f.runtime.root().join(".removed-waiting")], "servers": [], "secrets": []})
        .to_string(),
    );
    f.restart();
    assert_eq!(f.view("review-kit")["state"], "ready", "nothing was removed");
}

#[test]
fn an_uninstall_stopped_after_plugins_json_changed_is_finished_at_start() {
    let mut f = Fixture::new();
    f.import(&plugin(&f.source(), "review-kit", "1.0.0")).expect("imported");
    let moved = f.runtime.root().join(".removed-installed");
    std::fs::rename(f.library().join("review-kit"), &moved).expect("moved as uninstall does");
    f.settings.replace_plugin_registry(f.revision(), &[]).expect("plugins.json written");
    write(
        &f.runtime.root().join("journal.json"),
        &serde_json::json!({"operation": "uninstall", "name": "review-kit",
            "folders": [moved], "servers": [], "secrets": []})
        .to_string(),
    );
    f.restart();
    assert!(!f.listed("review-kit"));
    assert!(!moved.exists(), "the moved folder is removed");
}

#[test]
fn staging_and_removed_leftovers_are_swept_at_start() {
    let mut f = Fixture::new();
    for leftover in [".staging-a/x/aura-plugin.toml", ".removed-b/y.txt"] {
        write(&f.runtime.root().join(leftover), "left");
    }
    f.restart();
    let names: Vec<String> = std::fs::read_dir(f.runtime.root())
        .expect("plugins folder")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|name| !name.starts_with('.')), "{names:?}");
}
