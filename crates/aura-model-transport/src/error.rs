use aura_domain_common::EndpointId;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportTimeoutKind {
    Connect,
    FirstEvent,
    StreamIdle,
    Total,
}

#[derive(Debug, Error)]
pub enum ModelTransportError {
    #[error("model transport was cancelled")]
    Cancelled,
    #[error("model transport timed out during {0:?}")]
    Timeout(TransportTimeoutKind),
    #[error("endpoint {0} has an invalid zero concurrency limit")]
    InvalidEndpointLimit(EndpointId),
    #[error("endpoint gate state was poisoned")]
    GatePoisoned,
    #[error("SSE line exceeds the configured byte limit")]
    SseLineTooLarge,
    #[error("SSE event exceeds the configured byte limit")]
    SseEventTooLarge,
    #[error("SSE stream contains invalid UTF-8")]
    InvalidSseUtf8,
    #[error("SSE retry field is not an unsigned integer")]
    InvalidSseRetry,
    #[error("provider request plan is invalid: {0}")]
    InvalidProviderRequest(String),
    #[error("model network request used the wrong P04 caller or grant binding")]
    WrongNetworkBinding,
    #[error("model adapter failed: {0}")]
    Adapter(#[from] crate::ModelAdapterError),
    #[error("model transport persistence failed: {0}")]
    Store(String),
    #[error("model transport event consumer closed before terminal delivery")]
    EventChannelClosed,
    #[error("failed to generate transport retry jitter")]
    JitterUnavailable,
    #[error(transparent)]
    Network(#[from] aura_network_control::NetworkControlError),
}
