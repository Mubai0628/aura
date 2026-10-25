use std::collections::BTreeSet;

use aura_domain_common::{
    ArtifactId, ContentHash, NetworkRequestId, TransportAttemptId, UtcTimestamp,
};
use aura_model_domain::{
    EndpointAuthScheme, ModelExecutionSnapshot, ModelRequestPriority, ModelStreamEventEnvelope,
    NormalizedModelFailure, NormalizedModelOutput, PersistedModelFailure,
    ProviderContinuationState, ProviderRequestHeaderClass, TransportAttemptKind,
    TransportAttemptRecord, TransportAttemptState, TransportOperationContext,
};
use tokio::sync::mpsc;

use crate::{
    EndpointGate, ModelAdapterError, ModelCancellationToken, ModelNetworkExecutor,
    ModelProtocolDecoder, ModelTransportError, ProviderContinuationArtifact,
    ProviderContinuationArtifactKind, ProviderRequestPlan, RetryDecision, SseLimits,
    TransportRetryPolicy,
    attempt::AttemptExecution,
    failure::{
        cancelled_failure, endpoint_changed_failure, finish_cancelled_attempt,
        finish_failed_attempt, persisted_failure, random_jitter, send_terminal_event,
        send_terminal_failure, send_terminal_normalized_failure, set_attempt_state,
        wait_retry_delay,
    },
};

pub struct TransportEventChannel {
    pub(crate) sender: mpsc::Sender<ModelStreamEventEnvelope>,
}

impl Clone for TransportEventChannel {
    fn clone(&self) -> Self {
        Self { sender: self.sender.clone() }
    }
}

impl TransportEventChannel {
    #[must_use]
    pub fn bounded(capacity: usize) -> (Self, mpsc::Receiver<ModelStreamEventEnvelope>) {
        let (sender, receiver) = mpsc::channel(capacity.max(1));
        (Self { sender }, receiver)
    }
}

pub trait ProviderRequestFactory: Send + Sync {
    fn build_request(
        &self,
        operation_context: &TransportOperationContext,
        applied_compatibility_overrides: &BTreeSet<String>,
    ) -> Result<ProviderRequestPlan, ModelAdapterError>;

    fn current_endpoint_fingerprint(&self) -> Result<ContentHash, ModelTransportError>;
}

/// One attempt's raw provider byte stream.
///
/// WHY `append` IS STILL SYNCHRONOUS
/// Every real writer only reserves and extends a bounded in-memory `Vec` here; neither the
/// Agent writer nor the P05 writer touches a database, a file or a lock on this call. Making it
/// awaitable would create a storage task for a pure memory operation, which is exactly what the
/// Boundary forbids. `finalize` is the call that really persists, so *that* is the awaitable
/// one, and it owns whatever admission its implementor uses.
///
/// CANCELLATION
/// `abort` is synchronous and total: it is called on the paths that already decided the attempt
/// ends without a persisted artifact, and it must not need admission to release memory. A writer
/// dropped without `finalize` and without `abort` releases only memory; no artifact is written.
pub trait RawStreamWriter: Send {
    fn append(&mut self, bytes: &[u8]) -> Result<(), ModelTransportError>;

    /// Persists the bytes collected so far and answers with the real artifact identity.
    ///
    /// This is an admitted operation on the implementor's bounded owner: it may be refused (the
    /// artifact provably was not written) or answer unknown (it started and may still commit),
    /// and neither answer may be reported as the other.
    fn finalize(
        self,
    ) -> impl std::future::Future<Output = Result<ArtifactId, ModelTransportError>> + Send;

    fn abort(self) -> Result<(), ModelTransportError>
    where
        Self: Sized,
    {
        Ok(())
    }
}

/// The transport's storage port.
///
/// WHY EVERY METHOD IS AWAITABLE
/// The transport loop is asynchronous and its store calls really touch SQLite or an artifact
/// writer. When those calls were synchronous, the reactor worker that had to deliver the Agent
/// heartbeat was the same worker parked on the store mutex. Each method therefore returns a
/// future the transport can await, and each implementor decides where its own synchronous
/// critical section runs.
///
/// The futures are `Send`: the Agent run task is spawned, so a store call that was not `Send`
/// could not be awaited from the transport at all.
///
/// CLASSIFICATION EVERY IMPLEMENTOR MUST KEEP
/// * a call that returns before doing any business work is a refusal: the effect did not happen;
/// * a call that started and answered late keeps its result with its own owner and is never
///   reported as "did not run";
/// * a committed result is never discarded and never re-issued after cancellation.
pub trait TransportStore: Send + Sync {
    type Writer: RawStreamWriter;

    fn record_execution_snapshot(
        &self,
        snapshot: &ModelExecutionSnapshot,
    ) -> impl std::future::Future<Output = Result<(), ModelTransportError>> + Send;

    fn canonical_request_body_artifact(
        &self,
        attempt_id: TransportAttemptId,
        attempt_kind: TransportAttemptKind,
        request_plan: &ProviderRequestPlan,
    ) -> impl std::future::Future<Output = Result<Option<ArtifactId>, ModelTransportError>> + Send;

    fn begin_raw_stream(
        &self,
        attempt_id: TransportAttemptId,
    ) -> impl std::future::Future<Output = Result<Self::Writer, ModelTransportError>> + Send;

    fn provider_continuation_artifact(
        &self,
        attempt_id: TransportAttemptId,
        continuation: &ProviderContinuationArtifact,
    ) -> impl std::future::Future<Output = Result<ArtifactId, ModelTransportError>> + Send;

    fn record_attempt(
        &self,
        attempt: &TransportAttemptRecord,
    ) -> impl std::future::Future<Output = Result<(), ModelTransportError>> + Send;

    fn record_failure(
        &self,
        failure: &PersistedModelFailure,
    ) -> impl std::future::Future<Output = Result<(), ModelTransportError>> + Send;

    fn record_output(
        &self,
        snapshot: &ModelExecutionSnapshot,
        attempt_id: TransportAttemptId,
        output: &NormalizedModelOutput,
    ) -> impl std::future::Future<Output = Result<(), ModelTransportError>> + Send;
}

#[derive(Clone, Debug)]
pub enum TransportRunTerminal {
    Completed {
        output: NormalizedModelOutput,
        attempts: Vec<TransportAttemptRecord>,
    },
    Failed {
        failure: PersistedModelFailure,
        attempts: Vec<TransportAttemptRecord>,
        /// Bounded, structured evidence for a refused Provider output. `None`
        /// for a failure the transport could only classify.
        diagnostic: Option<crate::ProviderBindingViolationDiagnostic>,
    },
    Cancelled {
        cancellation: NormalizedModelFailure,
        attempts: Vec<TransportAttemptRecord>,
    },
}

pub struct ModelTransport<N, S> {
    pub(crate) network: N,
    pub(crate) store: S,
    pub(crate) retry_policy: TransportRetryPolicy,
    pub(crate) sse_limits: SseLimits,
}

pub struct ModelTransportExecution<'a> {
    pub snapshot: &'a ModelExecutionSnapshot,
    pub endpoint_timeouts: &'a aura_model_domain::ModelEndpointTimeouts,
    pub gate: &'a EndpointGate,
    pub priority: ModelRequestPriority,
    pub request_factory: &'a dyn ProviderRequestFactory,
    pub adapter: &'a dyn ModelProtocolDecoder,
    pub initial_compatibility_overrides: BTreeSet<String>,
    pub cancellation: &'a ModelCancellationToken,
    pub events: &'a TransportEventChannel,
}

pub(crate) enum AttemptTerminal {
    Completed {
        output: Box<NormalizedModelOutput>,
        terminal_event: aura_model_domain::ModelStreamEvent,
        next_sequence: u64,
    },
    Failed {
        failure: NormalizedModelFailure,
        emitted_valid_event: bool,
        next_sequence: u64,
    },
}

pub(crate) fn persisted_continuation_state(
    continuation: &ProviderContinuationArtifact,
    artifact_id: ArtifactId,
) -> Result<ProviderContinuationState, ModelTransportError> {
    match continuation.kind {
        ProviderContinuationArtifactKind::OpenAiResponses => {
            Ok(ProviderContinuationState::OpenAiResponses { state_ref: artifact_id })
        }
        ProviderContinuationArtifactKind::Anthropic => {
            Ok(ProviderContinuationState::Anthropic { state_ref: artifact_id })
        }
        ProviderContinuationArtifactKind::Opaque => {
            let provider_kind = continuation
                .provider_kind
                .as_ref()
                .filter(|value| !value.trim().is_empty())
                .cloned()
                .ok_or_else(|| {
                    ModelTransportError::Store(
                        "opaque provider continuation kind must not be blank".to_owned(),
                    )
                })?;
            Ok(ProviderContinuationState::Opaque { provider_kind, state_ref: artifact_id })
        }
    }
}

impl<N, S> ModelTransport<N, S>
where
    N: ModelNetworkExecutor,
    S: TransportStore,
{
    #[must_use]
    pub const fn new(
        network: N,
        store: S,
        retry_policy: TransportRetryPolicy,
        sse_limits: SseLimits,
    ) -> Self {
        Self { network, store, retry_policy, sse_limits }
    }

    pub async fn execute(
        &self,
        execution: ModelTransportExecution<'_>,
    ) -> Result<TransportRunTerminal, ModelTransportError> {
        let ModelTransportExecution {
            snapshot,
            endpoint_timeouts,
            gate,
            priority,
            request_factory,
            adapter,
            initial_compatibility_overrides,
            cancellation,
            events,
        } = execution;
        if adapter.api_format() != snapshot.api_format || gate.endpoint_id() != snapshot.endpoint_id
        {
            return Err(ModelTransportError::InvalidProviderRequest(
                "adapter or endpoint gate does not match the execution snapshot".to_owned(),
            ));
        }
        self.store.record_execution_snapshot(snapshot).await?;
        let operation_context = TransportOperationContext::AgentModelAttempt {
            model_step_id: snapshot.model_step_id,
            model_execution_snapshot_id: snapshot.id,
            context_snapshot_id: snapshot.context_snapshot_id,
            prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
        };
        let mut attempts = Vec::new();
        let mut applied_overrides = initial_compatibility_overrides;
        let mut next_kind = TransportAttemptKind::Initial;

        for attempt_number in 1..=crate::MAX_TRANSPORT_ATTEMPTS {
            if cancellation.is_cancelled() {
                return Ok(TransportRunTerminal::Cancelled {
                    cancellation: cancelled_failure(None, None),
                    attempts,
                });
            }
            if request_factory.current_endpoint_fingerprint()? != snapshot.endpoint_fingerprint {
                let failure = persisted_failure(endpoint_changed_failure(None));
                self.store.record_failure(&failure).await?;
                return Ok(TransportRunTerminal::Failed { failure, attempts, diagnostic: None });
            }
            let request_plan =
                request_factory.build_request(&operation_context, &applied_overrides)?;
            validate_compiled_request(
                &request_plan,
                &applied_overrides,
                &operation_context,
                snapshot,
            )?;
            let attempt_id = TransportAttemptId::new();
            let client_request_id = NetworkRequestId::new();
            let mut request_evidence = request_plan.request_evidence.clone();
            request_evidence.canonical_body_artifact_id = self
                .store
                .canonical_request_body_artifact(attempt_id, next_kind, &request_plan)
                .await?;
            if next_kind == TransportAttemptKind::CompatibilityAdjustedRetry
                && request_evidence.canonical_body_artifact_id.is_none()
            {
                return Err(ModelTransportError::Store(
                    "compatibility-adjusted retry must persist its canonical request body"
                        .to_owned(),
                ));
            }
            let mut attempt = TransportAttemptRecord {
                id: attempt_id,
                attempt_number,
                attempt_kind: next_kind,
                client_request_id: client_request_id.to_string(),
                provider_request_id: None,
                endpoint_id: snapshot.endpoint_id,
                endpoint_fingerprint: snapshot.endpoint_fingerprint.clone(),
                request_evidence,
                state: TransportAttemptState::QueuedForEndpoint,
                started_at: UtcTimestamp::now(),
                first_byte_at: None,
                first_model_event_at: None,
                finished_at: None,
                raw_stream_artifact_id: None,
                failure_id: None,
            };
            self.store.record_attempt(&attempt).await?;
            set_attempt_state(&self.store, &mut attempt, TransportAttemptState::AcquiringPermit)
                .await?;
            let permit = match gate.acquire(priority, cancellation).await {
                Ok(permit) => permit,
                Err(ModelTransportError::Cancelled) => {
                    let cancellation = cancelled_failure(Some(attempt_id), None);
                    finish_cancelled_attempt(&self.store, &mut attempt, None).await?;
                    attempts.push(attempt);
                    send_terminal_normalized_failure(
                        events,
                        snapshot,
                        attempt_id,
                        0,
                        &cancellation,
                    )
                    .await?;
                    return Ok(TransportRunTerminal::Cancelled { cancellation, attempts });
                }
                Err(error) => return Err(error),
            };
            set_attempt_state(&self.store, &mut attempt, TransportAttemptState::PreparingRequest)
                .await?;
            // Evidence belongs to the attempt that produced it: a transparent
            // retry starts from an empty record rather than inheriting one.
            let mut attempt_diagnostic = None;
            let result = self
                .run_attempt(AttemptExecution {
                    snapshot,
                    endpoint_timeouts,
                    request_plan: &request_plan,
                    adapter,
                    attempt_id,
                    client_request_id,
                    cancellation,
                    events,
                    attempt: &mut attempt,
                    diagnostic: &mut attempt_diagnostic,
                })
                .await;
            drop(permit);

            match result? {
                AttemptTerminal::Completed { mut output, terminal_event, next_sequence } => {
                    attempt.state = TransportAttemptState::Completed;
                    attempt.finished_at = Some(UtcTimestamp::now());
                    output.raw_response_ref = attempt.raw_stream_artifact_id;
                    if output.provider_request_id.is_none() {
                        output.provider_request_id = attempt.provider_request_id.clone();
                    }
                    self.store.record_attempt(&attempt).await?;
                    self.store.record_output(snapshot, attempt_id, &output).await?;
                    attempts.push(attempt);
                    send_terminal_event(
                        events,
                        ModelStreamEventEnvelope {
                            model_step_id: snapshot.model_step_id,
                            transport_attempt_id: attempt_id,
                            sequence: next_sequence,
                            received_at: UtcTimestamp::now(),
                            event: terminal_event,
                        },
                    )
                    .await?;
                    return Ok(TransportRunTerminal::Completed { output: *output, attempts });
                }
                AttemptTerminal::Failed { mut failure, emitted_valid_event, next_sequence } => {
                    failure.transport_attempt_id = Some(attempt_id);
                    if emitted_valid_event && failure.partial_output_ref.is_none() {
                        failure.partial_output_ref = attempt.raw_stream_artifact_id;
                    }
                    if failure.category == aura_model_domain::ModelFailureCategory::Cancelled {
                        let raw_artifact_id = attempt.raw_stream_artifact_id;
                        finish_cancelled_attempt(&self.store, &mut attempt, raw_artifact_id)
                            .await?;
                        attempts.push(attempt);
                        send_terminal_normalized_failure(
                            events,
                            snapshot,
                            attempt_id,
                            next_sequence,
                            &failure,
                        )
                        .await?;
                        return Ok(TransportRunTerminal::Cancelled {
                            cancellation: failure,
                            attempts,
                        });
                    }
                    let persisted = persisted_failure(failure.clone());
                    let raw_artifact_id = attempt.raw_stream_artifact_id;
                    finish_failed_attempt(&self.store, &mut attempt, &persisted, raw_artifact_id)
                        .await?;
                    let endpoint_changed = request_factory.current_endpoint_fingerprint()?
                        != snapshot.endpoint_fingerprint;
                    let decision = self.retry_policy.decide(
                        attempt_number,
                        &failure,
                        emitted_valid_event,
                        cancellation.is_cancelled(),
                        endpoint_changed,
                        random_jitter(self.retry_policy.jitter_limit())?,
                    );
                    attempts.push(attempt);
                    match decision {
                        RetryDecision::DoNotRetry => {
                            send_terminal_failure(
                                events,
                                snapshot,
                                attempt_id,
                                next_sequence,
                                &persisted,
                            )
                            .await?;
                            return Ok(TransportRunTerminal::Failed {
                                failure: persisted,
                                attempts,
                                diagnostic: attempt_diagnostic,
                            });
                        }
                        RetryDecision::RetryAfter(delay) => {
                            if let Err(ModelTransportError::Cancelled) =
                                wait_retry_delay(delay, cancellation).await
                            {
                                let cancellation = cancelled_failure(Some(attempt_id), None);
                                send_terminal_normalized_failure(
                                    events,
                                    snapshot,
                                    attempt_id,
                                    next_sequence,
                                    &cancellation,
                                )
                                .await?;
                                return Ok(TransportRunTerminal::Cancelled {
                                    cancellation,
                                    attempts,
                                });
                            }
                            next_kind = TransportAttemptKind::TransparentRetry;
                        }
                        RetryDecision::CompatibilityAdjustment { parameter } => {
                            if !request_plan.safe_optional_parameters.contains(&parameter)
                                || !applied_overrides.insert(parameter)
                            {
                                send_terminal_failure(
                                    events,
                                    snapshot,
                                    attempt_id,
                                    next_sequence,
                                    &persisted,
                                )
                                .await?;
                                return Ok(TransportRunTerminal::Failed {
                                    failure: persisted,
                                    attempts,
                                    diagnostic: attempt_diagnostic,
                                });
                            }
                            next_kind = TransportAttemptKind::CompatibilityAdjustedRetry;
                        }
                    }
                }
            }
        }
        Err(ModelTransportError::InvalidProviderRequest(
            "transport retry loop exhausted without a terminal result".to_owned(),
        ))
    }
}

fn validate_compiled_request(
    request_plan: &ProviderRequestPlan,
    expected_overrides: &BTreeSet<String>,
    expected_operation_context: &TransportOperationContext,
    snapshot: &ModelExecutionSnapshot,
) -> Result<(), ModelTransportError> {
    if !request_plan.safe_optional_parameters.is_empty() {
        return Err(ModelTransportError::InvalidProviderRequest(
            "Adapter Compilation Contract v1 has an empty compatibility-removal whitelist"
                .to_owned(),
        ));
    }
    request_plan.request_evidence.validate().map_err(|error| {
        ModelTransportError::InvalidProviderRequest(format!(
            "compiled provider request evidence is invalid: {error}"
        ))
    })?;
    if request_plan.request_evidence.canonical_body_artifact_id.is_some() {
        return Err(ModelTransportError::InvalidProviderRequest(
            "adapter must not pre-populate the canonical request body artifact".to_owned(),
        ));
    }
    let body_byte_length = u64::try_from(request_plan.body.len()).map_err(|_| {
        ModelTransportError::InvalidProviderRequest(
            "compiled provider request body length exceeds u64".to_owned(),
        )
    })?;
    if request_plan.request_evidence.body_byte_length != body_byte_length
        || request_plan.request_evidence.body_sha256 != ContentHash::sha256(&request_plan.body)
    {
        return Err(ModelTransportError::InvalidProviderRequest(
            "compiled provider request body does not match its SHA-256 evidence".to_owned(),
        ));
    }
    if request_plan.request_evidence.operation_context != *expected_operation_context
        || request_plan.request_evidence.adapter.api_format != snapshot.api_format
    {
        return Err(ModelTransportError::InvalidProviderRequest(
            "compiled provider request context or adapter does not match the execution snapshot"
                .to_owned(),
        ));
    }
    if request_plan.request_evidence.compatibility_removals != *expected_overrides
        || request_plan
            .request_evidence
            .compatibility_removals
            .iter()
            .any(|parameter| !request_plan.safe_optional_parameters.contains(parameter))
    {
        return Err(ModelTransportError::InvalidProviderRequest(
            "compiled compatibility overrides are not exact or not whitelisted".to_owned(),
        ));
    }
    validate_request_target(request_plan)?;
    validate_request_headers(request_plan, snapshot.auth_scheme)?;
    Ok(())
}

fn validate_request_target(request_plan: &ProviderRequestPlan) -> Result<(), ModelTransportError> {
    let target = &request_plan.request_evidence.target;
    let url = url::Url::parse(&request_plan.request_url).map_err(|_| {
        ModelTransportError::InvalidProviderRequest(
            "compiled provider request URL is invalid".to_owned(),
        )
    })?;
    let host = url.host_str().ok_or_else(|| {
        ModelTransportError::InvalidProviderRequest(
            "compiled provider request URL has no host".to_owned(),
        )
    })?;
    let port = url.port_or_known_default().ok_or_else(|| {
        ModelTransportError::InvalidProviderRequest(
            "compiled provider request URL has no effective port".to_owned(),
        )
    })?;
    let query_parameter_names =
        url.query_pairs().map(|(name, _)| name.into_owned()).collect::<BTreeSet<_>>();
    if !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || request_plan.method != target.method
        || url.scheme() != target.origin.scheme
        || host != target.origin.host
        || port != target.origin.port
        || url.path() != target.path
        || query_parameter_names != target.query_parameter_names
    {
        return Err(ModelTransportError::InvalidProviderRequest(
            "compiled provider request target does not match its redacted evidence".to_owned(),
        ));
    }
    Ok(())
}

fn validate_request_headers(
    request_plan: &ProviderRequestPlan,
    auth_scheme: EndpointAuthScheme,
) -> Result<(), ModelTransportError> {
    for (name, value) in &request_plan.headers {
        let canonical_name = name.to_ascii_lowercase();
        if canonical_name != *name
            || matches!(canonical_name.as_str(), "authorization" | "x-api-key")
            || !request_plan.request_evidence.headers.iter().any(|metadata| {
                metadata.name == canonical_name
                    && metadata.class != ProviderRequestHeaderClass::CredentialInjected
                    && metadata.non_secret_value.as_deref() == Some(value.as_str())
            })
        {
            return Err(ModelTransportError::InvalidProviderRequest(
                "compiled provider request headers do not match their redacted evidence".to_owned(),
            ));
        }
    }
    let expected_credential_header = match auth_scheme {
        EndpointAuthScheme::None => None,
        EndpointAuthScheme::OpenAiBearerApiKey => Some("authorization"),
        EndpointAuthScheme::AnthropicApiKey => Some("x-api-key"),
    };
    let credential_headers = request_plan
        .request_evidence
        .headers
        .iter()
        .filter(|metadata| metadata.class == ProviderRequestHeaderClass::CredentialInjected)
        .collect::<Vec<_>>();
    if credential_headers.len() != usize::from(expected_credential_header.is_some())
        || expected_credential_header.is_some_and(|expected| {
            credential_headers.first().is_none_or(|metadata| metadata.name != expected)
        })
    {
        return Err(ModelTransportError::InvalidProviderRequest(
            "compiled provider request credential header evidence does not match the auth scheme"
                .to_owned(),
        ));
    }
    Ok(())
}
