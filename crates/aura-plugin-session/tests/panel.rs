//! What Main is given to show a plugin's panel: the title, the folder its page is served from,
//! the first page, the origins the page may reach and what the plugin declared. A plugin without
//! a panel, or whose panel cannot be used, gets the reason instead.
use aura_plugin_session::panel_site;
use aura_plugins::scan_plugin;
use aura_skills::FileHashCache;
use std::path::Path;

fn plugin(root: &Path, manifest: &str, files: &[&str]) -> aura_plugins::PluginPackage {
    std::fs::write(root.join("aura-plugin.toml"), manifest).expect("manifest");
    for file in files {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
        std::fs::write(path, "<!doctype html><title>用量</title>").expect("file");
    }
    scan_plugin(root, &FileHashCache::default()).expect("scanned")
}

const HEAD: &str = "schema_version = 2\nname = \"usage-panel\"\nversion = \"0.1.0\"\n\
                    description = \"本会话用量\"\n";

#[test]
fn a_usable_panel_is_given_with_its_folder_page_origins_and_capabilities() {
    let folder = tempfile::tempdir().expect("folder");
    let package = plugin(
        folder.path(),
        &format!(
            "{HEAD}capabilities = [\"session.read\", \"session.events\"]\n\n[panel]\n\
             title = \"本会话用量\"\nentry = \"panel/app/main.html\"\n\
             network = [\"https://api.example.com\"]\n"
        ),
        &["panel/app/main.html"],
    );
    let site = panel_site(&package).expect("shown");
    assert_eq!(site.title, "本会话用量");
    assert_eq!(site.entry, "panel/app/main.html");
    assert_eq!(site.network, ["https://api.example.com"]);
    assert_eq!(site.capabilities, ["session.read", "session.events"]);
    assert_eq!(Path::new(&site.folder), package.root.as_path());
    assert!(!site.folder.starts_with(r"\\?\"), "{}", site.folder);
}

#[test]
fn a_plugin_without_a_usable_panel_is_told_why() {
    // A plugin needs something Aura can use: here a Skill and no panel.
    let none = tempfile::tempdir().expect("folder");
    let skill = none.path().join("skills").join("hello");
    std::fs::create_dir_all(&skill).expect("skill folder");
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: hello\ndescription: 打个招呼\n---\n说你好。\n",
    )
    .expect("skill");
    let package = plugin(none.path(), HEAD, &[]);
    let refused = panel_site(&package).expect_err("no panel").to_string();
    assert!(refused.starts_with("plugin_session.panel_missing"), "{refused}");
    assert!(refused.contains("usage-panel"), "{refused}");

    // The page it names is not there.
    let broken = tempfile::tempdir().expect("folder");
    let package = plugin(
        broken.path(),
        &format!("{HEAD}\n[panel]\ntitle = \"本会话用量\"\nentry = \"panel/index.html\"\n"),
        &["panel/other.html"],
    );
    let refused = panel_site(&package).expect_err("unusable").to_string();
    assert!(refused.starts_with("plugin_session.panel_unusable"), "{refused}");
    assert!(refused.contains("panel/index.html"), "{refused}");
}
