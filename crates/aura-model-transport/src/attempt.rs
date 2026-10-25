use std::time::Duration;

use aura_domain_common::{ArtifactId, NetworkRequestId, TransportAttemptId, UtcTimestamp};
use aura_model_domain::{
    ModelExecutionSnapshot, ModelStreamEvent, ModelStreamEventEnvelope, TransportAttemptRecord,
    TransportAttemptState, derive_provider_tool_call_protocol_bindings,
};
use tokio::time::Instant;

use crate::{
    ModelCancellationToken, ModelNetworkExecutor, ModelProtocolDecoder, ModelTransport,
    ModelTransportError, PROVIDER_TOOL_CALL_BINDING_INVALID, ProviderBindingViolationDiagnostic,
    ProviderErrorResponse, ProviderRequestPlan, RawStreamWriter, SseDecoder, TransportEventChannel,
    TransportHttpBody, TransportStore, TransportTimeoutKind,
    engine::AttemptTerminal,
    failure::{
        adapter_failure, cancelled_failure, finalize_writer, header, protocol_failure,
        provider_request_id, send_envelope, set_attempt_state, timeout_failure,
        transport_error_failure, wait_optional_deadline,
    },
};

pub(crate) struct AttemptExecution<'a> {
    pub snapshot: &'a ModelExecutionSnapshot,
    pub endpoint_timeouts: &'a aura_model_domain::ModelEndpointTimeouts,
    pub request_plan: &'a ProviderRequestPlan,
    pub adapter: &'a dyn ModelProtocolDecoder,
    pub attempt_id: TransportAttemptId,
    pub client_request_id: NetworkRequestId,
    pub cancellation: &'a ModelCancellationToken,
    pub events: &'a TransportEventChannel,
    pub attempt: &'a mut TransportAttemptRecord,
    /// Evidence for a refusal that must reach the application layer.
    ///
    /// The attempt itself only knows the normalized failure, which is a
    /// persisted contract; the bounded facts that name what was refused are
    /// recorded here, on the attempt that produced them, so the run terminal can
    /// carry them without widening `NormalizedModelFailure`.
    pub diagnostic: &'a mut Option<ProviderBindingViolationDiagnostic>,
}

impl<N, S> ModelTransport<N, S>
where
    N: ModelNetworkExecutor,
    S: TransportStore,
{
    pub(crate) async fn run_attempt(
        &self,
        execution: AttemptExecution<'_>,
    ) -> Result<AttemptTerminal, ModelTransportError> {
        let AttemptExecution {
            snapshot,
            endpoint_timeouts,
            request_plan,
            adapter,
            attempt_id,
            client_request_id,
            cancellation,
            events,
            attempt,
            diagnostic,
        } = execution;
        let total_deadline = endpoint_timeouts
            .total_ms
            .map(|milliseconds| Instant::now() + Duration::from_millis(milliseconds));
        set_attempt_state(&self.store, attempt, TransportAttemptState::AuthorizingNetwork).await?;
        // P05 §8.5: `connect_ms` bounds establishing the connection, which the network executor
        // does itself (the gateway's connect limit); `first_event_ms` runs from sending the
        // request to the provider's first event. A provider that sends its headers only once its
        // model starts is still waiting for its first event.
        let first_event_deadline =
            Instant::now() + Duration::from_millis(endpoint_timeouts.first_event_ms);
        let open = self.network.open(snapshot, request_plan, attempt_id, client_request_id);
        tokio::pin!(open);
        let response = tokio::select! {
            () = cancellation.cancelled() => {
                return Ok(AttemptTerminal::Failed {
                    failure: cancelled_failure(Some(attempt_id), None),
                    emitted_valid_event: false,
                    next_sequence: 0,
                });
            }
            () = tokio::time::sleep_until(first_event_deadline) => {
                return Ok(AttemptTerminal::Failed {
                    failure: timeout_failure(
                        TransportTimeoutKind::FirstEvent,
                        attempt_id,
                        false,
                        None,
                    ),
                    emitted_valid_event: false,
                    next_sequence: 0,
                });
            }
            () = wait_optional_deadline(total_deadline) => {
                return Ok(AttemptTerminal::Failed {
                    failure: timeout_failure(TransportTimeoutKind::Total, attempt_id, false, None),
                    emitted_valid_event: false,
                    next_sequence: 0,
                });
            }
            result = &mut open => match result {
                Ok(response) => response,
                Err(error) => {
                    return Ok(AttemptTerminal::Failed {
                        failure: transport_error_failure(error, attempt_id, false, None)?,
                        emitted_valid_event: false,
                        next_sequence: 0,
                    });
                }
            }
        };
        set_attempt_state(&self.store, attempt, TransportAttemptState::Connecting).await?;
        attempt.provider_request_id = provider_request_id(&response.headers);
        let mut raw_writer = Some(self.store.begin_raw_stream(attempt_id).await?);
        if !(200..300).contains(&response.status) {
            let body_result = collect_error_body(
                response.body,
                raw_writer.as_mut().ok_or_else(missing_raw_writer)?,
                endpoint_timeouts,
                total_deadline,
                cancellation,
            )
            .await;
            let body = match body_result {
                Ok(body) => {
                    let raw_artifact_id = finalize_writer(raw_writer.take()).await?;
                    attempt.raw_stream_artifact_id = Some(raw_artifact_id);
                    body
                }
                Err(error) => {
                    abort_writer(raw_writer.take())?;
                    return Ok(AttemptTerminal::Failed {
                        failure: transport_error_failure(error, attempt_id, false, None)?,
                        emitted_valid_event: false,
                        next_sequence: 0,
                    });
                }
            };
            let raw_artifact_id = attempt.raw_stream_artifact_id.ok_or_else(|| {
                ModelTransportError::Store("raw response artifact is missing".to_owned())
            })?;
            let mut failure = adapter.classify_http_error(&ProviderErrorResponse {
                status: response.status,
                headers: response.headers,
                body,
                provider_request_id: attempt.provider_request_id.clone(),
                raw_error_artifact_id: Some(raw_artifact_id),
            });
            failure.transport_attempt_id = Some(attempt_id);
            return Ok(AttemptTerminal::Failed {
                failure,
                emitted_valid_event: false,
                next_sequence: 0,
            });
        }
        if header(&response.headers, "content-type")
            .is_some_and(|value| !value.to_ascii_lowercase().contains("text/event-stream"))
        {
            abort_writer(raw_writer.take())?;
            return Ok(AttemptTerminal::Failed {
                failure: protocol_failure(
                    "provider_content_type_invalid",
                    "successful streaming response is not text/event-stream",
                    attempt_id,
                    None,
                ),
                emitted_valid_event: false,
                next_sequence: 0,
            });
        }
        set_attempt_state(&self.store, attempt, TransportAttemptState::AwaitingFirstEvent).await?;
        let mut body = response.body;
        let mut sse = SseDecoder::new(self.sse_limits);
        let mut decoder = adapter.create_stream_decoder(snapshot);
        let mut stream_idle_deadline = None;
        let mut emitted_valid_event = false;
        let mut sequence = 0_u64;
        let mut pending_terminal = None;
        loop {
            let phase_deadline = stream_idle_deadline.unwrap_or(first_event_deadline);
            let chunk = tokio::select! {
                () = cancellation.cancelled() => {
                    abort_writer(raw_writer.take())?;
                    return Ok(AttemptTerminal::Failed {
                        failure: cancelled_failure(Some(attempt_id), None),
                        emitted_valid_event,
                        next_sequence: sequence,
                    });
                }
                () = tokio::time::sleep_until(phase_deadline) => {
                    let kind = if emitted_valid_event {
                        TransportTimeoutKind::StreamIdle
                    } else {
                        TransportTimeoutKind::FirstEvent
                    };
                    abort_writer(raw_writer.take())?;
                    return Ok(AttemptTerminal::Failed {
                        failure: timeout_failure(kind, attempt_id, emitted_valid_event, None),
                        emitted_valid_event,
                        next_sequence: sequence,
                    });
                }
                () = wait_optional_deadline(total_deadline) => {
                    abort_writer(raw_writer.take())?;
                    return Ok(AttemptTerminal::Failed {
                        failure: timeout_failure(
                            TransportTimeoutKind::Total,
                            attempt_id,
                            emitted_valid_event,
                            None,
                        ),
                        emitted_valid_event,
                        next_sequence: sequence,
                    });
                }
                result = body.next_chunk() => match result {
                    Ok(chunk) => chunk,
                    Err(error) => {
                        abort_writer(raw_writer.take())?;
                        return Ok(AttemptTerminal::Failed {
                            failure: transport_error_failure(
                                error,
                                attempt_id,
                                emitted_valid_event,
                                None,
                            )?,
                            emitted_valid_event,
                            next_sequence: sequence,
                        });
                    }
                }
            };
            let Some(chunk) = chunk else {
                break;
            };
            if chunk.is_empty() {
                continue;
            }
            if attempt.first_byte_at.is_none() {
                attempt.first_byte_at = Some(UtcTimestamp::now());
                self.store.record_attempt(attempt).await?;
            }
            raw_writer.as_mut().ok_or_else(missing_raw_writer)?.append(&chunk)?;
            let frames = match sse.push(&chunk) {
                Ok(frames) => frames,
                Err(error) => {
                    abort_writer(raw_writer.take())?;
                    return Ok(AttemptTerminal::Failed {
                        failure: protocol_failure(
                            "provider_sse_invalid",
                            &error.to_string(),
                            attempt_id,
                            None,
                        ),
                        emitted_valid_event,
                        next_sequence: sequence,
                    });
                }
            };
            for frame in frames {
                let decoded = match decoder.push_frame(frame) {
                    Ok(decoded) => decoded,
                    Err(error) => {
                        abort_writer(raw_writer.take())?;
                        return Ok(AttemptTerminal::Failed {
                            failure: adapter_failure(error, attempt_id, None),
                            emitted_valid_event,
                            next_sequence: sequence,
                        });
                    }
                };
                if let Err(error) = emit_events(EmitEventsRequest {
                    store: &self.store,
                    channel: events,
                    snapshot,
                    attempt,
                    attempt_id,
                    decoded,
                    sequence: &mut sequence,
                    emitted_valid_event: &mut emitted_valid_event,
                    pending_terminal: &mut pending_terminal,
                    cancellation,
                    total_deadline,
                })
                .await
                {
                    abort_writer(raw_writer.take())?;
                    return map_event_delivery_error(
                        error,
                        attempt_id,
                        emitted_valid_event,
                        sequence,
                        None,
                    );
                }
            }
            if emitted_valid_event {
                stream_idle_deadline =
                    Some(Instant::now() + Duration::from_millis(endpoint_timeouts.stream_idle_ms));
            }
        }
        let trailing_frames = match sse.finish() {
            Ok(frames) => frames,
            Err(error) => {
                abort_writer(raw_writer.take())?;
                return Ok(AttemptTerminal::Failed {
                    failure: protocol_failure(
                        "provider_sse_invalid",
                        &error.to_string(),
                        attempt_id,
                        None,
                    ),
                    emitted_valid_event,
                    next_sequence: sequence,
                });
            }
        };
        for frame in trailing_frames {
            let decoded = match decoder.push_frame(frame) {
                Ok(decoded) => decoded,
                Err(error) => {
                    abort_writer(raw_writer.take())?;
                    return Ok(AttemptTerminal::Failed {
                        failure: adapter_failure(error, attempt_id, None),
                        emitted_valid_event,
                        next_sequence: sequence,
                    });
                }
            };
            if let Err(error) = emit_events(EmitEventsRequest {
                store: &self.store,
                channel: events,
                snapshot,
                attempt,
                attempt_id,
                decoded,
                sequence: &mut sequence,
                emitted_valid_event: &mut emitted_valid_event,
                pending_terminal: &mut pending_terminal,
                cancellation,
                total_deadline,
            })
            .await
            {
                abort_writer(raw_writer.take())?;
                return map_event_delivery_error(
                    error,
                    attempt_id,
                    emitted_valid_event,
                    sequence,
                    None,
                );
            }
        }
        match decoder.finish() {
            Ok(mut output) => {
                if let Err(error) = derive_provider_tool_call_protocol_bindings(
                    snapshot,
                    &request_plan.request_evidence,
                    &output,
                ) {
                    // The Provider named a call the frozen request never exposed.
                    // Nothing is dispatched and nothing is retried; the bounded
                    // facts of the refusal travel with the terminal failure so the
                    // application layer can show what was refused, for which step,
                    // against which frozen tool list.
                    *diagnostic = Some(ProviderBindingViolationDiagnostic::new(
                        &error,
                        &request_plan.request_evidence,
                        snapshot,
                        attempt_id,
                        adapter.tool_call_item_type(),
                    ));
                    abort_writer(raw_writer.take())?;
                    return Ok(AttemptTerminal::Failed {
                        failure: adapter_failure(
                            crate::ModelAdapterError::protocol(
                                PROVIDER_TOOL_CALL_BINDING_INVALID,
                                error.to_string(),
                            ),
                            attempt_id,
                            None,
                        ),
                        emitted_valid_event,
                        next_sequence: sequence,
                    });
                }
                let Some(terminal) = pending_terminal else {
                    abort_writer(raw_writer.take())?;
                    return Ok(AttemptTerminal::Failed {
                        failure: adapter_failure(
                            crate::ModelAdapterError::protocol(
                                "provider_terminal_missing",
                                "adapter finished without a normalized terminal event",
                            ),
                            attempt_id,
                            None,
                        ),
                        emitted_valid_event,
                        next_sequence: sequence,
                    });
                };
                let continuation = match decoder.take_continuation_artifact() {
                    Ok(continuation) => continuation,
                    Err(error) => {
                        abort_writer(raw_writer.take())?;
                        return Ok(AttemptTerminal::Failed {
                            failure: adapter_failure(error, attempt_id, None),
                            emitted_valid_event,
                            next_sequence: sequence,
                        });
                    }
                };
                if let Some(continuation) = continuation {
                    if continuation.bytes.is_empty() || continuation.media_type.trim().is_empty() {
                        return Err(ModelTransportError::Store(
                            "provider continuation artifact must have bytes and a media type"
                                .to_owned(),
                        ));
                    }
                    let artifact_id = self
                        .store
                        .provider_continuation_artifact(attempt_id, &continuation)
                        .await?;
                    output.provider_continuation = Some(
                        crate::engine::persisted_continuation_state(&continuation, artifact_id)?,
                    );
                }
                let raw_artifact_id = finalize_writer(raw_writer.take()).await?;
                attempt.raw_stream_artifact_id = Some(raw_artifact_id);
                Ok(AttemptTerminal::Completed {
                    output: Box::new(output),
                    terminal_event: terminal,
                    next_sequence: sequence,
                })
            }
            Err(error) => {
                abort_writer(raw_writer.take())?;
                Ok(AttemptTerminal::Failed {
                    failure: adapter_failure(error, attempt_id, None),
                    emitted_valid_event,
                    next_sequence: sequence,
                })
            }
        }
    }
}

async fn collect_error_body<B: TransportHttpBody, W: RawStreamWriter>(
    mut body: B,
    writer: &mut W,
    timeouts: &aura_model_domain::ModelEndpointTimeouts,
    total_deadline: Option<Instant>,
    cancellation: &ModelCancellationToken,
) -> Result<Vec<u8>, ModelTransportError> {
    let mut bytes = Vec::new();
    loop {
        let idle_deadline = Instant::now() + Duration::from_millis(timeouts.stream_idle_ms);
        let chunk = tokio::select! {
            () = cancellation.cancelled() => return Err(ModelTransportError::Cancelled),
            () = tokio::time::sleep_until(idle_deadline) => {
                return Err(ModelTransportError::Timeout(TransportTimeoutKind::StreamIdle));
            }
            () = wait_optional_deadline(total_deadline) => {
                return Err(ModelTransportError::Timeout(TransportTimeoutKind::Total));
            }
            result = body.next_chunk() => result?,
        };
        let Some(chunk) = chunk else {
            break;
        };
        writer.append(&chunk)?;
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

struct EmitEventsRequest<'a, S> {
    store: &'a S,
    channel: &'a TransportEventChannel,
    snapshot: &'a ModelExecutionSnapshot,
    attempt: &'a mut TransportAttemptRecord,
    attempt_id: TransportAttemptId,
    decoded: Vec<ModelStreamEvent>,
    sequence: &'a mut u64,
    emitted_valid_event: &'a mut bool,
    pending_terminal: &'a mut Option<ModelStreamEvent>,
    cancellation: &'a ModelCancellationToken,
    total_deadline: Option<Instant>,
}

async fn emit_events<S: TransportStore>(
    request: EmitEventsRequest<'_, S>,
) -> Result<(), ModelTransportError> {
    let EmitEventsRequest {
        store,
        channel,
        snapshot,
        attempt,
        attempt_id,
        decoded,
        sequence,
        emitted_valid_event,
        pending_terminal,
        cancellation,
        total_deadline,
    } = request;
    for event in decoded {
        if !*emitted_valid_event {
            *emitted_valid_event = true;
            attempt.first_model_event_at = Some(UtcTimestamp::now());
            attempt.state = TransportAttemptState::Streaming;
            store.record_attempt(attempt).await?;
        }
        if is_terminal_event(&event) {
            if pending_terminal.replace(event).is_some() {
                return Err(ModelTransportError::Adapter(crate::ModelAdapterError::protocol(
                    "provider_terminal_duplicate",
                    "adapter emitted more than one normalized terminal event",
                )));
            }
            continue;
        }
        let envelope = ModelStreamEventEnvelope {
            model_step_id: snapshot.model_step_id,
            transport_attempt_id: attempt_id,
            sequence: *sequence,
            received_at: UtcTimestamp::now(),
            event,
        };
        *sequence = sequence.saturating_add(1);
        send_envelope(channel, envelope, cancellation, total_deadline).await?;
    }
    Ok(())
}

fn is_terminal_event(event: &ModelStreamEvent) -> bool {
    matches!(
        event,
        ModelStreamEvent::ResponseCompleted { .. }
            | ModelStreamEvent::ResponseIncomplete { .. }
            | ModelStreamEvent::StreamError { .. }
    )
}

fn map_event_delivery_error(
    error: ModelTransportError,
    attempt_id: TransportAttemptId,
    emitted_valid_event: bool,
    sequence: u64,
    raw: Option<ArtifactId>,
) -> Result<AttemptTerminal, ModelTransportError> {
    match error {
        ModelTransportError::Cancelled => Ok(AttemptTerminal::Failed {
            failure: cancelled_failure(Some(attempt_id), raw),
            emitted_valid_event,
            next_sequence: sequence,
        }),
        ModelTransportError::Timeout(kind) => Ok(AttemptTerminal::Failed {
            failure: timeout_failure(kind, attempt_id, emitted_valid_event, raw),
            emitted_valid_event,
            next_sequence: sequence,
        }),
        ModelTransportError::Adapter(error) => Ok(AttemptTerminal::Failed {
            failure: adapter_failure(error, attempt_id, raw),
            emitted_valid_event,
            next_sequence: sequence,
        }),
        other => Err(other),
    }
}

fn missing_raw_writer() -> ModelTransportError {
    ModelTransportError::Store("raw stream writer is unavailable".to_owned())
}

fn abort_writer<W: RawStreamWriter>(writer: Option<W>) -> Result<(), ModelTransportError> {
    writer.ok_or_else(missing_raw_writer)?.abort()
}
