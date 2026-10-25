//! A plugin's MCP servers run as `<plugin>-<name>`, checked by the same rules as a server the
//! user adds; two servers that would share an ID, or an ID that is too long, are named.
mod support;
use aura_plugins::{HTTP_SECRET_KEY, server_id};
use support::*;

#[test]
fn a_server_runs_as_the_plugin_and_its_own_name() {
    assert_eq!(server_id("review-kit", "files"), "review-kit-files");
    assert_eq!(server_id("review-kit", "GitHub_Server"), "review-kit-github-server");
    assert_eq!(server_id("review-kit", "review-kit"), "review-kit");
    assert_eq!(server_id("review-kit", "--"), "review-kit");
}

#[test]
fn an_id_too_long_or_shared_is_a_problem_of_that_server() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("plugin");
    write(
        &root,
        "aura-plugin.toml",
        "schema_version = 1\nname = \"a-plugin-with-a-long-name\"\nversion = \"1\"\n",
    );
    write(
        &root,
        "mcp.json",
        r#"{"servers": [
            {"name": "files", "transport": {"type": "stdio", "command": "node"}},
            {"name": "Files", "transport": {"type": "stdio", "command": "node"}},
            {"name": "a-much-longer-server-name", "transport": {"type": "stdio", "command": "node"}}
        ]}"#,
    );
    let package = scan(&root).expect("a plugin");
    assert!(package.servers[0].problems.is_empty(), "{:?}", package.servers[0].problems);
    assert!(package.servers[1].problems.iter().any(|p| p.contains("另一个服务器")));
    assert!(package.servers[2].problems.iter().any(|p| p.contains("最长 32 个字符")));
    assert_eq!(package.counts().servers, 1);
}

#[test]
fn a_secret_in_configuration_is_refused_as_for_a_users_server() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("plugin");
    write(&root, "aura-plugin.toml", "schema_version = 1\nname = \"plugin\"\nversion = \"1\"\n");
    write(
        &root,
        "mcp.json",
        r#"{"servers": [{"name": "files", "transport": {"type": "stdio", "command": "node",
            "environment": [{"name": "API_TOKEN", "value": "sk-not-here"}]}}]}"#,
    );
    let package = scan(&root).expect("a plugin");
    assert!(package.servers[0].problems.iter().any(|p| p.contains("API_TOKEN")));
}

#[test]
fn an_http_sign_in_secret_has_its_own_key() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("plugin");
    write(&root, "aura-plugin.toml", "schema_version = 1\nname = \"plugin\"\nversion = \"1\"\n");
    write(
        &root,
        "mcp.json",
        r#"{"servers": [{"name": "remote", "transport": {"type": "http",
            "url": "https://mcp.example.com/mcp", "auth": {"type": "secret", "header": "X-Api-Key"}}}]}"#,
    );
    let package = scan(&root).expect("a plugin");
    assert_eq!(package.servers[0].secret_keys(), vec![HTTP_SECRET_KEY.to_owned()]);
    assert!(package.servers[0].problems.is_empty(), "{:?}", package.servers[0].problems);
}
