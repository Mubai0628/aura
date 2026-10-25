//! Importing: from a folder or a ZIP file; a trusted plugin's newer copy waits as its update.
use super::support::{Fixture, plugin, write};
use serde_json::json;
use std::io::Write;

fn zip_of(folder: &std::path::Path, zip: &std::path::Path, top: &str) {
    let mut writer = zip::ZipWriter::new(std::fs::File::create(zip).expect("zip"));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut stack = vec![folder.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let relative =
                path.strip_prefix(folder).expect("inside").to_string_lossy().replace('\\', "/");
            writer.start_file(format!("{top}/{relative}"), options).expect("file");
            writer.write_all(&std::fs::read(&path).expect("bytes")).expect("write");
        }
    }
    writer.finish().expect("finish");
}

#[test]
fn a_zip_file_imports_like_a_folder() {
    let f = Fixture::new();
    let source = plugin(&f.source(), "review-kit", "1.0.0");
    let zip = f.root.path().join("review-kit-main.zip");
    zip_of(&source, &zip, "review-kit-main");
    let listed = f.import(&zip).expect("imported");
    assert_eq!(listed["plugins"][0]["name"], "review-kit");
    assert_eq!(listed["plugins"][0]["state"], "not_trusted");
    assert!(f.library().join("review-kit/aura-plugin.toml").is_file());
    let leftovers: Vec<_> = std::fs::read_dir(f.runtime.root())
        .expect("plugins folder")
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with('.'))
        .collect();
    assert!(leftovers.is_empty(), "no staging left behind");
}

#[test]
fn a_trusted_plugins_newer_copy_waits_and_the_installed_one_keeps_working() {
    let f = Fixture::new();
    f.import(&plugin(&f.source(), "review-kit", "1.0.0")).expect("imported");
    f.trust("review-kit", false).expect("trusted");
    let newer = f.root.path().join("newer");
    f.import(&plugin(&newer, "review-kit", "2.0.0")).expect("the update");
    let view = f.view("review-kit");
    assert_eq!(view["state"], "ready", "still the trusted copy");
    assert_eq!(view["version"], "1.0.0");
    assert_eq!(view["pending_update"]["version"], "2.0.0");
    let preview = f
        .dispatch("agent.plugins.preview", json!({"name": "review-kit", "pending": true}))
        .expect("the update's preview");
    assert_eq!(preview["pending"], true);
    assert_eq!(preview["skills"][0]["description"], "Guide of 2.0.0");
    let updated = f.trust("review-kit", true).expect("updated");
    let view = &updated["plugins"][0];
    assert_eq!((view["version"].clone(), view["state"].clone()), (json!("2.0.0"), json!("ready")));
    assert!(view["pending_update"].is_null());
    let saved = f.saved("review-kit").expect("saved");
    assert!(saved.pending.is_none());
    assert_eq!(
        saved.imported_from.as_deref(),
        Some(newer.join("review-kit-source").display().to_string().as_str())
    );
    assert!(!f.runtime.pending_folder("review-kit").exists());
}

#[test]
fn discarding_an_update_keeps_the_installed_copy() {
    let f = Fixture::new();
    f.import(&plugin(&f.source(), "review-kit", "1.0.0")).expect("imported");
    f.trust("review-kit", false).expect("trusted");
    f.import(&plugin(&f.root.path().join("newer"), "review-kit", "2.0.0")).expect("the update");
    let kept = f.change("agent.plugins.discard_update", "review-kit").expect("discarded");
    assert!(kept["plugins"][0]["pending_update"].is_null());
    assert_eq!(kept["plugins"][0]["version"], "1.0.0");
    assert!(!f.runtime.pending_folder("review-kit").exists());
    let again = f.change("agent.plugins.discard_update", "review-kit");
    assert!(again.expect_err("nothing waits").to_string().starts_with("plugins.no_update"));
}

#[test]
fn an_untrusted_plugin_imported_again_is_simply_replaced() {
    let f = Fixture::new();
    f.import(&plugin(&f.source(), "review-kit", "1.0.0")).expect("imported");
    f.import(&plugin(&f.root.path().join("newer"), "review-kit", "2.0.0")).expect("replaced");
    let view = f.view("review-kit");
    assert_eq!(view["version"], "2.0.0");
    assert!(view["pending_update"].is_null());
}

#[test]
fn a_folder_that_is_not_a_plugin_is_refused_with_why_and_leaves_nothing() {
    let f = Fixture::new();
    let folder = f.source().join("not-a-plugin");
    write(&folder.join("readme.md"), "hello");
    let refused = f.import(&folder).expect_err("not a plugin").to_string();
    assert!(refused.starts_with("plugins.invalid") && refused.contains("不是插件"), "{refused}");
    assert!(f.list()["plugins"].as_array().expect("plugins").is_empty());
    let leftovers = std::fs::read_dir(f.runtime.root()).map(|e| e.count()).unwrap_or(0);
    assert_eq!(leftovers, 1, "only the empty packages folder");
}
