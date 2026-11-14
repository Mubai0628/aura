//! Plugin folders written for the tests, in a temporary folder.
#![allow(dead_code)]
use aura_plugins::{PluginPackage, PluginPackageError, scan_plugin};
use aura_skills::FileHashCache;
use std::path::Path;

pub fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative.replace('/', "\\"));
    std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
    std::fs::write(path, content).expect("written");
}

pub fn scan(root: &Path) -> Result<PluginPackage, PluginPackageError> {
    scan_plugin(root, &FileHashCache::default())
}

pub fn problems(result: Result<PluginPackage, PluginPackageError>) -> String {
    match result {
        Ok(package) => panic!("expected problems, read {}", package.manifest.name),
        Err(error) => error.to_string(),
    }
}

pub const SKILL: &str = "---\nname: review-guide\ndescription: How this team reviews changes\n---\n\nRead the diff first.\n";

pub const HOOK: &str = "schema_version = 1\nname = \"format-check\"\nversion = \"1.0.0\"\ndescription = \"检查格式\"\nevents = [\"tool.before_call\"]\n\n[command]\npowershell_script = \"check.ps1\"\n";

/// An Aura plugin with one of everything.
pub fn aura_plugin(root: &Path) {
    write(
        root,
        "aura-plugin.toml",
        "schema_version = 1\nname = \"review-kit\"\nversion = \"1.2.0\"\ndescription = \"审查用的 Skill、命令与服务器\"\nauthor = \"someone\"\n\n[limits]\nmemory_mb = 512\nprocesses = 8\n",
    );
    write(root, "skills/review-guide/SKILL.md", SKILL);
    write(
        root,
        "commands/review/command.toml",
        "name = \"review\"\ndescription = \"审查改动\"\nversion = 1\n",
    );
    write(root, "commands/review/template.md", "请审查 $ARGUMENTS 的改动。\n");
    write(root, "hooks/format-check/hook.toml", HOOK);
    write(root, "hooks/format-check/check.ps1", "'{\"decision\":\"continue\"}'\n");
    write(
        root,
        "mcp.json",
        r#"{"servers": [
            {"name": "files", "display_name": "文件", "transport": {"type": "stdio",
             "command": "${PLUGIN_ROOT}/bin/server.exe", "arguments": ["--root", "${PLUGIN_ROOT}"],
             "environment": [{"name": "LOG_LEVEL", "value": "info"}],
             "secret_environment": ["GITHUB_TOKEN"]}},
            {"name": "docs", "transport": {"type": "http", "url": "https://mcp.example.com/mcp",
             "auth": {"type": "oauth", "scopes": ["read"]}}}
        ]}"#,
    );
    write(root, "bin/server.exe", "not really a program");
}

/// A Claude Code plugin with commands, servers and the parts Aura does not use.
pub fn claude_plugin(root: &Path) {
    write(
        root,
        ".claude-plugin/plugin.json",
        r#"{"name": "commit-tools", "version": "0.3.0", "description": "Commit helpers",
            "author": {"name": "Someone", "email": "someone@example.com"}}"#,
    );
    write(
        root,
        "commands/commit.md",
        "---\ndescription: Write a commit message\nargument-hint: [scope]\nallowed-tools: Bash(git:*)\nmodel: some-model\n---\n\nWrite a commit message for $ARGUMENTS.\nCurrent status: !`git status`\nFirst file: $1\n",
    );
    write(root, "commands/Quick_Fix.md", "Fix the smallest thing first.\n");
    write(root, "commands/git/push.md", "Push it.\n");
    write(root, "skills/review-guide/SKILL.md", SKILL);
    write(
        root,
        ".mcp.json",
        r#"{"mcpServers": {
            "local": {"command": "${CLAUDE_PLUGIN_ROOT}/servers/run.cmd", "args": ["--port", "0"],
                      "env": {"GITHUB_TOKEN": "${GITHUB_TOKEN}", "MODE": "plain"}},
            "remote": {"type": "http", "url": "https://api.example.com/mcp",
                       "headers": {"Authorization": "Bearer ${API_KEY}", "X-Team": "core"}},
            "events": {"type": "sse", "url": "https://api.example.com/sse"}
        }}"#,
    );
    write(root, "servers/run.cmd", "@echo off\r\n");
    write(root, "hooks/hooks.json", "{}");
    write(root, "agents/reviewer.md", "---\nname: reviewer\n---\nReview.\n");
}
