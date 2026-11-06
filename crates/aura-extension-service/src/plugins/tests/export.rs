//! Exporting an installed plugin as a ZIP file at the place Main asked the user for: exactly the
//! files the plugin page read, and the export imports again as the same plugin.
use super::support::{Fixture, plugin};
use serde_json::json;

fn export(f: &Fixture, name: &str, path: &str) -> anyhow::Result<serde_json::Value> {
    f.dispatch("agent.plugins.export", json!({"name": name, "path": path}))
}

#[test]
fn an_installed_plugin_is_exported_and_imports_again_as_the_same_plugin() {
    let f = Fixture::new();
    f.import(&plugin(&f.source(), "review-kit", "1.0.0")).expect("imported");
    let destination = f.root.path().join("review-kit-1.0.0.zip");
    let exported = export(&f, "review-kit", &destination.display().to_string()).expect("exported");
    assert_eq!(exported, json!({"completed": true, "message": "已导出 6 个文件"}));
    let installed = f.view("review-kit");
    assert_eq!(installed["state"], "not_trusted", "exporting changes nothing");

    // Imported into another Aura it is the same content.
    let other = Fixture::new();
    other.import(&destination).expect("imported again");
    assert_eq!(other.view("review-kit")["fingerprint"], installed["fingerprint"]);
}

#[test]
fn an_export_that_cannot_be_written_says_why() {
    let f = Fixture::new();
    f.import(&plugin(&f.source(), "review-kit", "1.0.0")).expect("imported");
    let inside = f.library().join("review-kit").join("review-kit.zip");
    for (name, path, why) in [
        ("missing", f.root.path().join("x.zip").display().to_string(), "plugins.not_found"),
        ("review-kit", "review-kit.zip".to_owned(), "plugins.export_failed"),
        ("review-kit", f.root.path().join("x.txt").display().to_string(), "只能导出成 .zip"),
        ("review-kit", inside.display().to_string(), "插件自己的文件夹"),
    ] {
        let refused = export(&f, name, &path).expect_err(&path).to_string();
        assert!(refused.contains(why), "{path}: {refused}");
    }
    assert!(!inside.exists());
}
