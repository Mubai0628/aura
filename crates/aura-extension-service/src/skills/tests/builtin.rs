//! The Skills that come with Aura: listed as Aura's own, usable without trust and not pinned;
//! written again at each start where they differ from Aura's copy; switched off but never
//! removed.
use super::super::{SkillRuntime, loading};
use super::support::{Fixture, write};
use serde_json::json;
use std::path::PathBuf;

const NAME: &str = "write-aura-plugin";

fn folder(f: &Fixture) -> PathBuf {
    PathBuf::from(f.view(NAME)["path"].as_str().expect("path"))
}

#[test]
fn the_plugin_writing_skill_comes_with_aura_usable_unpinned_and_loads_with_its_files() {
    let f = Fixture::new();
    assert!(
        f.list()["skills"].as_array().expect("skills").is_empty(),
        "a test lists only what it wrote unless it asks for Aura's own"
    );
    f.runtime.include_builtin_skills();
    let skill = f.view(NAME);
    assert_eq!(skill["source"], "aura_builtin");
    assert_eq!(skill["state"], "ready");
    assert_eq!(skill["pinned"], false);
    let roots = f.list()["roots"].clone();
    assert!(
        roots.as_array().expect("roots").iter().any(|root| root["source"] == "aura_builtin"
            && root["exists"] == true
            && root["path"].as_str().is_some_and(|path| path.ends_with(r"skills\builtin"))),
        "{roots}"
    );
    let loaded = loading::load(&f.runtime, None, NAME, None).expect("read").expect("loaded");
    assert_eq!(
        loaded.files,
        [
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
            "sdk/python/aura_plugin.py",
        ]
    );
    for tool in ["plugins.validate", "plugins.dev_install", "plugins.logs", "tools.call"] {
        assert!(loaded.instructions.contains(tool), "{tool}");
    }
    let helpers = ["reference/service.md", "reference/channels.md", "sdk/python/aura_plugin.py"];
    for file in helpers.into_iter().chain(["sdk/node/aura-plugin.mjs", "examples/fake-chat"]) {
        assert!(loaded.instructions.contains(file), "SKILL.md points to {file}");
    }
    for query in ["插件", "飞书"] {
        let (found, _) = loading::search(&f.runtime, None, query, 20).expect("found");
        let names: Vec<&str> = found.iter().map(|skill| skill.name.as_str()).collect();
        assert_eq!(names, [NAME], "{query}");
    }
}

#[test]
fn each_start_puts_back_what_differs_and_leaves_what_matches() {
    let f = Fixture::new();
    f.runtime.include_builtin_skills();
    let folder = folder(&f);
    let skill_md = folder.join("SKILL.md");
    let original = std::fs::read(&skill_md).expect("written");
    let manifest = folder.join("reference").join("manifest.md");
    let untouched = std::fs::metadata(&manifest).and_then(|m| m.modified()).expect("time");
    write(&skill_md, "---\nname: write-aura-plugin\ndescription: changed\n---\n\nChanged.\n");
    write(&folder.join("extra.md"), "Left over.\n");
    f.list();
    assert_ne!(std::fs::read(&skill_md).expect("read"), original, "once per start");

    let next = SkillRuntime::new(std::sync::Arc::clone(&f.settings), None);
    next.include_builtin_skills();
    next.list(None).expect("listed");
    assert_eq!(std::fs::read(&skill_md).expect("read"), original, "put back");
    assert!(!folder.join("extra.md").exists(), "the folder is Aura's own");
    assert_eq!(
        std::fs::metadata(&manifest).and_then(|m| m.modified()).expect("time"),
        untouched,
        "a file that matches is not written"
    );
}

#[test]
fn a_skill_that_comes_with_aura_is_switched_off_but_never_removed() {
    let f = Fixture::new();
    f.runtime.include_builtin_skills();
    let skill = f.view(NAME);
    let refused = f.dispatch(
        "agent.skills.remove",
        json!({"id": skill["id"], "expected_revision": f.revision()}),
    );
    assert!(
        refused.expect_err("refused").to_string().starts_with("skills.not_removable"),
        "removing it is refused"
    );
    assert!(folder(&f).join("SKILL.md").exists());
    f.dispatch(
        "agent.skills.set_enabled",
        json!({"id": skill["id"], "enabled": false, "expected_revision": f.revision()}),
    )
    .expect("off");
    assert_eq!(f.view(NAME)["state"], "disabled");
    assert!(loading::load(&f.runtime, None, NAME, None).expect("read").is_err());
}
