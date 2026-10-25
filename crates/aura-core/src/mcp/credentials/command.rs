//! `agent.mcp.secret.set`: the one route that takes an MCP secret. The value arrives only on
//! a bound SecretInput frame; the answer names the secret's ID, never its value.
use crate::*;
use aura_mcp::{McpSecretSetRequest, McpSecretSetResult};

pub(crate) const METHOD: &str = "agent.mcp.secret.set";

pub(crate) fn binding(
    method: &str,
) -> Option<(aura_ipc::SecretInputOperation, aura_ipc::SecretInputSlot)> {
    (method == METHOD).then_some((
        aura_ipc::SecretInputOperation::McpSecretSet,
        aura_ipc::SecretInputSlot::ApiKey,
    ))
}

fn envelope(
    code: &str,
    category: ErrorCategory,
    message: String,
    correlation: CorrelationId,
) -> IpcErrorEnvelope {
    IpcErrorEnvelope {
        code: ErrorCode::new(code).expect("closed MCP secret code"),
        category,
        correlation_id: correlation,
        terminal: IpcRequestTerminal::Failed,
        retryable: false,
        message,
        details: BTreeMap::new(),
    }
}

pub(crate) fn validate(
    params: &Value,
    correlation: CorrelationId,
) -> std::result::Result<(), IpcErrorEnvelope> {
    match serde_json::from_value::<McpSecretSetRequest>(params.clone()) {
        Ok(request) if (1..=32).contains(&request.server_id.len()) => Ok(()),
        _ => Err(envelope(
            "mcp.secret_invalid_request",
            ErrorCategory::Validation,
            "MCP 密钥请求无效，没有保存；请重新打开 MCP 设置后再试。".into(),
            correlation,
        )),
    }
}

/// Writes the secret off the reactor (the database and the Credential Manager both block).
pub(crate) async fn dispatch(
    state: &RuntimeState,
    request: &JsonRpcRequest,
    secret: &SecretBytes,
) -> std::result::Result<p05::P05CommandDispatch, IpcErrorEnvelope> {
    validate(&request.params, request.id)?;
    let StartupMode::Ready(services) = &state.startup_mode else {
        return Err(envelope(
            "core.recovery_required",
            ErrorCategory::Unavailable,
            "Aura 的数据需要恢复，暂不能保存密钥。".into(),
            request.id,
        ));
    };
    let parsed: McpSecretSetRequest =
        serde_json::from_value(request.params.clone()).map_err(|_| {
            envelope(
                "mcp.secret_invalid_request",
                ErrorCategory::Validation,
                "MCP 密钥请求无效，没有保存；请重新打开 MCP 设置后再试。".into(),
                request.id,
            )
        })?;
    let runtime = Arc::clone(&services.mcp);
    let value = SecretBytes::new(secret.expose().to_vec());
    let correlation = request.id;
    let written = tokio::task::spawn_blocking(move || {
        let written = runtime.secrets().set(&parsed.server_id, parsed.secret_id, &value);
        if written.is_ok() {
            runtime.secret_written(&parsed.server_id);
        }
        written
    })
    .await
    .map_err(|_| {
        envelope(
            "mcp.secret_failed",
            ErrorCategory::Internal,
            "密钥没有确认写入，请查看服务器状态后再设置".into(),
            correlation,
        )
    })?;
    match written {
        Ok(secret_id) => Ok(p05::P05CommandDispatch {
            value: serde_json::to_value(McpSecretSetResult { secret_id }).map_err(|_| {
                envelope(
                    "mcp.secret_failed",
                    ErrorCategory::Internal,
                    "密钥结果无法生成".into(),
                    correlation,
                )
            })?,
            projection_changes: Vec::new(),
        }),
        Err(reason) => Err(envelope(
            "mcp.secret_failed",
            ErrorCategory::Conflict,
            format!("mcp.secret_failed: {reason}"),
            correlation,
        )),
    }
}
