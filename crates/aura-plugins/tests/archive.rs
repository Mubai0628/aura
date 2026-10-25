//! Plugin ZIP files: a GitHub download (one top-level folder) or a plugin at the top unpacks into
//! a plugin Aura can read; `.git` is left out; a ZIP without a plugin manifest is refused.
mod support;
use aura_plugins::{PluginLayout, unpack_plugin_archive};
use aura_skills::SkillArchiveError;
use std::io::Write;
use std::path::{Path, PathBuf};
use support::scan;
use zip::{CompressionMethod, write::SimpleFileOptions};

fn archive(dir: &Path, name: &str, files: &[(&str, &str)]) -> PathBuf {
    let path = dir.join(name);
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).expect("zip"));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (entry, content) in files {
        zip.start_file(*entry, options).expect("entry");
        zip.write_all(content.as_bytes()).expect("bytes");
    }
    zip.finish().expect("finish");
    path
}

fn unpack(path: &Path) -> (tempfile::TempDir, Result<PathBuf, SkillArchiveError>) {
    let out = tempfile::tempdir().expect("out");
    let result = unpack_plugin_archive(std::fs::File::open(path).expect("open"), out.path());
    (out, result)
}

#[test]
fn a_github_download_unpacks_from_its_top_folder_without_git() {
    let temp = tempfile::tempdir().expect("temp");
    let zip = archive(
        temp.path(),
        "commit-tools-main.zip",
        &[
            ("commit-tools-main/.claude-plugin/plugin.json", r#"{"name": "commit-tools"}"#),
            ("commit-tools-main/commands/commit.md", "Write a commit message.\n"),
            ("commit-tools-main/.git/HEAD", "ref: refs/heads/main\n"),
        ],
    );
    let (out, root) = unpack(&zip);
    let root = root.expect("unpacked");
    assert_eq!(root, out.path().join("commit-tools-main"));
    assert!(!root.join(".git").exists(), "not part of the plugin");
    let package = scan(&root).expect("a plugin");
    assert_eq!(package.layout, PluginLayout::ClaudeCode);
    assert_eq!(package.command_items()[0].name, "commit");
}

#[test]
fn an_aura_plugin_at_the_top_unpacks_in_place() {
    let temp = tempfile::tempdir().expect("temp");
    let zip = archive(
        temp.path(),
        "plain.zip",
        &[
            ("aura-plugin.toml", "schema_version = 1\nname = \"plain\"\nversion = \"1\"\n"),
            ("skills/review-guide/SKILL.md", support::SKILL),
        ],
    );
    let (out, root) = unpack(&zip);
    assert_eq!(root.expect("unpacked"), out.path());
    assert_eq!(scan(out.path()).expect("a plugin").manifest.name, "plain");
}

#[test]
fn a_zip_without_a_plugin_manifest_is_refused_naming_both() {
    let temp = tempfile::tempdir().expect("temp");
    let zip = archive(temp.path(), "readme.zip", &[("README.md", "hello")]);
    let (out, result) = unpack(&zip);
    let message = result.expect_err("no manifest").to_string();
    assert!(message.contains("aura-plugin.toml") && message.contains(".claude-plugin/plugin.json"));
    assert_eq!(std::fs::read_dir(out.path()).expect("dir").count(), 0, "nothing written");
}
