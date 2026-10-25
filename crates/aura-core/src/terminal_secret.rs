//! Closed terminal-only metadata and SecretInput adapter. No credential persistence.
use crate::*;
use aura_windows_user_contract::TerminalSensitiveReplyV1;
pub(crate) const SUBMIT: &str = "terminal.user.sensitive.submit";
pub(crate) fn binding(
    method: &str,
) -> Option<(aura_ipc::SecretInputOperation, aura_ipc::SecretInputSlot)> {
    (method == SUBMIT).then_some((
        aura_ipc::SecretInputOperation::TerminalSensitiveInput,
        aura_ipc::SecretInputSlot::TerminalInput,
    ))
}
fn error(correlation: CorrelationId) -> IpcErrorEnvelope {
    IpcErrorEnvelope { code: ErrorCode::new("terminal.sensitive.unavailable").expect("static code"), category: ErrorCategory::Validation, message: "安全输入未提交：请求已结束、会话不匹配或终端已被接管。请检查当前输入请求；不要自动重发输入。".into(), retryable: false, correlation_id: correlation, terminal: IpcRequestTerminal::Failed, details: BTreeMap::new() }
}
pub(crate) fn validate(
    params: &Value,
    correlation: CorrelationId,
) -> std::result::Result<(), IpcErrorEnvelope> {
    serde_json::from_value::<TerminalSensitiveReplyV1>(params.clone())
        .map(|_| ())
        .map_err(|_| error(correlation))
}
pub(crate) async fn dispatch(
    state: &RuntimeState,
    request: &JsonRpcRequest,
    secret: &SecretBytes,
) -> std::result::Result<p05::P05CommandDispatch, IpcErrorEnvelope> {
    let result: Result<_> = async {
        let StartupMode::Ready(services) = &state.startup_mode else {
            bail!("core.recovery_required")
        };
        let input = serde_json::from_value(request.params.clone())?;
        let value = services
            .commands()?
            .terminal_sensitive_submit(input, SecretBytes::new(secret.expose().to_vec()))
            .await?;
        Ok(p05::P05CommandDispatch {
            value: serde_json::to_value(value)?,
            projection_changes: Vec::new(),
        })
    }
    .await;
    result.map_err(|_| error(request.id))
}
