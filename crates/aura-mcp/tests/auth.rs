//! How a server signs in. The configuration names where a secret goes (a header, an
//! environment variable) or that OAuth is used; the secret and the tokens are never in it. A
//! configuration without any of this serializes exactly as before, so its trust still holds.
use aura_domain_common::CredentialId;
use aura_mcp::{
    McpHeader, McpHttpAuth, McpSecretEnvironment, McpServerConfig, McpTransport,
    config_fingerprint, validate_server,
};
use serde_json::json;

fn server(transport: McpTransport) -> McpServerConfig {
    McpServerConfig {
        id: "web".into(),
        display_name: "网络".into(),
        transport,
        enabled: false,
        trusted_fingerprint: None,
        startup_timeout_ms: 30_000,
        call_timeout_ms: 120_000,
        available_for_subagents: false,
    }
}

fn http(headers: Vec<McpHeader>, auth: Option<McpHttpAuth>) -> McpServerConfig {
    server(McpTransport::Http { url: "https://mcp.example.com/mcp".into(), headers, auth })
}

fn stdio(secret_environment: Vec<McpSecretEnvironment>) -> McpServerConfig {
    server(McpTransport::Stdio {
        command: "C:\\Tools\\server.exe".into(),
        arguments: vec![],
        working_directory: None,
        environment: vec![],
        secret_environment,
    })
}

fn secret(header: Option<&str>) -> Option<McpHttpAuth> {
    Some(McpHttpAuth::Secret { secret_id: CredentialId::new(), header: header.map(Into::into) })
}

fn refused(config: &McpServerConfig, words: &str) -> bool {
    validate_server(config).iter().any(|problem| problem.contains(words))
}

#[test]
fn a_secret_goes_in_a_bearer_or_a_named_header_aura_does_not_set_itself() {
    assert!(validate_server(&http(vec![], secret(None))).is_empty());
    assert!(validate_server(&http(vec![], secret(Some("X-Api-Key")))).is_empty());
    assert!(validate_server(&http(vec![], secret(Some("Authorization")))).is_empty());
    for header in ["Content-Type", "Mcp-Session-Id", "Cookie", "Proxy-Authorization"] {
        assert!(refused(&http(vec![], secret(Some(header))), "不能用来放密钥"), "{header}");
    }
    assert!(refused(&http(vec![], secret(Some("X Key"))), "不合法"));
    let plain = McpHeader { name: "X-Team".into(), value: "docs".into() };
    assert!(refused(&http(vec![plain], secret(Some("x-team"))), "已经在普通请求头里"));
}

#[test]
fn oauth_takes_an_optional_client_and_plain_scopes() {
    let oauth = |client_id: Option<&str>, scopes: &[&str]| {
        Some(McpHttpAuth::OAuth {
            client_id: client_id.map(Into::into),
            scopes: scopes.iter().map(|scope| (*scope).to_owned()).collect(),
        })
    };
    assert!(validate_server(&http(vec![], oauth(None, &[]))).is_empty());
    assert!(validate_server(&http(vec![], oauth(Some("aura"), &["repo", "read:org"]))).is_empty());
    assert!(refused(&http(vec![], oauth(Some(" "), &[])), "client_id"));
    assert!(refused(&http(vec![], oauth(None, &["two words"])), "scopes"));
    let many: Vec<String> = (0..17).map(|n| format!("s{n}")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    assert!(refused(&http(vec![], oauth(None, &many)), "scopes"));
}

#[test]
fn a_secret_environment_variable_may_look_like_a_secret_but_not_repeat() {
    let entry =
        |name: &str| McpSecretEnvironment { name: name.into(), secret_id: CredentialId::new() };
    let config = stdio(vec![entry("GITHUB_TOKEN"), entry("OPENAI_API_KEY")]);
    assert!(validate_server(&config).is_empty(), "{:?}", validate_server(&config));
    assert_eq!(config.transport.secret_ids().len(), 2);
    assert!(refused(&stdio(vec![entry("GITHUB_TOKEN"), entry("github_token")]), "重复"));
    assert!(refused(&stdio(vec![entry("1BAD")]), "不合法"));
    let shared = CredentialId::new();
    let twice = vec![
        McpSecretEnvironment { name: "A_TOKEN".into(), secret_id: shared },
        McpSecretEnvironment { name: "B_TOKEN".into(), secret_id: shared },
    ];
    assert!(refused(&stdio(twice), "同一个密钥"));
    let nine: Vec<_> = (0..9).map(|n| entry(&format!("T{n}"))).collect();
    assert!(refused(&stdio(nine), "最多 8 项"));
    // An ordinary variable still may not carry a secret.
    let mut plain = stdio(vec![]);
    if let McpTransport::Stdio { environment, .. } = &mut plain.transport {
        environment
            .push(aura_mcp::McpEnvironmentEntry { name: "API_KEY".into(), value: "x".into() });
    }
    assert!(refused(&plain, "带密钥的环境变量"));
}

#[test]
fn without_sign_in_the_configuration_and_its_trust_stay_as_they_were() {
    let before = json!({
        "type": "http",
        "url": "https://mcp.example.com/mcp",
        "headers": [],
    });
    let transport = http(vec![], None).transport;
    assert_eq!(serde_json::to_value(&transport).expect("json"), before);
    let read: McpTransport = serde_json::from_value(before).expect("an existing configuration");
    assert_eq!(config_fingerprint(&read, None), config_fingerprint(&transport, None));
    let stdio_before = json!({
        "type": "stdio",
        "command": "C:\\Tools\\server.exe",
        "arguments": [],
        "working_directory": null,
        "environment": [],
    });
    assert_eq!(serde_json::to_value(&stdio(vec![]).transport).expect("json"), stdio_before);
    // Where a secret goes is part of the trust; the secret itself is not in the file.
    let signed = http(vec![], secret(None)).transport;
    assert_ne!(config_fingerprint(&signed, None), config_fingerprint(&transport, None));
    let text = serde_json::to_string(&signed).expect("json");
    assert!(text.contains("\"type\":\"secret\"") && text.contains("crd_"), "{text}");
    let oauth =
        serde_json::to_value(McpHttpAuth::OAuth { client_id: None, scopes: vec![] }).expect("json");
    assert_eq!(oauth, json!({"type": "oauth"}));
}
