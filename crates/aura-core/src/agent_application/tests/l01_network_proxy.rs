//! A proxy edit is one field of the existing Tools revision. Omitting it must not clear it.
use super::*;
use aura_tool_contract::{
    AgentNetworkProxyTestRequest, AgentToolSettingsUpdate, ToolNetworkProxyProtocolV1,
    ToolNetworkProxyV1,
};

fn update_from(
    view: &aura_tool_contract::AgentToolSettingsView,
    proxy: Option<Option<ToolNetworkProxyV1>>,
) -> AgentToolSettingsUpdate {
    AgentToolSettingsUpdate {
        expected_revision: view.revision,
        fixed_tool_ids: view.fixed_tool_ids.clone(),
        max_dynamic_tool_schemas: view.max_dynamic_tool_schemas,
        max_concurrent_tool_calls: view.max_concurrent_tool_calls,
        default_timeout_ms: view.default_timeout_ms,
        max_retries: view.max_retries,
        use_recommended_fixed_tool_ids: false,
        network_proxy: proxy,
        web_service: None,
    }
}

#[tokio::test]
async fn l02_b0_proxy_recovery_is_previewed_and_revision_checked() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let agent = services.agent_required();
    let first = agent.tool_settings().expect("settings");
    let proxy =
        ToolNetworkProxyV1::new(true, ToolNetworkProxyProtocolV1::Http, "127.0.0.1".into(), 10809)
            .expect("proxy");
    let saved = agent
        .update_tool_settings(
            CommandInvocationId::new(),
            update_from(&first, Some(Some(proxy.clone()))),
        )
        .await
        .expect("save");
    let cleared = agent
        .update_tool_settings(CommandInvocationId::new(), update_from(&saved, Some(None)))
        .await
        .expect("clear");
    let wire = serde_json::to_value(&cleared).expect("view");
    assert_eq!(
        wire["network_proxy_recovery"]["source_revision"],
        serde_json::json!(saved.revision)
    );
    assert_eq!(
        wire["network_proxy_recovery"]["expected_revision"],
        serde_json::json!(cleared.revision)
    );
    assert_eq!(
        wire["network_proxy_recovery"]["proxy"],
        serde_json::to_value(&proxy).expect("proxy")
    );
    let mut restore = update_from(&cleared, Some(Some(proxy.clone())));
    let mut newer = update_from(&cleared, None);
    newer.max_concurrent_tool_calls = 7;
    let latest = agent
        .update_tool_settings(CommandInvocationId::new(), newer)
        .await
        .expect("concurrent save");
    assert!(
        agent.update_tool_settings(CommandInvocationId::new(), restore.clone()).await.is_err(),
        "a stale preview cannot overwrite a newer edit"
    );
    restore.expected_revision = latest.revision;
    restore.max_concurrent_tool_calls = latest.max_concurrent_tool_calls;
    let restored = agent
        .update_tool_settings(CommandInvocationId::new(), restore)
        .await
        .expect("fresh preview apply");
    assert_eq!(restored.network_proxy, Some(proxy));
    assert_eq!(restored.max_concurrent_tool_calls, 7);
}

#[tokio::test]
async fn l01_proxy_save_is_field_scoped_and_the_probe_stays_on_the_loopback_port() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let agent = services.agent_required();
    let view = agent.tool_settings().expect("settings");
    assert!(view.network_proxy.is_none());
    let proxy =
        ToolNetworkProxyV1::new(true, ToolNetworkProxyProtocolV1::Http, "127.0.0.1".into(), 10809)
            .expect("proxy");
    let saved = agent
        .update_tool_settings(
            CommandInvocationId::new(),
            update_from(&view, Some(Some(proxy.clone()))),
        )
        .await
        .expect("save proxy");
    assert_eq!(saved.network_proxy.as_ref(), Some(&proxy));
    assert!(saved.revision != view.revision);

    let mut strategy = update_from(&saved, None);
    strategy.max_concurrent_tool_calls = if saved.max_concurrent_tool_calls == 7 { 8 } else { 7 };
    let kept = agent
        .update_tool_settings(CommandInvocationId::new(), strategy)
        .await
        .expect("strategy save");
    assert_eq!(kept.network_proxy.as_ref(), Some(&proxy), "an omitted proxy stays stored");
    assert_eq!(
        kept.max_concurrent_tool_calls,
        if saved.max_concurrent_tool_calls == 7 { 8 } else { 7 }
    );

    let cleared = agent
        .update_tool_settings(CommandInvocationId::new(), update_from(&kept, Some(None)))
        .await
        .expect("clear");
    assert!(cleared.network_proxy.is_none());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let port = listener.local_addr().expect("address").port();
    let server = tokio::spawn(async move {
        let _ = listener.accept().await;
    });
    let open = agent
        .test_network_proxy(AgentNetworkProxyTestRequest {
            protocol: ToolNetworkProxyProtocolV1::Http,
            host: "127.0.0.1".into(),
            port,
        })
        .await
        .expect("probe");
    assert!(open.ok);
    assert_eq!(open.message, "连接成功");
    server.abort();
    let closed_listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
    let closed_port = closed_listener.local_addr().expect("address").port();
    drop(closed_listener);
    let closed = agent
        .test_network_proxy(AgentNetworkProxyTestRequest {
            protocol: ToolNetworkProxyProtocolV1::Http,
            host: "127.0.0.1".into(),
            port: closed_port,
        })
        .await
        .expect("closed probe");
    assert!(!closed.ok);
    assert_eq!(closed.message, "拒绝连接");
    let remote = agent
        .test_network_proxy(AgentNetworkProxyTestRequest {
            protocol: ToolNetworkProxyProtocolV1::Socks5,
            host: "example.com".into(),
            port: 10808,
        })
        .await
        .expect("rejected host");
    assert!(!remote.ok);
    assert!(remote.message.contains("回环"));
}
