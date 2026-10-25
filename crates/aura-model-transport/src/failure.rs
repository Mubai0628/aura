use std::{future::pending, time::Duration};

use aura_domain_common::{
    ArtifactId, ContentHash, ModelFailureId, TransportAttemptId, UtcTimestamp,
};
use aura_model_domain::{
    FailureFingerprint, ModelExecutionSnapshot, ModelFailureCategory, ModelRetryClass,
    ModelStreamEvent, ModelStreamEventEnvelope, NormalizedModelFailure, PersistedModelFailure,
    TransportAttemptRecord, TransportAttemptState,
};
use tokio::time::Instant;

use crate::{
    ModelAdapterError, ModelCancellationToken, ModelTransportError, RawStreamWriter,
    TransportEventChannel, TransportStore, TransportTimeoutKind,
};

pub(crate) fn persisted_failure(failure: NormalizedModelFailure) -> PersistedModelFailure {
    let fingerprint_bytes = serde_json::to_vec(&(
        failure.category,
        &failure.code,
        &failure.message,
        failure.provider_status,
    ))
    .unwrap_or_else(|_| failure.code.as_bytes().to_vec());
    PersistedModelFailure {
        id: ModelFailureId::new(),
        fingerprint: FailureFingerprint(ContentHash::blake3(&fingerprint_bytes)),
        failure,
        created_at: UtcTimestamp::now(),
    }
}

pub(crate) fn protocol_failure(
    code: &str,
    message: &str,
    attempt_id: TransportAttemptId,
    raw: Option<ArtifactId>,
) -> NormalizedModelFailure {
    NormalizedModelFailure {
        category: ModelFailureCategory::Protocol,
        code: code.to_owned(),
        message: message.to_owned(),
        retry_class: ModelRetryClass::NotRetryable,
        provider_status: None,
        provider_request_id: None,
        provider_error_ref: raw,
        partial_output_ref: None,
        transport_attempt_id: Some(attempt_id),
    }
}

pub(crate) fn adapter_failure(
    error: ModelAdapterError,
    attempt_id: TransportAttemptId,
    raw: Option<ArtifactId>,
) -> NormalizedModelFailure {
    let mut failure = error.into_failure(Some(attempt_id));
    failure.provider_error_ref = raw;
    failure
}

pub(crate) fn cancelled_failure(
    attempt_id: Option<TransportAttemptId>,
    partial: Option<ArtifactId>,
) -> NormalizedModelFailure {
    NormalizedModelFailure {
        category: ModelFailureCategory::Cancelled,
        code: "model_transport_cancelled".to_owned(),
        message: "model transport was cancelled".to_owned(),
        retry_class: ModelRetryClass::NotRetryable,
        provider_status: None,
        provider_request_id: None,
        provider_error_ref: None,
        partial_output_ref: partial,
        transport_attempt_id: attempt_id,
    }
}

pub(crate) fn endpoint_changed_failure(
    attempt_id: Option<TransportAttemptId>,
) -> NormalizedModelFailure {
    NormalizedModelFailure {
        category: ModelFailureCategory::InvalidRequest,
        code: "endpoint_fingerprint_changed".to_owned(),
        message: "endpoint configuration changed before a transport attempt".to_owned(),
        retry_class: ModelRetryClass::NotRetryable,
        provider_status: None,
        provider_request_id: None,
        provider_error_ref: None,
        partial_output_ref: None,
        transport_attempt_id: attempt_id,
    }
}

pub(crate) fn timeout_failure(
    kind: TransportTimeoutKind,
    attempt_id: TransportAttemptId,
    emitted_valid_event: bool,
    partial: Option<ArtifactId>,
) -> NormalizedModelFailure {
    NormalizedModelFailure {
        category: if emitted_valid_event {
            ModelFailureCategory::StreamInterrupted
        } else {
            ModelFailureCategory::Transport
        },
        code: format!("model_transport_{kind:?}_timeout").to_ascii_lowercase(),
        message: format!("model transport timed out during {kind:?}"),
        retry_class: if emitted_valid_event {
            ModelRetryClass::NotRetryable
        } else {
            ModelRetryClass::TransparentRetry { retry_after_ms: None }
        },
        provider_status: None,
        provider_request_id: None,
        provider_error_ref: None,
        partial_output_ref: partial,
        transport_attempt_id: Some(attempt_id),
    }
}

pub(crate) fn transport_error_failure(
    error: ModelTransportError,
    attempt_id: TransportAttemptId,
    emitted_valid_event: bool,
    partial: Option<ArtifactId>,
) -> Result<NormalizedModelFailure, ModelTransportError> {
    if matches!(
        &error,
        ModelTransportError::Store(_)
            | ModelTransportError::EventChannelClosed
            | ModelTransportError::InvalidProviderRequest(_)
            | ModelTransportError::WrongNetworkBinding
            | ModelTransportError::JitterUnavailable
            | ModelTransportError::GatePoisoned
            | ModelTransportError::InvalidEndpointLimit(_)
    ) {
        return Err(error);
    }
    if matches!(&error, ModelTransportError::Cancelled) {
        return Ok(cancelled_failure(Some(attempt_id), partial));
    }
    let stable_code = match &error {
        ModelTransportError::Network(
            aura_network_control::NetworkControlError::ContentEncodingUnsupported,
        ) => Some("transport.content_encoding_unsupported"),
        ModelTransportError::Network(
            aura_network_control::NetworkControlError::DecompressionFailed,
        ) => Some("transport.decompression_failed"),
        ModelTransportError::Network(
            aura_network_control::NetworkControlError::ResponseTooLarge
            | aura_network_control::NetworkControlError::DownloadLimitExceeded,
        ) => Some("transport.response_too_large"),
        _ => None,
    };
    let retryable = !emitted_valid_event
        && matches!(
            &error,
            ModelTransportError::Network(
                aura_network_control::NetworkControlError::Timeout
                    | aura_network_control::NetworkControlError::RateLimited
                    | aura_network_control::NetworkControlError::Transport(_)
            ) | ModelTransportError::Timeout(_)
        );
    Ok(NormalizedModelFailure {
        category: if emitted_valid_event {
            ModelFailureCategory::StreamInterrupted
        } else {
            ModelFailureCategory::Transport
        },
        code: stable_code.unwrap_or("model_transport_failed").to_owned(),
        message: error.to_string(),
        retry_class: if retryable {
            ModelRetryClass::TransparentRetry { retry_after_ms: None }
        } else {
            ModelRetryClass::NotRetryable
        },
        provider_status: None,
        provider_request_id: None,
        provider_error_ref: None,
        partial_output_ref: partial,
        transport_attempt_id: Some(attempt_id),
    })
}

pub(crate) async fn set_attempt_state<S: TransportStore>(
    store: &S,
    attempt: &mut TransportAttemptRecord,
    state: TransportAttemptState,
) -> Result<(), ModelTransportError> {
    attempt.state = state;
    store.record_attempt(attempt).await
}

pub(crate) async fn finish_failed_attempt<S: TransportStore>(
    store: &S,
    attempt: &mut TransportAttemptRecord,
    failure: &PersistedModelFailure,
    raw_artifact_id: Option<ArtifactId>,
) -> Result<(), ModelTransportError> {
    if failure.failure.category == ModelFailureCategory::Cancelled {
        return Err(ModelTransportError::Store(
            "cancelled attempts must not be persisted as model failures".to_owned(),
        ));
    }
    store.record_failure(failure).await?;
    attempt.state = TransportAttemptState::Failed;
    attempt.finished_at = Some(UtcTimestamp::now());
    attempt.failure_id = Some(failure.id);
    if raw_artifact_id.is_some() {
        attempt.raw_stream_artifact_id = raw_artifact_id;
    }
    store.record_attempt(attempt).await
}

pub(crate) async fn finish_cancelled_attempt<S: TransportStore>(
    store: &S,
    attempt: &mut TransportAttemptRecord,
    raw_artifact_id: Option<ArtifactId>,
) -> Result<(), ModelTransportError> {
    attempt.state = TransportAttemptState::Cancelled;
    attempt.finished_at = Some(UtcTimestamp::now());
    attempt.failure_id = None;
    if raw_artifact_id.is_some() {
        attempt.raw_stream_artifact_id = raw_artifact_id;
    }
    store.record_attempt(attempt).await
}

pub(crate) async fn send_terminal_failure(
    channel: &TransportEventChannel,
    snapshot: &ModelExecutionSnapshot,
    attempt_id: TransportAttemptId,
    sequence: u64,
    failure: &PersistedModelFailure,
) -> Result<(), ModelTransportError> {
    channel
        .sender
        .send(ModelStreamEventEnvelope {
            model_step_id: snapshot.model_step_id,
            transport_attempt_id: attempt_id,
            sequence,
            received_at: UtcTimestamp::now(),
            event: ModelStreamEvent::StreamError { failure: failure.failure.clone() },
        })
        .await
        .map_err(|_| ModelTransportError::EventChannelClosed)
}

pub(crate) async fn send_terminal_normalized_failure(
    channel: &TransportEventChannel,
    snapshot: &ModelExecutionSnapshot,
    attempt_id: TransportAttemptId,
    sequence: u64,
    failure: &NormalizedModelFailure,
) -> Result<(), ModelTransportError> {
    channel
        .sender
        .send(ModelStreamEventEnvelope {
            model_step_id: snapshot.model_step_id,
            transport_attempt_id: attempt_id,
            sequence,
            received_at: UtcTimestamp::now(),
            event: ModelStreamEvent::StreamError { failure: failure.clone() },
        })
        .await
        .map_err(|_| ModelTransportError::EventChannelClosed)
}

pub(crate) async fn send_terminal_event(
    channel: &TransportEventChannel,
    envelope: ModelStreamEventEnvelope,
) -> Result<(), ModelTransportError> {
    channel.sender.send(envelope).await.map_err(|_| ModelTransportError::EventChannelClosed)
}

pub(crate) async fn send_envelope(
    channel: &TransportEventChannel,
    envelope: ModelStreamEventEnvelope,
    cancellation: &ModelCancellationToken,
    total_deadline: Option<Instant>,
) -> Result<(), ModelTransportError> {
    tokio::select! {
        () = cancellation.cancelled() => Err(ModelTransportError::Cancelled),
        () = wait_optional_deadline(total_deadline) => {
            Err(ModelTransportError::Timeout(TransportTimeoutKind::Total))
        }
        result = channel.sender.send(envelope) => {
            result.map_err(|_| ModelTransportError::EventChannelClosed)
        }
    }
}

pub(crate) async fn finalize_writer<W: RawStreamWriter>(
    writer: Option<W>,
) -> Result<ArtifactId, ModelTransportError> {
    writer
        .ok_or_else(|| ModelTransportError::Store("raw stream writer is unavailable".to_owned()))?
        .finalize()
        .await
}

pub(crate) async fn wait_retry_delay(
    delay: Duration,
    cancellation: &ModelCancellationToken,
) -> Result<(), ModelTransportError> {
    tokio::select! {
        () = cancellation.cancelled() => Err(ModelTransportError::Cancelled),
        () = tokio::time::sleep(delay) => Ok(()),
    }
}

pub(crate) async fn wait_optional_deadline(deadline: Option<Instant>) {
    if let Some(deadline) = deadline {
        tokio::time::sleep_until(deadline).await;
    } else {
        pending::<()>().await;
    }
}

pub(crate) fn random_jitter(limit: Duration) -> Result<Duration, ModelTransportError> {
    let limit_millis = u64::try_from(limit.as_millis()).unwrap_or(u64::MAX);
    if limit_millis == 0 {
        return Ok(Duration::ZERO);
    }
    let mut bytes = [0_u8; 8];
    getrandom::fill(&mut bytes).map_err(|_| ModelTransportError::JitterUnavailable)?;
    let value = u64::from_le_bytes(bytes) % limit_millis.saturating_add(1);
    Ok(Duration::from_millis(value))
}

pub(crate) fn provider_request_id(headers: &[(String, String)]) -> Option<String> {
    ["x-request-id", "request-id", "cf-ray"]
        .into_iter()
        .find_map(|name| header(headers, name))
        .filter(|value| !value.trim().is_empty())
}

pub(crate) fn header(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}
