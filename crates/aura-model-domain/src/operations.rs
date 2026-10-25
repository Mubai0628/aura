use std::collections::BTreeSet;

use aura_domain_common::{
    ArtifactId, ContentHash, CorrelationId, EndpointId, ModelAvailabilityTestId,
    ModelDiscoveryRunId, ModelFailureId, ModelProfileId, NormalizedIssueCode, Revision,
    TransportAttemptId, UtcTimestamp,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    ChatOutputTokenFieldAlias, CredentialBindingFingerprint, EndpointAuthScheme,
    EndpointCredentialState, EndpointCredentialStatus, ModelDiscoveryMergeResult,
    ModelDiscoveryOutcome, ModelDiscoveryStage, ModelDiscoveryState, ModelEndpointConfig,
    ModelProfileConfig,
};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderErrorSummaryView {
    pub issue_code: NormalizedIssueCode,
    pub provider_status: Option<u16>,
    pub provider_message: Option<String>,
    pub message_was_redacted: bool,
    pub message_was_truncated: bool,
    pub provider_request_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelOperationCancellationReason {
    UserRequested,
    ApplicationRestarted,
    ConfigurationChanged,
    TargetDeleted,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDiscoveryRunRecord {
    pub id: ModelDiscoveryRunId,
    pub correlation_id: CorrelationId,
    pub endpoint_id: EndpointId,
    pub source_models_revision: Revision,
    pub endpoint_revision_at_start: Revision,
    pub endpoint_fingerprint: ContentHash,
    pub revision: Revision,
    pub state: ModelDiscoveryState,
    pub stage: Option<ModelDiscoveryStage>,
    pub outcome: Option<ModelDiscoveryOutcome>,
    pub cancellation_reason: Option<ModelOperationCancellationReason>,
    pub transport_attempt_ids: Vec<TransportAttemptId>,
    pub result: Option<ModelDiscoveryMergeResult>,
    pub failure_id: Option<ModelFailureId>,
    pub provider_error: Option<ProviderErrorSummaryView>,
    pub raw_response_artifact_id: Option<ArtifactId>,
    pub started_at: UtcTimestamp,
    pub finished_at: Option<UtcTimestamp>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelAvailabilityTestState {
    Queued,
    Running,
    Completed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelAvailabilityTestStage {
    Config,
    Network,
    Tls,
    Http,
    Authentication,
    ModelGeneration,
    Completed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelAvailabilityTestOutcome {
    Available,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelAvailabilityCheckNotRunReason {
    BlockedByPreviousStage,
    NotApplicableForScheme,
    NoAuthenticationRequired,
    AuthenticationNotConfirmed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelDiscoveryUnsupportedReason {
    ModelsEndpointDisabledByConfiguration,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelAvailabilityCheckResult {
    Passed { latency_ms: Option<u64> },
    Failed { error_code: NormalizedIssueCode, provider_status: Option<u16> },
    NotRun { reason_code: ModelAvailabilityCheckNotRunReason },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAvailabilityTestResult {
    pub outcome: ModelAvailabilityTestOutcome,
    pub config: ModelAvailabilityCheckResult,
    pub network: ModelAvailabilityCheckResult,
    pub tls: ModelAvailabilityCheckResult,
    pub http: ModelAvailabilityCheckResult,
    pub authentication: ModelAvailabilityCheckResult,
    pub model_generation: ModelAvailabilityCheckResult,
    pub tested_endpoint_id: EndpointId,
    pub tested_model_profile_id: ModelProfileId,
    pub tested_model_name: String,
    pub started_at: UtcTimestamp,
    pub completed_at: UtcTimestamp,
    pub warnings: Vec<NormalizedIssueCode>,
    pub provider_error: Option<ProviderErrorSummaryView>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAvailabilityTestRecord {
    pub id: ModelAvailabilityTestId,
    pub correlation_id: CorrelationId,
    pub endpoint_id: EndpointId,
    pub model_profile_id: ModelProfileId,
    pub models_revision_at_start: Revision,
    pub endpoint_revision_at_start: Revision,
    pub model_profile_revision_at_start: Revision,
    pub endpoint_security_fingerprint: ContentHash,
    pub endpoint_snapshot: ModelEndpointConfig,
    pub model_profile_snapshot: ModelProfileConfig,
    pub credential_status_at_start: Option<EndpointCredentialStatus>,
    pub credential_binding_fingerprint: Option<CredentialBindingFingerprint>,
    pub revision: Revision,
    pub state: ModelAvailabilityTestState,
    pub stage: Option<ModelAvailabilityTestStage>,
    pub cancellation_reason: Option<ModelOperationCancellationReason>,
    pub direct_transport_attempt_ids: Vec<TransportAttemptId>,
    pub applied_output_token_field_alias: Option<ChatOutputTokenFieldAlias>,
    pub result: Option<ModelAvailabilityTestResult>,
    pub raw_response_artifact_id: Option<ArtifactId>,
    pub started_at: UtcTimestamp,
    pub completed_at: Option<UtcTimestamp>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ModelOperationContractError {
    #[error("operational and frozen config revisions must start at one")]
    InvalidRevision,
    #[error("transport attempt IDs must be unique within an operation")]
    DuplicateTransportAttempt,
    #[error("discovery run terminal fields do not match its state and outcome")]
    DiscoveryTerminalMismatch,
    #[error("discovery result belongs to a different endpoint")]
    DiscoveryEndpointMismatch,
    #[error("availability-test endpoint snapshot does not match the record")]
    AvailabilityEndpointMismatch,
    #[error("availability-test profile snapshot does not match the record")]
    AvailabilityProfileMismatch,
    #[error("availability-test frozen object revisions do not match their snapshots")]
    AvailabilityRevisionMismatch,
    #[error("availability-test terminal fields do not match its state")]
    AvailabilityTerminalMismatch,
    #[error("availability-test check matrix is inconsistent")]
    AvailabilityCheckMatrix,
    #[error("availability-test outcome is inconsistent with model generation")]
    AvailabilityOutcomeMismatch,
    #[error("availability-test result identity does not match its frozen snapshots")]
    AvailabilityResultIdentityMismatch,
    #[error("availability-test warning codes are not strictly sorted and unique")]
    AvailabilityWarningsNotCanonical,
    #[error("availability-test credential status is invalid")]
    InvalidCredentialStatus,
}

impl ModelDiscoveryState {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }
}

impl ModelDiscoveryRunRecord {
    pub fn validate(&self) -> Result<(), ModelOperationContractError> {
        if self.revision.value() == 0
            || self.source_models_revision.value() == 0
            || self.endpoint_revision_at_start.value() == 0
        {
            return Err(ModelOperationContractError::InvalidRevision);
        }
        if self.transport_attempt_ids.iter().copied().collect::<BTreeSet<_>>().len()
            != self.transport_attempt_ids.len()
        {
            return Err(ModelOperationContractError::DuplicateTransportAttempt);
        }
        if self.result.as_ref().is_some_and(|result| result.endpoint_id != self.endpoint_id) {
            return Err(ModelOperationContractError::DiscoveryEndpointMismatch);
        }

        let fields_match = match self.state {
            ModelDiscoveryState::Queued => {
                self.stage.is_none()
                    && self.outcome.is_none()
                    && self.cancellation_reason.is_none()
                    && self.result.is_none()
                    && self.failure_id.is_none()
                    && self.provider_error.is_none()
                    && self.raw_response_artifact_id.is_none()
                    && self.transport_attempt_ids.is_empty()
                    && self.finished_at.is_none()
            }
            ModelDiscoveryState::Running => {
                self.stage.is_some_and(|stage| stage != ModelDiscoveryStage::Completed)
                    && self.outcome.is_none()
                    && self.cancellation_reason.is_none()
                    && self.result.is_none()
                    && self.failure_id.is_none()
                    && self.finished_at.is_none()
            }
            ModelDiscoveryState::Completed => {
                self.stage == Some(ModelDiscoveryStage::Completed)
                    && self.cancellation_reason.is_none()
                    && self.finished_at.is_some()
                    && match self.outcome {
                        Some(ModelDiscoveryOutcome::Succeeded) => {
                            self.result.is_some() && self.failure_id.is_none()
                        }
                        Some(ModelDiscoveryOutcome::Partial) => self.result.is_some(),
                        Some(ModelDiscoveryOutcome::Failed) => {
                            self.result.is_none() && self.failure_id.is_some()
                        }
                        Some(ModelDiscoveryOutcome::Unsupported) => {
                            self.result.is_none() && self.failure_id.is_none()
                        }
                        None => false,
                    }
            }
            ModelDiscoveryState::Cancelled => {
                self.stage != Some(ModelDiscoveryStage::Completed)
                    && self.outcome.is_none()
                    && self.cancellation_reason.is_some()
                    && self.result.is_none()
                    && self.failure_id.is_none()
                    && self.finished_at.is_some()
            }
        };
        fields_match.then_some(()).ok_or(ModelOperationContractError::DiscoveryTerminalMismatch)
    }
}

impl ModelAvailabilityTestState {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }
}

impl ModelAvailabilityCheckResult {
    #[must_use]
    pub const fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }

    #[must_use]
    pub const fn is_passed(&self) -> bool {
        matches!(self, Self::Passed { .. })
    }

    #[must_use]
    pub const fn is_blocked(&self) -> bool {
        matches!(
            self,
            Self::NotRun {
                reason_code: ModelAvailabilityCheckNotRunReason::BlockedByPreviousStage
            }
        )
    }
}

impl ModelAvailabilityTestResult {
    fn validate_for(
        &self,
        endpoint: &ModelEndpointConfig,
        profile: &ModelProfileConfig,
        started_at: UtcTimestamp,
        completed_at: UtcTimestamp,
    ) -> Result<(), ModelOperationContractError> {
        if self.tested_endpoint_id != endpoint.id
            || self.tested_model_profile_id != profile.id
            || self.tested_model_name != profile.provider_model_name
            || self.started_at != started_at
            || self.completed_at != completed_at
        {
            return Err(ModelOperationContractError::AvailabilityResultIdentityMismatch);
        }
        if !self
            .warnings
            .windows(2)
            .all(|pair| pair[0].as_str().as_bytes() < pair[1].as_str().as_bytes())
        {
            return Err(ModelOperationContractError::AvailabilityWarningsNotCanonical);
        }

        self.validate_check_matrix(endpoint)?;
        let expected_outcome = if self.model_generation.is_passed() {
            ModelAvailabilityTestOutcome::Available
        } else {
            ModelAvailabilityTestOutcome::Unavailable
        };
        if self.outcome != expected_outcome {
            return Err(ModelOperationContractError::AvailabilityOutcomeMismatch);
        }
        Ok(())
    }

    fn validate_check_matrix(
        &self,
        endpoint: &ModelEndpointConfig,
    ) -> Result<(), ModelOperationContractError> {
        if !matches!(
            self.config,
            ModelAvailabilityCheckResult::Passed { .. }
                | ModelAvailabilityCheckResult::Failed { .. }
        ) {
            return Err(ModelOperationContractError::AvailabilityCheckMatrix);
        }
        if self.config.is_failed() {
            return all_blocked([
                &self.network,
                &self.tls,
                &self.http,
                &self.authentication,
                &self.model_generation,
            ]);
        }
        if !matches!(
            self.network,
            ModelAvailabilityCheckResult::Passed { .. }
                | ModelAvailabilityCheckResult::Failed { .. }
        ) {
            return Err(ModelOperationContractError::AvailabilityCheckMatrix);
        }
        if self.network.is_failed() {
            return all_blocked([
                &self.tls,
                &self.http,
                &self.authentication,
                &self.model_generation,
            ]);
        }

        let tls_valid = if endpoint.base_url.starts_with("http://") {
            matches!(
                self.tls,
                ModelAvailabilityCheckResult::NotRun {
                    reason_code: ModelAvailabilityCheckNotRunReason::NotApplicableForScheme
                }
            )
        } else {
            matches!(
                self.tls,
                ModelAvailabilityCheckResult::Passed { .. }
                    | ModelAvailabilityCheckResult::Failed { .. }
            )
        };
        if !tls_valid {
            return Err(ModelOperationContractError::AvailabilityCheckMatrix);
        }
        if self.tls.is_failed() {
            return all_blocked([&self.http, &self.authentication, &self.model_generation]);
        }

        if !matches!(
            self.http,
            ModelAvailabilityCheckResult::Passed { .. }
                | ModelAvailabilityCheckResult::Failed { .. }
        ) {
            return Err(ModelOperationContractError::AvailabilityCheckMatrix);
        }
        if self.http.is_failed() {
            return all_blocked([&self.authentication, &self.model_generation]);
        }

        let authentication_valid = match endpoint.auth_scheme {
            EndpointAuthScheme::None => matches!(
                self.authentication,
                ModelAvailabilityCheckResult::Failed { .. }
                    | ModelAvailabilityCheckResult::NotRun {
                        reason_code: ModelAvailabilityCheckNotRunReason::NoAuthenticationRequired
                            | ModelAvailabilityCheckNotRunReason::AuthenticationNotConfirmed
                    }
            ),
            EndpointAuthScheme::OpenAiBearerApiKey | EndpointAuthScheme::AnthropicApiKey => {
                matches!(
                    self.authentication,
                    ModelAvailabilityCheckResult::Passed { .. }
                        | ModelAvailabilityCheckResult::Failed { .. }
                        | ModelAvailabilityCheckResult::NotRun {
                            reason_code:
                                ModelAvailabilityCheckNotRunReason::AuthenticationNotConfirmed
                        }
                )
            }
        };
        if !authentication_valid {
            return Err(ModelOperationContractError::AvailabilityCheckMatrix);
        }
        if self.authentication.is_failed() {
            return self
                .model_generation
                .is_blocked()
                .then_some(())
                .ok_or(ModelOperationContractError::AvailabilityCheckMatrix);
        }
        if matches!(
            self.authentication,
            ModelAvailabilityCheckResult::NotRun {
                reason_code: ModelAvailabilityCheckNotRunReason::AuthenticationNotConfirmed
            }
        ) && !self.model_generation.is_failed()
        {
            return Err(ModelOperationContractError::AvailabilityCheckMatrix);
        }
        if !matches!(
            self.model_generation,
            ModelAvailabilityCheckResult::Passed { .. }
                | ModelAvailabilityCheckResult::Failed { .. }
        ) {
            return Err(ModelOperationContractError::AvailabilityCheckMatrix);
        }
        Ok(())
    }
}

fn all_blocked<const N: usize>(
    checks: [&ModelAvailabilityCheckResult; N],
) -> Result<(), ModelOperationContractError> {
    checks
        .iter()
        .all(|check| check.is_blocked())
        .then_some(())
        .ok_or(ModelOperationContractError::AvailabilityCheckMatrix)
}

impl ModelAvailabilityTestRecord {
    pub fn validate(&self) -> Result<(), ModelOperationContractError> {
        if self.revision.value() == 0
            || self.models_revision_at_start.value() == 0
            || self.endpoint_revision_at_start.value() == 0
            || self.model_profile_revision_at_start.value() == 0
        {
            return Err(ModelOperationContractError::InvalidRevision);
        }
        if self.endpoint_snapshot.id != self.endpoint_id
            || self.model_profile_snapshot.endpoint_id != self.endpoint_id
            || !self.endpoint_snapshot.model_order.contains(&self.model_profile_id)
        {
            return Err(ModelOperationContractError::AvailabilityEndpointMismatch);
        }
        if self.model_profile_snapshot.id != self.model_profile_id {
            return Err(ModelOperationContractError::AvailabilityProfileMismatch);
        }
        if self.endpoint_snapshot.revision != self.endpoint_revision_at_start
            || self.model_profile_snapshot.revision != self.model_profile_revision_at_start
        {
            return Err(ModelOperationContractError::AvailabilityRevisionMismatch);
        }
        if self.direct_transport_attempt_ids.iter().copied().collect::<BTreeSet<_>>().len()
            != self.direct_transport_attempt_ids.len()
        {
            return Err(ModelOperationContractError::DuplicateTransportAttempt);
        }
        match (
            self.endpoint_snapshot.auth_scheme,
            self.credential_status_at_start.as_ref(),
            self.credential_binding_fingerprint.as_ref(),
        ) {
            (EndpointAuthScheme::None, None, None) => {}
            (
                EndpointAuthScheme::OpenAiBearerApiKey | EndpointAuthScheme::AnthropicApiKey,
                Some(status),
                Some(_),
            ) if status.state == EndpointCredentialState::Configured => status
                .validate()
                .map_err(|_| ModelOperationContractError::InvalidCredentialStatus)?,
            _ => return Err(ModelOperationContractError::InvalidCredentialStatus),
        }

        let fields_match = match self.state {
            ModelAvailabilityTestState::Queued => {
                self.stage.is_none()
                    && self.cancellation_reason.is_none()
                    && self.result.is_none()
                    && self.completed_at.is_none()
                    && self.direct_transport_attempt_ids.is_empty()
                    && self.raw_response_artifact_id.is_none()
            }
            ModelAvailabilityTestState::Running => {
                self.stage.is_some_and(|stage| stage != ModelAvailabilityTestStage::Completed)
                    && self.cancellation_reason.is_none()
                    && self.result.is_none()
                    && self.completed_at.is_none()
            }
            ModelAvailabilityTestState::Completed => {
                self.stage == Some(ModelAvailabilityTestStage::Completed)
                    && self.cancellation_reason.is_none()
                    && self.result.is_some()
                    && self.completed_at.is_some()
                    && !self.direct_transport_attempt_ids.is_empty()
            }
            ModelAvailabilityTestState::Cancelled => {
                self.stage != Some(ModelAvailabilityTestStage::Completed)
                    && self.cancellation_reason.is_some()
                    && self.result.is_none()
                    && self.completed_at.is_some()
            }
        };
        if !fields_match {
            return Err(ModelOperationContractError::AvailabilityTerminalMismatch);
        }
        if let (Some(result), Some(completed_at)) = (&self.result, self.completed_at) {
            result.validate_for(
                &self.endpoint_snapshot,
                &self.model_profile_snapshot,
                self.started_at,
                completed_at,
            )?;
        }
        Ok(())
    }
}
