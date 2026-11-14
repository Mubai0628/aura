//! Closed terminal-only metadata and SecretInput adapter. No credential persistence.
use crate::*;
use aura_agent_execution::command_execution::terminal_sensitive_refusal as refusal;
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
pub(crate) fn validate(
    params: &Value,
    correlation: CorrelationId,
) -> std::result::Result<(), IpcErrorEnvelope> {
    serde_json::from_value::<TerminalSensitiveReplyV1>(params.clone())
        .map(|_| ())
        .map_err(|_| refusal(correlation, None))
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
    result.map_err(|failure| refusal(request.id, Some(&failure)))
}
