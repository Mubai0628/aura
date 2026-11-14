use aura_config::{AuraPaths, ConfigModule, SettingsFacade, SettingsFacadeError};
use aura_mcp::{McpEnvironmentEntry, McpServerConfig, McpTransport};

fn facade() -> (tempfile::TempDir, AuraPaths, std::sync::Arc<SettingsFacade>) {
    let temp = tempfile::tempdir().expect("TEMP");
    let paths = AuraPaths::from_roots(temp.path().join("config"), temp.path().join("data"));
    let facade = SettingsFacade::open(paths.clone()).expect("settings");
    (temp, paths, facade)
}

fn server(id: &str, enabled: bool, trusted: Option<&str>) -> McpServerConfig {
    McpServerConfig {
        id: id.into(),
        display_name: format!("服务器 {id}"),
        transport: McpTransport::Stdio {
            command: "C:\\Tools\\server.exe".into(),
            arguments: vec!["--stdio".into()],
            working_directory: None,
            environment: vec![McpEnvironmentEntry {
                name: "LOG_LEVEL".into(),
                value: "info".into(),
            }],
            secret_environment: vec![],
        },
        enabled,
        trusted_fingerprint: trusted.map(str::to_owned),
        startup_timeout_ms: 30_000,
        call_timeout_ms: 120_000,
        available_for_subagents: false,
    }
}

#[test]
fn the_servers_keep_order_trust_and_switches_at_each_revision() {
    let (_temp, paths, facade) = facade();
    let empty = facade.mcp_registry();
    assert!(empty.servers.is_empty() && empty.issue.is_none());
    let fingerprint = "b".repeat(64);
    let servers = vec![server("second", false, None), server("first", true, Some(&fingerprint))];
    let revision = facade.replace_mcp_registry(empty.revision, &servers).expect("written");
    assert_eq!(revision, empty.revision + 1);
    let reopened = SettingsFacade::open(paths).expect("reopened").mcp_registry();
    assert_eq!(reopened.servers, servers, "the order and every field survive a restart");
    assert_eq!(reopened.revision, revision);
}

#[test]
fn a_stale_revision_or_a_secret_is_refused_without_writing() {
    let (_temp, _paths, facade) = facade();
    let start = facade.mcp_registry().revision;
    facade.replace_mcp_registry(start, &[server("a", true, None)]).expect("first");
    let stale = facade.replace_mcp_registry(start, &[server("b", true, None)]).expect_err("stale");
    assert!(matches!(stale, SettingsFacadeError::RevisionConflict { .. }), "{stale:?}");
    let mut secret = server("c", true, None);
    if let McpTransport::Stdio { environment, .. } = &mut secret.transport {
        environment.push(McpEnvironmentEntry { name: "API_TOKEN".into(), value: "x".into() });
    }
    let current = facade.mcp_registry().revision;
    assert!(facade.replace_mcp_registry(current, &[secret]).is_err(), "a secret never lands");
    assert_eq!(facade.mcp_registry().servers, [server("a", true, None)]);
}

#[test]
fn a_hand_edited_file_that_cannot_be_read_reaches_no_server() {
    let (_temp, paths, facade) = facade();
    drop(facade);
    for bad in [
        r#"{"schema_version":1,"revision":3,"servers":[{"id":"x","display_name":"x","transport":{"type":"stdio","command":"x"},"tools":[]}]}"#,
        r#"{"schema_version":1,"revision":3,"servers":[{"id":"x","display_name":"x","transport":{"type":"stdio","command":"x"},"trusted_fingerprint":"short"}]}"#,
        r#"{"schema_version":1,"revision":3,"extra":1}"#,
        r#"{"schema_version":1,"revision":3,"servers":[{"id":"x","display_name":"x","transport":{"type":"http","url":"http://example.com/mcp"}}]}"#,
    ] {
        std::fs::write(paths.config_path(ConfigModule::Mcp), bad).expect("hand edit");
        let reopened = SettingsFacade::open(paths.clone()).expect("other domains stay available");
        let registry = reopened.mcp_registry();
        assert!(registry.servers.is_empty(), "{bad}");
        assert!(registry.issue.as_deref().is_some_and(|i| i.contains("都不连接")), "{bad}");
        assert_eq!(
            std::fs::read_to_string(paths.config_path(ConfigModule::Mcp)).expect("kept"),
            bad
        );
    }
}
