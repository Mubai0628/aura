//! The envelope a brokered dispatch answers with when it could not run.
use aura_runtime_contract::IpcErrorEnvelope;

pub fn error_envelope(
    correlation_id: aura_domain_common::CorrelationId,
    error: impl std::fmt::Display,
) -> IpcErrorEnvelope {
    IpcErrorEnvelope {
        code: aura_domain_common::ErrorCode::new("broker.dispatch_unavailable")
            .expect("static code"),
        category: aura_domain_common::ErrorCategory::Unavailable,
        message: error.to_string(),
        retryable: false,
        correlation_id,
        terminal: aura_runtime_contract::IpcRequestTerminal::Failed,
        details: Default::default(),
    }
}
