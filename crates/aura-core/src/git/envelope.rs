use aura_domain_common::{CorrelationId, ErrorCode};
use aura_runtime_contract::{IpcErrorEnvelope, IpcRequestTerminal};
pub(crate) fn envelope(id: CorrelationId, error: anyhow::Error) -> IpcErrorEnvelope {
    let mut envelope = crate::broker::error_envelope(id, &error);
    let message = error.to_string();
    let code = message.split_once(':').map_or("git.read_failed", |(code, _)| code);
    if code.starts_with("git.")
        && let Ok(code) = ErrorCode::new(code)
    {
        envelope.code = code;
    }
    if error.is::<super::GitCancelled>() {
        envelope.terminal = IpcRequestTerminal::Cancelled;
    }
    if error.is::<super::StopUnconfirmed>() {
        envelope.details.insert("outcome_unknown".into(), serde_json::json!(true));
        envelope.details.insert(
            crate::command_settlement::OUTCOME_RECOVERY_DETAIL.into(),
            serde_json::json!(crate::command_settlement::OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY),
        );
        envelope.retryable = false;
    }
    envelope
}
