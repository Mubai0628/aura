//! Checking a plugin folder before it is installed: every problem, each with where it is, and
//! whether the plugin can be installed at all.
mod support;
use aura_plugins::validate_plugin;
use aura_skills::FileHashCache;
use support::*;

#[test]
fn a_plugin_that_cannot_be_installed_says_every_reason() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("broken");
    write(
        &root,
        "aura-plugin.toml",
        "schema_version = 2\nname = \"Broken Name\"\nversion = \"\"\ncapabilities = [\"files.read\"]\n",
    );
    let checked = validate_plugin(&root, &FileHashCache::default());
    assert!(!checked.installable);
    assert!(checked.name.is_none());
    let all = checked.problems.join("\n");
    for expected in ["name", "version", "files.read"] {
        assert!(all.contains(expected), "{expected}: {all}");
    }
    let empty = temp.path().join("empty");
    std::fs::create_dir_all(&empty).expect("folder");
    let checked = validate_plugin(&empty, &FileHashCache::default());
    assert!(!checked.installable);
    assert!(checked.problems.join("\n").contains("aura-plugin.toml"), "{:?}", checked.problems);
}

#[test]
fn a_plugin_with_broken_parts_can_be_installed_and_names_each_part() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("review-kit");
    aura_plugin(&root);
    write(&root, "skills/broken/SKILL.md", "no frontmatter\n");
    write(&root, "hooks/format-check/hook.toml", "schema_version = 1\n");
    let checked = validate_plugin(&root, &FileHashCache::default());
    assert!(checked.installable, "{:?}", checked.problems);
    assert_eq!(checked.name.as_deref(), Some("review-kit"));
    assert_eq!(checked.version.as_deref(), Some("1.2.0"));
    let all = checked.problems.join("\n");
    assert!(all.contains("Skill skills/broken"), "{all}");
    assert!(all.contains("Hook hooks/format-check"), "{all}");
    assert_eq!(checked.brings.skills, 1, "the working Skill is still brought");
}

#[test]
fn a_clean_plugin_has_no_problems() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("review-kit");
    aura_plugin(&root);
    let checked = validate_plugin(&root, &FileHashCache::default());
    assert!(checked.installable);
    assert!(checked.problems.is_empty(), "{:?}", checked.problems);
    assert!(checked.warnings.is_empty(), "{:?}", checked.warnings);
}
