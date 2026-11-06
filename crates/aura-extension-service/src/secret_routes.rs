//! The routes that take an extension's secret: an MCP server's (`agent.mcp.secret.set`) and one
//! of a plugin's secret settings (`agent.plugins.secret.set`). The value arrives only on a bound
//! SecretInput frame; the answer names the secret's credential, never its value.
use crate::mcp::McpRuntime;
use crate::plugins::PluginRuntime;
use aura_domain_common::{CorrelationId, ErrorCategory, ErrorCode};
use aura_ipc::{SecretInputOperation, SecretInputSlot};
use aura_mcp::{McpSecretSetRequest, McpSecretSetResult};
use aura_plugins::PluginSecretSetRequest;
use aura_runtime_contract::{IpcErrorEnvelope, IpcRequestTerminal};
use aura_windows_platform::SecretBytes;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

pub const MCP_SECRET_SET: &str = "agent.mcp.secret.set";
pub const PLUGIN_SECRET_SET: &str = "agent.plugins.secret.set";

/// The frame a route's value arrives on.
#[must_use]
pub fn binding(method: &str) -> Option<(SecretInputOperation, SecretInputSlot)> {
    let operation = match method {
        MCP_SECRET_SET => SecretInputOperation::McpSecretSet,
        PLUGIN_SECRET_SET => SecretInputOperation::PluginSecretSet,
        _ => return None,
    };
    Some((operation, SecretInputSlot::ApiKey))
}

fn envelope(
    code: &str,
    category: ErrorCategory,
    message: String,
    correlation: CorrelationId,
) -> IpcErrorEnvelope {
    IpcErrorEnvelope {
        code: ErrorCode::new(code).expect("closed extension secret code"),
        category,
        correlation_id: correlation,
        terminal: IpcRequestTerminal::Failed,
        retryable: false,
        message,
        details: BTreeMap::new(),
    }
}

enum Request {
    Mcp(McpSecretSetRequest),
    Plugin(PluginSecretSetRequest),
}

impl Request {
    /// The route's request, or the refusal sent before any frame is bound.
    fn read(
        method: &str,
        params: &Value,
        correlation: CorrelationId,
    ) -> Result<Self, IpcErrorEnvelope> {
        let params = params.clone();
        let read = match method {
            PLUGIN_SECRET_SET => serde_json::from_value::<PluginSecretSetRequest>(params)
                .ok()
                .filter(|r| (1..=32).contains(&r.name.len()) && (1..=40).contains(&r.key.len()))
                .map(Self::Plugin),
            _ => serde_json::from_value::<McpSecretSetRequest>(params)
                .ok()
                .filter(|request| (1..=32).contains(&request.server_id.len()))
                .map(Self::Mcp),
        };
        read.ok_or_else(|| match method {
            PLUGIN_SECRET_SET => envelope(
                "plugins.secret_invalid_request",
                ErrorCategory::Validation,
                "保密设置的请求无效，没有保存；请重新打开插件设置后再试。".into(),
                correlation,
            ),
            _ => envelope(
                "mcp.secret_invalid_request",
                ErrorCategory::Validation,
                "MCP 密钥请求无效，没有保存；请重新打开 MCP 设置后再试。".into(),
                correlation,
            ),
        })
    }

    /// The code a failure is answered with, and what is said when the write is not confirmed.
    const fn failed(&self) -> (&'static str, &'static str) {
        match self {
            Self::Mcp(_) => ("mcp.secret_failed", "密钥没有确认写入，请查看服务器状态后再设置"),
            Self::Plugin(_) => {
                ("plugins.secret_failed", "保密设置没有确认写入，请在插件页查看后再设置")
            }
        }
    }
}

pub fn validate(
    method: &str,
    params: &Value,
    correlation: CorrelationId,
) -> Result<(), IpcErrorEnvelope> {
    Request::read(method, params, correlation).map(|_| ())
}

/// Writes the secret off the reactor (the database and the Credential Manager both block).
pub async fn dispatch(
    mcp: Arc<McpRuntime>,
    plugins: Arc<PluginRuntime>,
    method: &str,
    params: &Value,
    secret: &SecretBytes,
    correlation: CorrelationId,
) -> Result<Value, IpcErrorEnvelope> {
    let request = Request::read(method, params, correlation)?;
    let (code, unconfirmed) = request.failed();
    let value = SecretBytes::new(secret.expose().to_vec());
    let written = tokio::task::spawn_blocking(move || match request {
        Request::Mcp(request) => {
            let written = mcp.secrets().set(&request.server_id, request.secret_id, &value);
            if written.is_ok() {
                mcp.secret_written(&request.server_id);
            }
            written
                .map(|secret_id| serde_json::to_value(McpSecretSetResult { secret_id }))
                .map_err(|reason| format!("mcp.secret_failed: {reason}"))
        }
        Request::Plugin(request) => plugins
            .set_secret(&request, &value)
            .map(serde_json::to_value)
            .map_err(|reason| reason.to_string()),
    })
    .await
    .map_err(|_| envelope(code, ErrorCategory::Internal, unconfirmed.into(), correlation))?;
    match written {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(_)) => {
            Err(envelope(code, ErrorCategory::Internal, "密钥结果无法生成".into(), correlation))
        }
        Err(reason) => Err(envelope(code, ErrorCategory::Conflict, reason, correlation)),
    }
}
