use std::{collections::BTreeSet, sync::Arc, time::Instant};

use aura_domain_common::{
    ArtifactId, ContentHash, CorrelationId, ModelFailureId, NetworkRequestId, NormalizedIssueCode,
    Revision, TransportAttemptId, UtcTimestamp,
};
use aura_model_domain::{
    FailureFingerprint, ModelFailureCategory, ModelRetryClass, NormalizedModelFailure,
    PersistedModelFailure, TransportAttemptKind, TransportAttemptRecord, TransportAttemptState,
    TransportOperationContext,
};
use aura_model_transport::{ModelCancellationToken, ProviderRequestPlan};
use aura_network_control::{
    ConfigurationOperationRequestManifestV1, ConfigurationRequestCredentialBindingV1,
    ConfigurationRequestDataClass, ConfigurationRequestPurpose, ConfigurationRequestTargetV1,
    ConfigurationResponseMediaPolicyV1, ConfigurationTransportPolicyV1, CoreHttpClientGateway,
    NetworkControlError, ProviderTransportGatewayRequest, ProviderTransportImmutableRequest,
    ProviderTransportRequestAuthority, SecretScanner, UserConfigurationOperationIdentity,
    canonical_non_secret_headers_hash, issue_configuration_provider_transport_grant,
};
use aura_permission_domain::RedirectPolicy;
use aura_persistence::{PersistenceCoordinator, PersistenceError};
use thiserror::Error;
use url::Url;

use super::transport_store::PersistentTransportStore;

#[derive(Clone, Debug)]
pub struct ConfigurationTransportResponse {
    pub attempt: TransportAttemptRecord,
    pub status: u16,
    pub body: Vec<u8>,
    pub raw_response_artifact_id: Option<ArtifactId>,
    pub elapsed_ms: u64,
}

#[derive(Clone, Debug)]
pub struct ConfigurationTransportFailure {
    pub attempt: TransportAttemptRecord,
    pub failure: PersistedModelFailure,
    pub issue_code: NormalizedIssueCode,
    pub http_response_received: bool,
}

#[derive(Clone, Debug)]
pub enum ConfigurationTransportOutcome {
    Response(Box<ConfigurationTransportResponse>),
    Failed(Box<ConfigurationTransportFailure>),
    Cancelled(Box<ConfigurationTransportCancelled>),
}

#[derive(Clone, Debug)]
pub struct ConfigurationTransportCancelled {
    pub attempt: TransportAttemptRecord,
}

struct AttemptFailureDetails {
    issue_code: NormalizedIssueCode,
    category: ModelFailureCategory,
    retryable: bool,
    provider_status: Option<u16>,
    http_response_received: bool,
}

pub type ConfigurationAttemptBinder =
    Arc<dyn Fn(&TransportAttemptRecord) -> Result<(), String> + Send + Sync + 'static>;

pub struct ConfigurationTransportExecuteRequest {
    pub identity: UserConfigurationOperationIdentity,
    pub correlation_id: CorrelationId,
    pub models_revision: Revision,
    pub endpoint_id: aura_domain_common::EndpointId,
    pub endpoint_security_fingerprint: ContentHash,
    pub purpose: ConfigurationRequestPurpose,
    pub operation_context: TransportOperationContext,
    pub request_plan: ProviderRequestPlan,
    pub credential_binding: Option<ConfigurationRequestCredentialBindingV1>,
    pub attempt_number: u32,
    pub attempt_kind: TransportAttemptKind,
    pub timeout_ms: u64,
    pub cancellation: ModelCancellationToken,
    pub persist_and_bind_attempt: ConfigurationAttemptBinder,
}

#[derive(Debug, Error)]
pub enum ConfigurationTransportError {
    #[error("configuration transport request does not match its frozen operation")]
    BindingMismatch,
    #[error("configuration transport request target is invalid")]
    InvalidTarget,
    #[error("configuration transport request exceeds its frozen byte policy")]
    RequestTooLarge,
    #[error("configuration transport contract is invalid: {0}")]
    Contract(String),
    #[error("configuration transport attempt could not be bound to its operation: {0}")]
    OperationBinding(String),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
}

#[derive(Clone)]
pub struct ConfigurationTransport {
    gateway: Arc<CoreHttpClientGateway>,
    persistence: Arc<PersistenceCoordinator>,
    transport_store: Arc<PersistentTransportStore>,
}

impl ConfigurationTransport {
    #[must_use]
    pub fn new(
        gateway: Arc<CoreHttpClientGateway>,
        persistence: Arc<PersistenceCoordinator>,
        transport_store: Arc<PersistentTransportStore>,
    ) -> Self {
        Self { gateway, persistence, transport_store }
    }

    pub async fn execute(
        &self,
        request: ConfigurationTransportExecuteRequest,
    ) -> Result<ConfigurationTransportOutcome, ConfigurationTransportError> {
        let ConfigurationTransportExecuteRequest {
            identity,
            correlation_id,
            models_revision,
            endpoint_id,
            endpoint_security_fingerprint,
            purpose,
            operation_context,
            request_plan,
            credential_binding,
            attempt_number,
            attempt_kind,
            timeout_ms,
            cancellation,
            persist_and_bind_attempt,
        } = request;
        validate_operation_binding(
            &identity,
            models_revision,
            endpoint_id,
            &purpose,
            &operation_context,
            &request_plan,
        )?;
        if timeout_ms == 0 {
            return Err(ConfigurationTransportError::Contract(
                "provider request timeout must be non-zero".to_owned(),
            ));
        }

        let transport_attempt_id = TransportAttemptId::new();
        let network_request_id = NetworkRequestId::new();
        let request_url = Url::parse(&request_plan.request_url)
            .map_err(|_| ConfigurationTransportError::InvalidTarget)?;
        let origin = request_plan.request_evidence.target.origin.clone();
        let mut path_and_query = request_url.path().to_owned();
        if let Some(query) = request_url.query() {
            path_and_query.push('?');
            path_and_query.push_str(query);
        }
        let limits = ConfigurationTransportPolicyV1::limits_for(&purpose);
        if request_plan.request_evidence.body_byte_length > limits.max_request_body_bytes {
            return Err(ConfigurationTransportError::RequestTooLarge);
        }
        let response_media_policy = ConfigurationResponseMediaPolicyV1 {
            accepted_media_types: BTreeSet::from(["application/json".to_owned()]),
            require_content_type: true,
        };
        let target = ConfigurationRequestTargetV1 {
            method: request_plan.method.clone(),
            origin,
            path_and_query,
        };
        let non_secret_headers_hash = canonical_non_secret_headers_hash(&request_plan.headers)
            .map_err(|error| ConfigurationTransportError::Contract(error.to_string()))?;
        let manifest = ConfigurationOperationRequestManifestV1 {
            schema_version: aura_network_control::CONFIGURATION_MANIFEST_SCHEMA_VERSION,
            command_invocation_id: identity.command_invocation_id(),
            correlation_id,
            user_configuration_operation_identity_binding_hash: identity
                .binding_hash()
                .map_err(|error| ConfigurationTransportError::Contract(error.to_string()))?,
            purpose,
            transport_attempt_id,
            endpoint_id,
            models_config_revision: models_revision,
            endpoint_security_fingerprint: endpoint_security_fingerprint.clone(),
            target,
            non_secret_headers_hash,
            request_body_hash: request_plan.request_evidence.body_sha256.clone(),
            request_body_length: request_plan.request_evidence.body_byte_length,
            credential_binding,
            max_request_bytes: limits.max_request_body_bytes,
            max_response_bytes: limits.max_response_wire_bytes,
            max_decompressed_response_bytes: limits.max_response_decompressed_bytes,
            redirect_policy: RedirectPolicy::SameOrigin {
                max_hops: aura_network_control::CONFIGURATION_REDIRECT_MAX_HOPS,
            },
            response_media_policy,
            data_class: ConfigurationRequestDataClass::ProtocolMetadata,
        };
        manifest
            .validate(&identity, &operation_context)
            .map_err(|error| ConfigurationTransportError::Contract(error.to_string()))?;

        let started_at = UtcTimestamp::now();
        let mut evidence = request_plan.request_evidence.clone();
        evidence.operation_context = operation_context.clone();
        evidence.canonical_body_artifact_id = None;
        let mut attempt = TransportAttemptRecord {
            id: transport_attempt_id,
            attempt_number,
            attempt_kind,
            client_request_id: network_request_id.to_string(),
            provider_request_id: None,
            endpoint_id,
            endpoint_fingerprint: endpoint_security_fingerprint,
            request_evidence: evidence,
            state: TransportAttemptState::AuthorizingNetwork,
            started_at,
            first_byte_at: None,
            first_model_event_at: None,
            finished_at: None,
            raw_stream_artifact_id: None,
            failure_id: None,
        };
        if let Err(error) = persist_and_bind_attempt(&attempt) {
            return Err(ConfigurationTransportError::OperationBinding(error));
        }

        let grant = match issue_configuration_provider_transport_grant(
            &identity,
            operation_context,
            &manifest,
            network_request_id,
            UtcTimestamp::now(),
        ) {
            Ok(grant) => grant,
            Err(_) => {
                return self.fail_attempt_with_issue(
                    attempt,
                    AttemptFailureDetails {
                        issue_code: NormalizedIssueCode::new("transport.authorization_failed")
                            .expect("static P05 issue code is registered"),
                        category: ModelFailureCategory::Authorization,
                        retryable: false,
                        provider_status: None,
                        http_response_received: false,
                    },
                );
            }
        };
        self.transition_intermediate_attempt(
            correlation_id,
            &mut attempt,
            TransportAttemptState::Authorized,
        )?;
        let gateway_request = ProviderTransportGatewayRequest {
            authority: ProviderTransportRequestAuthority::UserConfiguration(identity),
            grant,
            immutable_request: ProviderTransportImmutableRequest {
                request_id: network_request_id,
                method: request_plan.method,
                requested_url: request_plan.request_url,
                headers: request_plan.headers,
                body: request_plan.body,
                redirect_policy: manifest.redirect_policy,
                byte_limits: aura_permission_domain::NetworkByteLimits {
                    max_upload_bytes: manifest.max_request_bytes,
                    max_download_bytes: manifest.max_response_bytes,
                    max_decompressed_bytes: manifest.max_decompressed_response_bytes,
                },
                connect_timeout_ms: timeout_ms,
                timeout_ms: Some(timeout_ms),
            },
        };

        self.transition_intermediate_attempt(
            correlation_id,
            &mut attempt,
            TransportAttemptState::DispatchPrepared,
        )?;
        self.transition_intermediate_attempt(
            correlation_id,
            &mut attempt,
            TransportAttemptState::Connecting,
        )?;

        let started = Instant::now();
        let gateway_result = tokio::select! {
            biased;
            () = cancellation.cancelled() => {
                attempt.state = TransportAttemptState::Cancelled;
                attempt.finished_at = Some(UtcTimestamp::now());
                return Ok(ConfigurationTransportOutcome::Cancelled(Box::new(
                    ConfigurationTransportCancelled { attempt },
                )));
            }
            result = self.gateway.execute_provider_transport_observed(gateway_request) => result,
        };
        match gateway_result {
            Ok(response) => {
                let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                attempt.first_byte_at = Some(UtcTimestamp::now());
                self.transition_intermediate_attempt(
                    correlation_id,
                    &mut attempt,
                    TransportAttemptState::ResponseStarted,
                )?;
                match response.body {
                    Ok(body) => {
                        self.complete_response(
                            correlation_id,
                            &mut attempt,
                            response.status,
                            response.headers,
                            body,
                            elapsed_ms,
                        )
                        .await
                    }
                    Err(error) => self.fail_attempt(attempt, &error, Some(response.status), true),
                }
            }
            Err(error) => self.fail_attempt(attempt, &error, None, false),
        }
    }

    fn transition_intermediate_attempt(
        &self,
        correlation_id: CorrelationId,
        attempt: &mut TransportAttemptRecord,
        next_state: TransportAttemptState,
    ) -> Result<(), ConfigurationTransportError> {
        let persisted_attempt = attempt.clone();
        attempt.state = next_state;
        match self.persistence.transition_transport_attempt(correlation_id, attempt) {
            Ok(_) => Ok(()),
            Err(transition_error) => {
                *attempt = persisted_attempt;
                Err(ConfigurationTransportError::Persistence(transition_error))
            }
        }
    }

    async fn complete_response(
        &self,
        correlation_id: CorrelationId,
        attempt: &mut TransportAttemptRecord,
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        elapsed_ms: u64,
    ) -> Result<ConfigurationTransportOutcome, ConfigurationTransportError> {
        let mut artifact_body = body.clone();
        if !(200..300).contains(&status)
            && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&artifact_body)
        {
            artifact_body = match serde_json::to_vec(&SecretScanner::redact_provider_error(&value))
            {
                Ok(body) => body,
                Err(_) => {
                    return self.fail_attempt_with_issue(
                        attempt.clone(),
                        AttemptFailureDetails {
                            issue_code: NormalizedIssueCode::new("internal.unexpected")
                                .expect("static P05 issue code is registered"),
                            category: ModelFailureCategory::Unknown,
                            retryable: false,
                            provider_status: Some(status),
                            http_response_received: true,
                        },
                    );
                }
            };
        }
        let media_type = response_media_type(&headers);
        let media_type_accepted = media_type.as_deref() == Some("application/json");
        let artifact_id = if media_type_accepted {
            match std::str::from_utf8(&artifact_body) {
                Ok(text) => match SecretScanner.scan_text("provider.response", text) {
                    Ok(()) => match self
                        .transport_store
                        .write_operation_artifact(
                            &attempt.request_evidence.operation_context,
                            "application/json".to_owned(),
                            artifact_body,
                        )
                        .await
                    {
                        Ok(artifact_id) => Some(artifact_id),
                        Err(_) => {
                            return self.fail_attempt_with_issue(
                                attempt.clone(),
                                AttemptFailureDetails {
                                    issue_code: NormalizedIssueCode::new(
                                        "persistence.artifact_write_failed",
                                    )
                                    .expect("static P05 issue code is registered"),
                                    category: ModelFailureCategory::Persistence,
                                    retryable: false,
                                    provider_status: Some(status),
                                    http_response_received: true,
                                },
                            );
                        }
                    },
                    Err(error) => {
                        return self.fail_attempt(attempt.clone(), &error, Some(status), true);
                    }
                },
                Err(_) => None,
            }
        } else {
            None
        };
        attempt.raw_stream_artifact_id = artifact_id;
        self.transition_intermediate_attempt(
            correlation_id,
            attempt,
            TransportAttemptState::ResponseCompleted,
        )?;
        attempt.state = TransportAttemptState::Completed;
        attempt.finished_at = Some(UtcTimestamp::now());
        Ok(ConfigurationTransportOutcome::Response(Box::new(ConfigurationTransportResponse {
            attempt: attempt.clone(),
            status,
            body,
            raw_response_artifact_id: artifact_id,
            elapsed_ms,
        })))
    }

    fn fail_attempt(
        &self,
        attempt: TransportAttemptRecord,
        error: &NetworkControlError,
        provider_status: Option<u16>,
        http_response_received: bool,
    ) -> Result<ConfigurationTransportOutcome, ConfigurationTransportError> {
        let (issue_code, category, retryable) = normalized_transport_issue(error);
        self.fail_attempt_with_issue(
            attempt,
            AttemptFailureDetails {
                issue_code,
                category,
                retryable,
                provider_status,
                http_response_received,
            },
        )
    }

    fn fail_attempt_with_issue(
        &self,
        mut attempt: TransportAttemptRecord,
        details: AttemptFailureDetails,
    ) -> Result<ConfigurationTransportOutcome, ConfigurationTransportError> {
        let AttemptFailureDetails {
            issue_code,
            category,
            retryable,
            provider_status,
            http_response_received,
        } = details;
        let model_failure_id = ModelFailureId::new();
        let failure = PersistedModelFailure {
            id: model_failure_id,
            fingerprint: FailureFingerprint(ContentHash::sha256(
                format!(
                    "{}|{}|{}",
                    attempt.endpoint_id,
                    issue_code.as_str(),
                    operation_context_name(&attempt.request_evidence.operation_context)
                )
                .as_bytes(),
            )),
            failure: NormalizedModelFailure {
                category,
                code: issue_code.as_str().to_owned(),
                message: "Provider transport failed before a complete response was accepted."
                    .to_owned(),
                retry_class: if retryable {
                    ModelRetryClass::TransparentRetry { retry_after_ms: None }
                } else {
                    ModelRetryClass::NotRetryable
                },
                provider_status,
                provider_request_id: None,
                provider_error_ref: None,
                partial_output_ref: None,
                transport_attempt_id: Some(attempt.id),
            },
            created_at: UtcTimestamp::now(),
        };
        self.persistence.store_model_failure(&failure)?;
        attempt.state = TransportAttemptState::Failed;
        attempt.failure_id = Some(model_failure_id);
        attempt.finished_at = Some(UtcTimestamp::now());
        Ok(ConfigurationTransportOutcome::Failed(Box::new(ConfigurationTransportFailure {
            attempt,
            failure,
            issue_code,
            http_response_received,
        })))
    }
}

fn validate_operation_binding(
    identity: &UserConfigurationOperationIdentity,
    models_revision: Revision,
    endpoint_id: aura_domain_common::EndpointId,
    purpose: &ConfigurationRequestPurpose,
    operation_context: &TransportOperationContext,
    request_plan: &ProviderRequestPlan,
) -> Result<(), ConfigurationTransportError> {
    let context_matches = match (purpose, operation_context) {
        (
            ConfigurationRequestPurpose::ModelDiscovery { model_discovery_run_id },
            TransportOperationContext::ModelDiscovery { discovery_run_id },
        ) => model_discovery_run_id == discovery_run_id,
        (
            ConfigurationRequestPurpose::ModelAvailabilityProbe {
                model_availability_test_id,
                model_profile_id,
            },
            TransportOperationContext::ModelAvailabilityTest {
                model_availability_test_id: context_test_id,
                model_profile_id: context_profile_id,
            },
        ) => {
            model_availability_test_id == context_test_id && model_profile_id == context_profile_id
        }
        _ => false,
    };
    if identity.target_endpoint_id() != endpoint_id
        || identity.config_revision() != models_revision
        || !context_matches
        || request_plan.request_evidence.operation_context != *operation_context
        || request_plan.request_evidence.body_sha256 != ContentHash::sha256(&request_plan.body)
        || request_plan.request_evidence.body_byte_length
            != u64::try_from(request_plan.body.len()).unwrap_or(u64::MAX)
    {
        return Err(ConfigurationTransportError::BindingMismatch);
    }
    Ok(())
}

fn response_media_type(headers: &[(String, String)]) -> Option<String> {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .and_then(|(_, value)| value.split(';').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
}

const fn operation_context_name(context: &TransportOperationContext) -> &'static str {
    match context {
        TransportOperationContext::AgentModelAttempt { .. } => "agent_model_attempt",
        TransportOperationContext::ModelDiscovery { .. } => "model_discovery",
        TransportOperationContext::ModelAvailabilityTest { .. } => "model_availability_test",
    }
}

fn normalized_transport_issue(
    error: &NetworkControlError,
) -> (NormalizedIssueCode, ModelFailureCategory, bool) {
    let (code, category, retryable) = match error {
        NetworkControlError::Timeout => ("network.timeout", ModelFailureCategory::Transport, true),
        NetworkControlError::TlsServerNameMismatch => {
            ("tls.server_name_mismatch", ModelFailureCategory::Transport, false)
        }
        NetworkControlError::RedirectBlocked | NetworkControlError::RedirectLimitExceeded => {
            ("network.redirect_blocked", ModelFailureCategory::Transport, false)
        }
        NetworkControlError::ContentEncodingUnsupported => {
            ("transport.content_encoding_unsupported", ModelFailureCategory::Protocol, false)
        }
        NetworkControlError::DecompressionFailed => {
            ("transport.decompression_failed", ModelFailureCategory::Protocol, false)
        }
        NetworkControlError::ResponseTooLarge
        | NetworkControlError::DownloadLimitExceeded
        | NetworkControlError::UploadLimitExceeded => {
            ("transport.response_too_large", ModelFailureCategory::Transport, false)
        }
        NetworkControlError::CredentialBindingMismatch => {
            ("credential.binding_mismatch", ModelFailureCategory::Authentication, false)
        }
        NetworkControlError::CredentialUnavailable => {
            ("credential.unavailable", ModelFailureCategory::Authentication, false)
        }
        NetworkControlError::SecretDetected { .. }
        | NetworkControlError::CredentialMaterialForbidden => {
            ("transport.secret_detected", ModelFailureCategory::Authorization, false)
        }
        NetworkControlError::RateLimited => {
            ("provider.rate_limited", ModelFailureCategory::RateLimit, true)
        }
        NetworkControlError::Transport(_) => {
            ("network.transport_failed", ModelFailureCategory::Transport, true)
        }
        _ => ("internal.unexpected", ModelFailureCategory::Unknown, false),
    };
    (
        NormalizedIssueCode::new(code).expect("static P05 issue code is registered"),
        category,
        retryable,
    )
}
