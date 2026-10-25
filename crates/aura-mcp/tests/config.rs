use aura_mcp::{
    McpEnvironmentEntry, McpHeader, McpServerConfig, McpTransport, ResolvedCommand,
    config_fingerprint, secret_like, validate_server, validate_servers,
};

fn stdio(id: &str) -> McpServerConfig {
    McpServerConfig {
        id: id.into(),
        display_name: "文件".into(),
        transport: McpTransport::Stdio {
            command: "C:\\Tools\\mcp-files.exe".into(),
            arguments: vec!["--root".into(), "C:\\Work".into()],
            working_directory: None,
            environment: vec![McpEnvironmentEntry {
                name: "LOG_LEVEL".into(),
                value: "info".into(),
            }],
            secret_environment: vec![],
        },
        enabled: false,
        trusted_fingerprint: None,
        startup_timeout_ms: 30_000,
        call_timeout_ms: 120_000,
        available_for_subagents: false,
    }
}

fn http(url: &str, headers: Vec<McpHeader>) -> McpServerConfig {
    McpServerConfig {
        transport: McpTransport::Http { url: url.into(), headers, auth: None },
        ..stdio("web")
    }
}

#[test]
fn a_plain_stdio_server_is_valid_and_its_defaults_apply() {
    assert!(validate_server(&stdio("files")).is_empty());
    let parsed: McpServerConfig = serde_json::from_value(serde_json::json!({
        "id": "files",
        "display_name": "Files",
        "transport": {"type": "stdio", "command": "mcp-files"}
    }))
    .expect("defaults");
    assert_eq!((parsed.startup_timeout_ms, parsed.call_timeout_ms), (30_000, 120_000));
    assert!(!parsed.enabled && parsed.trusted_fingerprint.is_none());
    assert!(
        serde_json::from_value::<McpServerConfig>(serde_json::json!({
            "id": "files", "display_name": "Files",
            "transport": {"type": "stdio", "command": "x"}, "tools": []
        }))
        .is_err(),
        "a per-tool switch is not a configuration key"
    );
}

#[test]
fn ids_are_stable_lowercase_names_and_aura_names_are_reserved() {
    for bad in ["", "Files", "1files", "files_x", "a-very-long-server-name-over-thirty-two"] {
        assert!(!validate_server(&stdio(bad)).is_empty(), "{bad}");
    }
    assert!(validate_server(&stdio("resources"))[0].contains("保留"));
    let problems = validate_servers(&[stdio("files"), stdio("files")]);
    assert!(problems.iter().any(|p| p.contains("重复")), "{problems:?}");
}

#[test]
fn secrets_never_enter_the_configuration() {
    for name in ["GITHUB_TOKEN", "OPENAI_API_KEY", "DB_PASSWORD", "AURA_THING", "MY_SECRET"] {
        assert!(secret_like(name), "{name}");
        let mut server = stdio("files");
        if let McpTransport::Stdio { environment, .. } = &mut server.transport {
            environment.push(McpEnvironmentEntry { name: name.into(), value: "x".into() });
        }
        assert!(validate_server(&server).iter().any(|p| p.contains("密钥")), "{name}");
    }
    assert!(!secret_like("LOG_LEVEL") && !secret_like("KEYBOARD_LAYOUT"));
    let auth = http(
        "https://mcp.example.com/mcp",
        vec![McpHeader { name: "Authorization".into(), value: "Bearer x".into() }],
    );
    assert!(validate_server(&auth).iter().any(|p| p.contains("凭据")));
    let managed = http(
        "https://mcp.example.com/mcp",
        vec![McpHeader { name: "Mcp-Session-Id".into(), value: "x".into() }],
    );
    assert!(validate_server(&managed).iter().any(|p| p.contains("由 Aura 设置")));
    for url in ["https://mcp.example.com/mcp?api_key=x", "https://mcp.example.com/mcp?a=1&token=x"]
    {
        assert!(validate_server(&http(url, vec![])).iter().any(|p| p.contains("密钥")), "{url}");
    }
    assert!(validate_server(&http("https://mcp.example.com/mcp?team=docs", vec![])).is_empty());
}

#[test]
fn http_needs_https_except_on_this_computer() {
    assert!(validate_server(&http("https://mcp.example.com/mcp", vec![])).is_empty());
    assert!(validate_server(&http("http://127.0.0.1:8080/mcp", vec![])).is_empty());
    assert!(validate_server(&http("http://localhost:8080/mcp", vec![])).is_empty());
    assert!(!validate_server(&http("http://mcp.example.com/mcp", vec![])).is_empty());
    assert!(!validate_server(&http("https://user:pw@mcp.example.com/mcp", vec![])).is_empty());
    assert!(!validate_server(&http("ftp://mcp.example.com", vec![])).is_empty());
}

#[test]
fn the_fingerprint_covers_what_runs_and_nothing_else() {
    let resolved = ResolvedCommand { path: "C:\\Tools\\mcp-files.exe".into(), sha256: "aa".into() };
    let server = stdio("files");
    let base = config_fingerprint(&server.transport, Some(&resolved));
    let mut renamed = server.clone();
    renamed.display_name = "新名字".into();
    renamed.call_timeout_ms = 5_000;
    renamed.enabled = true;
    assert_eq!(config_fingerprint(&renamed.transport, Some(&resolved)), base);
    let mut argued = server.clone();
    if let McpTransport::Stdio { arguments, .. } = &mut argued.transport {
        arguments.push("--write".into());
    }
    assert_ne!(config_fingerprint(&argued.transport, Some(&resolved)), base);
    let replaced = ResolvedCommand { sha256: "bb".into(), ..resolved };
    assert_ne!(config_fingerprint(&server.transport, Some(&replaced)), base, "a replaced program");
}
