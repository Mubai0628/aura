//! The secret routes: a plugin setting's value and an MCP server's secret each arrive on their own
//! SecretInput frame, are written where they belong and answered without the value.
use super::service::{SECRET, bot};
use super::support::Fixture;
use crate::secret_routes::{self, MCP_SECRET_SET, PLUGIN_SECRET_SET};
use aura_domain_common::CorrelationId;
use aura_ipc::{SecretInputOperation, SecretInputSlot};
use aura_windows_platform::SecretBytes;
use serde_json::{Value, json};
use std::sync::Arc;

async fn send(f: &Fixture, method: &str, params: &Value, value: &str) -> Result<Value, String> {
    let secret = SecretBytes::new(value.as_bytes().to_vec());
    let runtime = Arc::clone(&f.runtime);
    secret_routes::dispatch(f.mcp(), runtime, method, params, &secret, CorrelationId::new())
        .await
        .map_err(|refused| format!("{} {}", refused.code.as_str(), refused.message))
}

#[test]
fn each_route_has_its_own_frame_and_nothing_else_does() {
    let api_key = SecretInputSlot::ApiKey;
    assert_eq!(
        secret_routes::binding(PLUGIN_SECRET_SET),
        Some((SecretInputOperation::PluginSecretSet, api_key))
    );
    assert_eq!(
        secret_routes::binding(MCP_SECRET_SET),
        Some((SecretInputOperation::McpSecretSet, api_key))
    );
    assert_eq!(secret_routes::binding("agent.plugins.configure"), None);
}

#[test]
fn a_plugin_secret_request_names_only_the_plugin_and_the_setting() {
    let correlation = CorrelationId::new();
    let valid = json!({"name": "chat-bot", "key": "app_secret"});
    secret_routes::validate(PLUGIN_SECRET_SET, &valid, correlation).expect("valid");
    for wrong in [
        json!({"name": "chat-bot"}),
        json!({"name": "chat-bot", "key": "app_secret", "value": SECRET}),
        json!({"name": "", "key": "app_secret"}),
        json!({"name": "chat-bot", "key": "k".repeat(41)}),
    ] {
        let refused = secret_routes::validate(PLUGIN_SECRET_SET, &wrong, correlation)
            .expect_err("refused before a frame is bound");
        assert_eq!(refused.code.as_str(), "plugins.secret_invalid_request", "{wrong}");
    }
}

#[tokio::test]
async fn a_plugin_setting_is_written_and_answered_with_the_revision_only() {
    let f = Fixture::with_secrets();
    bot(&f);
    let params = json!({"name": "chat-bot", "key": "app_secret"});
    let answer = send(&f, PLUGIN_SECRET_SET, &params, SECRET).await.expect("written");
    assert_eq!(answer, json!({"revision": f.revision()}));
    assert!(!answer.to_string().contains(SECRET));
    assert_eq!(f.secrets().len(), 1);
    let text = json!({"name": "chat-bot", "key": "app_id"});
    let refused = send(&f, PLUGIN_SECRET_SET, &text, SECRET).await.expect_err("a text setting");
    assert!(refused.starts_with("plugins.secret_failed plugins.setting_not_secret"), "{refused}");
}

#[tokio::test]
async fn an_mcp_secret_still_goes_to_its_server() {
    let f = Fixture::with_secrets();
    let params = json!({"server_id": "web", "secret_id": null});
    let answer = send(&f, MCP_SECRET_SET, &params, SECRET).await.expect("written");
    assert!(answer["secret_id"].is_string(), "{answer}");
    assert!(!answer.to_string().contains(SECRET));
    assert_eq!(f.secrets().len(), 1);
}
