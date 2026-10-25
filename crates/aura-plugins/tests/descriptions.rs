//! What a plugin says about itself is shown, never a reason to refuse it. Real Claude Code
//! plugins have descriptions over 500 characters and on several lines; each was refused.
mod support;
use support::*;

const DESCRIPTION_CHARS: usize = 2000;

#[test]
fn a_long_claude_description_on_several_lines_is_kept_on_one_line() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("long-notes");
    let description = format!("第一行说明。\n{}\r\n\t最后一行。", "很长的说明".repeat(300));
    let manifest = serde_json::json!({
        "name": "long-notes",
        "version": "1.0.0",
        "description": description,
        "author": {"name": "A".repeat(300)},
        "homepage": format!("https://example.com/{}", "p".repeat(300)),
    });
    write(&root, ".claude-plugin/plugin.json", &manifest.to_string());
    write(&root, "skills/review-guide/SKILL.md", SKILL);
    let package = scan(&root).expect("a long description does not refuse the plugin");
    let shown = &package.manifest.description;
    assert!(shown.starts_with("第一行说明。 很长的说明"), "{shown}");
    assert!(shown.ends_with("很长的说明 最后一行。"), "the whole description is kept: {shown}");
    assert!(!shown.chars().any(char::is_control), "{shown}");
    let author = package.manifest.author.expect("author");
    assert_eq!(author.chars().count(), 201, "an author is cut, not refused: {author}");
    assert!(author.ends_with('…'));
    let homepage = package.manifest.homepage.expect("homepage");
    assert!(homepage.starts_with("https://example.com/") && homepage.chars().count() <= 201);
}

#[test]
fn an_aura_description_beyond_the_limit_is_cut_not_refused() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("long-kit");
    let description = "很长的说明".repeat(600);
    write(
        &root,
        "aura-plugin.toml",
        &format!(
            "schema_version = 1\nname = \"long-kit\"\nversion = \"1.0.0\"\ndescription = \"\"\"\n{description}\n第二段\"\"\"\n"
        ),
    );
    write(&root, "skills/review-guide/SKILL.md", SKILL);
    let package = scan(&root).expect("a long description does not refuse the plugin");
    let shown = &package.manifest.description;
    assert_eq!(shown.chars().count(), DESCRIPTION_CHARS + 1, "{shown}");
    assert!(shown.starts_with("很长的说明很长的说明") && shown.ends_with('…'));
    assert!(!shown.chars().any(char::is_control));
}
