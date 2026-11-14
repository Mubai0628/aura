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

/// A request one of Core's routes refused, answered with the code its message starts with
/// (`skills.busy: …`) so a window can look the code up; a message without one is answered as
/// `agent.request_rejected`.
pub fn refused_envelope(
    correlation_id: aura_domain_common::CorrelationId,
    error: impl std::fmt::Display,
) -> IpcErrorEnvelope {
    let message = error.to_string();
    let code = message
        .split_once(": ")
        .map(|(code, _)| code)
        .filter(|code| code.contains('.'))
        .and_then(|code| aura_domain_common::ErrorCode::new(code).ok())
        .unwrap_or_else(|| {
            aura_domain_common::ErrorCode::new("agent.request_rejected").expect("static code")
        });
    IpcErrorEnvelope {
        code,
        category: aura_domain_common::ErrorCategory::Validation,
        message,
        retryable: false,
        correlation_id,
        terminal: aura_runtime_contract::IpcRequestTerminal::Failed,
        details: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_carries_the_code_its_message_starts_with() {
        let id = aura_domain_common::CorrelationId::new();
        let named = refused_envelope(id, "skills.busy: Skill 正在被另一项操作修改。");
        assert_eq!(named.code.as_str(), "skills.busy");
        assert_eq!(named.message, "skills.busy: Skill 正在被另一项操作修改。");
        for unnamed in ["没有代码的一句话。", "fatal: not a code", "Missing field `x`"] {
            let envelope = refused_envelope(id, unnamed);
            assert_eq!(envelope.code.as_str(), "agent.request_rejected", "{unnamed}");
            assert_eq!(envelope.message, unnamed);
        }
    }
}
