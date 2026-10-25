use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use aura_domain_common::{
    CommandInvocationId, ContentHash, CorrelationId, EndpointId, ModelAvailabilityTestId,
    ModelProfileId, NormalizedIssueCode, Revision, UtcTimestamp,
};
use aura_model_adapter_anthropic::AnthropicMessagesAdapter;
use aura_model_adapter_openai_chat::OpenAiChatCompletionsAdapter;
use aura_model_adapter_openai_responses::OpenAiResponsesAdapter;
use aura_model_config::{
    LearnChatOutputTokenFieldAliasResult, ModelConfigError, endpoint_fingerprint,
    learn_chat_output_token_field_alias, resolve_api_url,
};
use aura_model_domain::{
    CancellationSource, ChatOutputTokenField, ChatOutputTokenFieldAlias, EndpointAuthScheme,
    EndpointCredentialStatus, ModelApiFormat, ModelAvailabilityCheckNotRunReason,
    ModelAvailabilityCheckResult, ModelAvailabilityPrerequisiteFailure,
    ModelAvailabilityTestOutcome, ModelAvailabilityTestRecord, ModelAvailabilityTestResult,
    ModelAvailabilityTestStage, ModelAvailabilityTestStartRequest, ModelAvailabilityTestState,
    ModelChatOutputTokenFieldAliasLearnedPayload, ModelEndpointConfig, ModelProfileConfig,
    TransportAttemptKind, TransportAttemptRecord, TransportOperationContext,
};
use aura_model_transport::{
    ModelAdapterError, ModelAvailabilityProbeCompilationInput, ModelGenerationProbeProtocol,
    ProviderRequestPlan,
};
use aura_network_control::{
    ConfigurationOperationKind, ConfigurationRequestCredentialBindingV1,
    ConfigurationRequestPurpose, UserConfigurationOperationIdentity,
};
use aura_persistence::{
    MODEL_CONFIG_MUTATION_JOURNAL_KIND, ModelAvailabilityTransportTransition,
    ModelConfigMutationJournalState, ModelProjectionDraft, PersistenceCoordinator,
    PersistenceError,
};
use aura_projection_contract::ProjectionChange;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{
    P05OperationControl, P05Services,
    commands::{prepared_snapshot, user_event},
    config_journal::{ModelConfigCommandPlan, ModelConfigJournalError},
    configuration_transport::{
        ConfigurationTransportError, ConfigurationTransportExecuteRequest,
        ConfigurationTransportFailure, ConfigurationTransportOutcome,
        ConfigurationTransportResponse,
    },
    provider_error::summarize_provider_error,
};

pub(crate) struct PreparedModelAvailabilityTest {
    id: ModelAvailabilityTestId,
    models_revision: Revision,
    endpoint: ModelEndpointConfig,
    profile: ModelProfileConfig,
    endpoint_security_fingerprint: ContentHash,
    credential_status: Option<EndpointCredentialStatus>,
    credential_binding: Option<ConfigurationRequestCredentialBindingV1>,
    operation_context: TransportOperationContext,
    request_plan: ProviderRequestPlan,
    applied_output_token_field_alias: Option<ChatOutputTokenFieldAlias>,
    configured_output_token_field: Option<ChatOutputTokenField>,
    resolved_request_url: String,
}

pub(crate) struct ModelAvailabilityCommandCompletion {
    pub projection_changes: Vec<ProjectionChange>,
}

pub(crate) struct ModelAvailabilityQueueCompletion {
    pub accepted: aura_model_domain::ModelAvailabilityTestStartAccepted,
    pub projection_changes: Vec<ProjectionChange>,
    pub newly_queued: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AliasPersistenceCommit {
    learned: bool,
    alias: ChatOutputTokenFieldAlias,
}

enum AliasPersistenceOutcome {
    Persisted(ChatOutputTokenFieldAlias),
    AlreadyPresent(ChatOutputTokenFieldAlias),
    NotPersistedBeforeForwardPoint,
}

#[derive(Debug, Error)]
pub(crate) enum ModelAvailabilityServiceError {
    #[error("model availability prerequisite failed: {0:?}")]
    Prerequisite(ModelAvailabilityPrerequisiteFailure),
    #[error("model availability persistence failed: {0}")]
    Persistence(#[from] PersistenceError),
    #[error("model availability transport failed: {0}")]
    Transport(#[from] ConfigurationTransportError),
    #[error("model availability adapter failed: {0}")]
    Adapter(#[from] ModelAdapterError),
    #[error("model availability compatibility learning failed after its forward point: {0}")]
    ConfigJournal(#[from] ModelConfigJournalError),
    #[error("compatibility.output_token_field_conflict")]
    OutputTokenFieldAliasConflict,
    #[error("model availability record revision overflow")]
    RevisionOverflow,
    #[error("model availability operation did not publish its scoped projection")]
    MissingProjection,
    #[error("model availability operation was not found")]
    OperationNotFound,
    #[error("model availability worker terminated unexpectedly")]
    WorkerTerminated,
    #[error(
        "model availability operational revision conflict: expected {expected}, actual {actual}"
    )]
    OperationRevisionConflict { expected: u64, actual: u64 },
}

impl P05Services {
    pub(crate) async fn cancel_model_availability_for_deleted_targets(
        &self,
        endpoint_ids: &BTreeSet<EndpointId>,
        profile_ids: &BTreeSet<ModelProfileId>,
    ) -> Result<Vec<ProjectionChange>, ModelAvailabilityServiceError> {
        let targets = self
            .persistence
            .active_model_availability_tests()?
            .into_iter()
            .filter(|record| {
                endpoint_ids.contains(&record.endpoint_id)
                    || profile_ids.contains(&record.model_profile_id)
            })
            .map(|record| record.id)
            .collect::<Vec<_>>();
        for operation_id in targets {
            let Some(record) = self.persistence.model_availability_test(operation_id)? else {
                continue;
            };
            if record.state.is_terminal() {
                continue;
            }
            let Some(control) = self.operation_cancellations.availability(operation_id) else {
                let current = self
                    .persistence
                    .model_availability_test(operation_id)?
                    .ok_or(ModelAvailabilityServiceError::OperationNotFound)?;
                if current.state.is_terminal() {
                    continue;
                }
                return Err(ModelAvailabilityServiceError::WorkerTerminated);
            };
            control.cancel(aura_model_domain::ModelOperationCancellationReason::TargetDeleted);

            if record.state == ModelAvailabilityTestState::Queued {
                // Preserve the same strict Queued CAS used by the public
                // cancel command. A concurrent Running transition delegates
                // terminalization to the Worker.
                match self.commit_availability_cancelled(
                    record,
                    Vec::new(),
                    aura_model_domain::ModelOperationCancellationReason::TargetDeleted,
                ) {
                    Ok(completion) => {
                        self.persistence
                            .publish_model_projection_changes(&completion.projection_changes)?;
                        control.mark_terminal_projection_published();
                    }
                    Err(ModelAvailabilityServiceError::Persistence(
                        PersistenceError::ModelAvailabilityTestStateConflict,
                    )) => {}
                    Err(error) => return Err(error),
                }
            }

            control.wait_for_terminal_projection().await;
            let current = self
                .persistence
                .model_availability_test(operation_id)?
                .ok_or(ModelAvailabilityServiceError::OperationNotFound)?;
            if !current.state.is_terminal() {
                return Err(ModelAvailabilityServiceError::WorkerTerminated);
            }
        }
        Ok(Vec::new())
    }

    pub(crate) fn queue_model_availability_test(
        &self,
        prepared: &PreparedModelAvailabilityTest,
        identity: &UserConfigurationOperationIdentity,
        correlation_id: CorrelationId,
    ) -> Result<ModelAvailabilityQueueCompletion, ModelAvailabilityServiceError> {
        validate_availability_identity(prepared, identity)?;
        self.revalidate_prepared_availability_test(prepared)?;
        if let Some(existing) = self
            .persistence
            .latest_model_availability_test_for_profile(prepared.profile.id)?
            .filter(|record| !record.state.is_terminal())
        {
            return Ok(ModelAvailabilityQueueCompletion {
                accepted: aura_model_domain::ModelAvailabilityTestStartAccepted {
                    model_availability_test_id: existing.id,
                    operational_revision: existing.revision,
                    state: existing.state,
                },
                projection_changes: Vec::new(),
                newly_queued: false,
            });
        }
        let record = new_availability_record(prepared, correlation_id);
        let projection_changes = self
            .persistence
            .create_model_availability_test(
                &record,
                self.availability_profile_projection_drafts(&record)?,
            )?
            .projection_changes;
        Ok(ModelAvailabilityQueueCompletion {
            accepted: aura_model_domain::ModelAvailabilityTestStartAccepted {
                model_availability_test_id: record.id,
                operational_revision: record.revision,
                state: record.state,
            },
            projection_changes,
            newly_queued: true,
        })
    }

    pub(crate) fn prepare_model_availability_test(
        &self,
        request: ModelAvailabilityTestStartRequest,
    ) -> Result<PreparedModelAvailabilityTest, ModelAvailabilityServiceError> {
        ensure_model_config_is_stable(&self.persistence)?;
        let snapshot = self.model_config.snapshot().map_err(|_| {
            prerequisite(ModelAvailabilityPrerequisiteFailure::ModelProfileNotPersisted)
        })?;
        let models_revision = Revision::new(snapshot.revision);
        let profile =
            snapshot.registry.models.get(&request.model_profile_id).cloned().ok_or_else(|| {
                prerequisite(ModelAvailabilityPrerequisiteFailure::ModelProfileNotFound)
            })?;
        let endpoint =
            snapshot.registry.endpoints.get(&profile.endpoint_id).cloned().ok_or_else(|| {
                prerequisite(ModelAvailabilityPrerequisiteFailure::EndpointNotFound)
            })?;

        if profile.revision.value() != request.expected_model_profile_revision.value()
            || endpoint.revision.value() != request.expected_endpoint_revision.value()
        {
            return Err(prerequisite(
                ModelAvailabilityPrerequisiteFailure::ConfigurationRevisionChanged,
            ));
        }
        if !endpoint.model_order.contains(&profile.id) {
            return Err(prerequisite(
                ModelAvailabilityPrerequisiteFailure::EndpointConfigurationInvalid,
            ));
        }
        if endpoint.base_url.is_empty() {
            return Err(prerequisite(ModelAvailabilityPrerequisiteFailure::BaseUrlMissing));
        }
        if profile.provider_model_name.trim().is_empty() {
            return Err(prerequisite(ModelAvailabilityPrerequisiteFailure::ModelNameMissing));
        }

        let resolved_request_url = resolve_api_url(&endpoint.base_url, endpoint.api_format)
            .map_err(|_| {
                prerequisite(ModelAvailabilityPrerequisiteFailure::EndpointConfigurationInvalid)
            })?
            .to_string();
        let endpoint_security_fingerprint = endpoint_fingerprint(&endpoint).map_err(|_| {
            prerequisite(ModelAvailabilityPrerequisiteFailure::EndpointConfigurationInvalid)
        })?;
        let credential = self.configuration_credential_preflight(&endpoint).map_err(|_| {
            prerequisite(ModelAvailabilityPrerequisiteFailure::CredentialNotConfigured)
        })?;
        let configured_output_token_field = configured_output_token_field(&endpoint, &profile)?;
        let applied_output_token_field_alias = configured_output_token_field_alias(
            &endpoint,
            &profile,
            configured_output_token_field,
        )?;

        let id = ModelAvailabilityTestId::new();
        let operation_context = TransportOperationContext::ModelAvailabilityTest {
            model_availability_test_id: id,
            model_profile_id: profile.id,
        };
        let input = ModelAvailabilityProbeCompilationInput {
            endpoint: &endpoint,
            model_profile: &profile,
            resolved_request_url: &resolved_request_url,
            operation_context: &operation_context,
            applied_output_token_field_alias: applied_output_token_field_alias.as_ref(),
        };
        let request_plan = compile_probe(endpoint.api_format, input).map_err(|_| {
            prerequisite(ModelAvailabilityPrerequisiteFailure::RequestContractUnresolved)
        })?;

        Ok(PreparedModelAvailabilityTest {
            id,
            models_revision,
            endpoint,
            profile,
            endpoint_security_fingerprint,
            credential_status: credential.credential_status,
            credential_binding: credential.credential_binding,
            operation_context,
            request_plan,
            applied_output_token_field_alias,
            configured_output_token_field,
            resolved_request_url,
        })
    }

    pub(crate) async fn execute_model_availability_test(
        &self,
        prepared: PreparedModelAvailabilityTest,
        identity: UserConfigurationOperationIdentity,
        correlation_id: CorrelationId,
        control: P05OperationControl,
    ) -> Result<ModelAvailabilityCommandCompletion, ModelAvailabilityServiceError> {
        validate_availability_identity(&prepared, &identity)?;
        self.revalidate_prepared_availability_test(&prepared)?;
        let cancellation = control.cancellation();

        let mut record = self
            .persistence
            .model_availability_test(prepared.id)?
            .ok_or(ModelAvailabilityServiceError::OperationNotFound)?;
        let started_at = record.started_at;
        let mut projection_changes = Vec::new();

        if cancellation.is_cancelled() {
            return self.finish_availability_cancelled(
                record,
                projection_changes,
                control
                    .cancellation_reason()
                    .unwrap_or(aura_model_domain::ModelOperationCancellationReason::UserRequested),
            );
        }

        let services = self.clone();
        let operation_id = prepared.id;
        let first_outcome = self
            .configuration_transport
            .execute(ConfigurationTransportExecuteRequest {
                identity: identity.clone(),
                correlation_id,
                models_revision: prepared.models_revision,
                endpoint_id: prepared.endpoint.id,
                endpoint_security_fingerprint: prepared.endpoint_security_fingerprint.clone(),
                purpose: ConfigurationRequestPurpose::ModelAvailabilityProbe {
                    model_availability_test_id: prepared.id,
                    model_profile_id: prepared.profile.id,
                },
                operation_context: prepared.operation_context.clone(),
                request_plan: prepared.request_plan.clone(),
                credential_binding: prepared.credential_binding.clone(),
                attempt_number: 1,
                attempt_kind: TransportAttemptKind::Initial,
                timeout_ms: prepared
                    .endpoint
                    .timeouts
                    .total_ms
                    .unwrap_or(prepared.endpoint.timeouts.first_event_ms),
                cancellation: cancellation.clone(),
                persist_and_bind_attempt: Arc::new(move |attempt| {
                    services
                        .bind_availability_transport_attempt(operation_id, attempt)
                        .map_err(|error| error.to_string())
                }),
            })
            .await?;

        record = self
            .persistence
            .model_availability_test(prepared.id)?
            .ok_or(ModelAvailabilityServiceError::OperationNotFound)?;

        if let ConfigurationTransportOutcome::Cancelled(cancelled) = &first_outcome {
            debug_assert!(record.direct_transport_attempt_ids.contains(&cancelled.attempt.id));
            return self.finish_availability_cancelled_with_attempt(
                record,
                projection_changes,
                control
                    .cancellation_reason()
                    .unwrap_or(aura_model_domain::ModelOperationCancellationReason::UserRequested),
                cancelled.attempt.clone(),
            );
        }

        let mut final_outcome = first_outcome;
        let mut applied_output_token_field_alias =
            prepared.applied_output_token_field_alias.clone();
        let mut warnings = Vec::new();

        if let Some(alias) = retry_alias_for_explicit_field_rejection(&prepared, &final_outcome) {
            if cancellation.is_cancelled() {
                return self.finish_availability_cancelled_with_attempt(
                    record,
                    projection_changes,
                    control.cancellation_reason().unwrap_or(
                        aura_model_domain::ModelOperationCancellationReason::UserRequested,
                    ),
                    configuration_outcome_attempt(&final_outcome).clone(),
                );
            }
            let expected_revision = record.revision;
            record.revision = next_revision(expected_revision)?;
            record.stage = Some(ModelAvailabilityTestStage::ModelGeneration);
            let first_attempt = configuration_outcome_attempt(&final_outcome).clone();
            let first_commit =
                self.persistence.transition_model_availability_test_with_transport_attempt(
                    ModelAvailabilityTransportTransition {
                        correlation_id,
                        expected_revision,
                        next: &record,
                        cancelled_by: None,
                        attempt: &first_attempt,
                        attempt_cancelled_by: None,
                        projection_drafts: self.availability_profile_projection_drafts(&record)?,
                    },
                )?;
            projection_changes.extend(first_commit.projection_changes);
            let retry_plan = compile_probe(
                prepared.endpoint.api_format,
                ModelAvailabilityProbeCompilationInput {
                    endpoint: &prepared.endpoint,
                    model_profile: &prepared.profile,
                    resolved_request_url: &prepared.resolved_request_url,
                    operation_context: &prepared.operation_context,
                    applied_output_token_field_alias: Some(&alias),
                },
            )?;
            let services = self.clone();
            let operation_id = prepared.id;
            let retry_outcome = self
                .configuration_transport
                .execute(ConfigurationTransportExecuteRequest {
                    identity: identity.clone(),
                    correlation_id,
                    models_revision: prepared.models_revision,
                    endpoint_id: prepared.endpoint.id,
                    endpoint_security_fingerprint: prepared.endpoint_security_fingerprint.clone(),
                    purpose: ConfigurationRequestPurpose::ModelAvailabilityProbe {
                        model_availability_test_id: prepared.id,
                        model_profile_id: prepared.profile.id,
                    },
                    operation_context: prepared.operation_context.clone(),
                    request_plan: retry_plan,
                    credential_binding: prepared.credential_binding.clone(),
                    attempt_number: 2,
                    attempt_kind: TransportAttemptKind::CompatibilityAdjustedRetry,
                    timeout_ms: prepared
                        .endpoint
                        .timeouts
                        .total_ms
                        .unwrap_or(prepared.endpoint.timeouts.first_event_ms),
                    cancellation: cancellation.clone(),
                    persist_and_bind_attempt: Arc::new(move |attempt| {
                        services
                            .bind_availability_transport_attempt(operation_id, attempt)
                            .map_err(|error| error.to_string())
                    }),
                })
                .await?;
            record = self
                .persistence
                .model_availability_test(prepared.id)?
                .ok_or(ModelAvailabilityServiceError::OperationNotFound)?;
            if let ConfigurationTransportOutcome::Cancelled(cancelled) = &retry_outcome {
                debug_assert!(record.direct_transport_attempt_ids.contains(&cancelled.attempt.id));
                return self.finish_availability_cancelled_with_attempt(
                    record,
                    projection_changes,
                    control.cancellation_reason().unwrap_or(
                        aura_model_domain::ModelOperationCancellationReason::UserRequested,
                    ),
                    cancelled.attempt.clone(),
                );
            }
            if availability_outcome_succeeded(&prepared, started_at, &retry_outcome) {
                match self.persist_learned_output_token_alias(
                    &prepared,
                    identity.command_invocation_id(),
                    correlation_id,
                    &alias,
                )? {
                    AliasPersistenceOutcome::Persisted(persisted)
                    | AliasPersistenceOutcome::AlreadyPresent(persisted) => {
                        applied_output_token_field_alias = Some(persisted);
                    }
                    AliasPersistenceOutcome::NotPersistedBeforeForwardPoint => {
                        applied_output_token_field_alias = Some(alias);
                        warnings.push(issue_code(
                            "compatibility.output_token_field_alias_not_persisted",
                        ));
                    }
                }
            }
            final_outcome = retry_outcome;
        }

        let alias_conflict = existing_alias_was_explicitly_rejected(&prepared, &final_outcome);
        let (raw_response_artifact_id, mut result, terminal_attempt) = match final_outcome {
            ConfigurationTransportOutcome::Response(response) => {
                let result = availability_result_for_response(
                    &prepared.endpoint,
                    &prepared.profile,
                    started_at,
                    &response,
                );
                (response.raw_response_artifact_id, result, response.attempt)
            }
            ConfigurationTransportOutcome::Failed(failure) => {
                let result = availability_result_for_transport_failure(
                    &prepared.endpoint,
                    &prepared.profile,
                    started_at,
                    &failure,
                );
                (failure.attempt.raw_stream_artifact_id, result, failure.attempt)
            }
            ConfigurationTransportOutcome::Cancelled(_) => {
                unreachable!("cancelled availability outcomes return before result mapping")
            }
        };
        if alias_conflict {
            result.model_generation = failed_check(
                "compatibility.output_token_field_conflict",
                provider_status_from_result(&result),
            );
            result.outcome = ModelAvailabilityTestOutcome::Unavailable;
        }
        result.warnings.extend(warnings);
        result
            .warnings
            .sort_by(|left, right| left.as_str().as_bytes().cmp(right.as_str().as_bytes()));
        result.warnings.dedup();

        let completed_at = result.completed_at;
        let expected_revision = record.revision;
        record.revision = next_revision(expected_revision)?;
        record.state = ModelAvailabilityTestState::Completed;
        record.stage = Some(ModelAvailabilityTestStage::Completed);
        record.applied_output_token_field_alias = applied_output_token_field_alias;
        record.result = Some(result);
        record.raw_response_artifact_id = raw_response_artifact_id;
        record.completed_at = Some(completed_at);
        let terminal = self.persistence.transition_model_availability_test_with_transport_attempt(
            ModelAvailabilityTransportTransition {
                correlation_id,
                expected_revision,
                next: &record,
                cancelled_by: None,
                attempt: &terminal_attempt,
                attempt_cancelled_by: None,
                projection_drafts: self.availability_profile_projection_drafts(&record)?,
            },
        )?;
        projection_changes.extend(terminal.projection_changes);
        Ok(ModelAvailabilityCommandCompletion { projection_changes })
    }

    fn bind_availability_transport_attempt(
        &self,
        model_availability_test_id: ModelAvailabilityTestId,
        transport_attempt: &TransportAttemptRecord,
    ) -> Result<(), ModelAvailabilityServiceError> {
        let mut record = self
            .persistence
            .model_availability_test(model_availability_test_id)?
            .ok_or(ModelAvailabilityServiceError::OperationNotFound)?;
        if record.state.is_terminal() {
            return Err(ModelAvailabilityServiceError::Persistence(
                PersistenceError::ModelAvailabilityTestStateConflict,
            ));
        }
        let expected = record.revision;
        record.revision = next_revision(expected)?;
        record.state = ModelAvailabilityTestState::Running;
        record.stage = Some(record.stage.unwrap_or(ModelAvailabilityTestStage::Network));
        record.direct_transport_attempt_ids.push(transport_attempt.id);
        let commit = self.persistence.start_model_availability_transport_attempt(
            record.correlation_id,
            expected,
            &record,
            transport_attempt,
            self.availability_profile_projection_drafts(&record)?,
        )?;
        self.persistence.publish_model_projection_changes(&commit.projection_changes)?;
        Ok(())
    }

    fn finish_availability_cancelled(
        &self,
        record: ModelAvailabilityTestRecord,
        projection_changes: Vec<ProjectionChange>,
        reason: aura_model_domain::ModelOperationCancellationReason,
    ) -> Result<ModelAvailabilityCommandCompletion, ModelAvailabilityServiceError> {
        if let Some(current) = self.persistence.model_availability_test(record.id)? {
            if current.state.is_terminal() {
                return Ok(ModelAvailabilityCommandCompletion { projection_changes });
            }
            return self.commit_availability_cancelled(current, projection_changes, reason);
        }
        self.commit_availability_cancelled(record, projection_changes, reason)
    }

    fn finish_availability_cancelled_with_attempt(
        &self,
        record: ModelAvailabilityTestRecord,
        projection_changes: Vec<ProjectionChange>,
        reason: aura_model_domain::ModelOperationCancellationReason,
        attempt: TransportAttemptRecord,
    ) -> Result<ModelAvailabilityCommandCompletion, ModelAvailabilityServiceError> {
        if let Some(current) = self.persistence.model_availability_test(record.id)? {
            if current.state.is_terminal() {
                return Ok(ModelAvailabilityCommandCompletion { projection_changes });
            }
            return self.commit_availability_cancelled_with_attempt(
                current,
                projection_changes,
                reason,
                attempt,
            );
        }
        self.commit_availability_cancelled_with_attempt(record, projection_changes, reason, attempt)
    }

    fn commit_availability_cancelled_with_attempt(
        &self,
        mut record: ModelAvailabilityTestRecord,
        mut projection_changes: Vec<ProjectionChange>,
        reason: aura_model_domain::ModelOperationCancellationReason,
        attempt: TransportAttemptRecord,
    ) -> Result<ModelAvailabilityCommandCompletion, ModelAvailabilityServiceError> {
        let expected = record.revision;
        record.revision = next_revision(expected)?;
        record.state = ModelAvailabilityTestState::Cancelled;
        record.cancellation_reason = Some(reason);
        record.result = None;
        record.completed_at = Some(UtcTimestamp::now());
        let attempt_cancelled_by = (attempt.state
            == aura_model_domain::TransportAttemptState::Cancelled)
            .then(|| transport_cancellation_source(reason));
        let terminal = self.persistence.transition_model_availability_test_with_transport_attempt(
            ModelAvailabilityTransportTransition {
                correlation_id: record.correlation_id,
                expected_revision: expected,
                next: &record,
                cancelled_by: Some(reason),
                attempt: &attempt,
                attempt_cancelled_by,
                projection_drafts: self.availability_profile_projection_drafts(&record)?,
            },
        )?;
        projection_changes.extend(terminal.projection_changes);
        Ok(ModelAvailabilityCommandCompletion { projection_changes })
    }

    fn commit_availability_cancelled(
        &self,
        mut record: ModelAvailabilityTestRecord,
        mut projection_changes: Vec<ProjectionChange>,
        reason: aura_model_domain::ModelOperationCancellationReason,
    ) -> Result<ModelAvailabilityCommandCompletion, ModelAvailabilityServiceError> {
        let expected = record.revision;
        record.revision = next_revision(expected)?;
        record.state = ModelAvailabilityTestState::Cancelled;
        record.cancellation_reason = Some(reason);
        record.result = None;
        record.completed_at = Some(UtcTimestamp::now());
        let terminal = self.persistence.transition_model_availability_test(
            expected,
            &record,
            Some(reason),
            self.availability_profile_projection_drafts(&record)?,
        )?;
        projection_changes.extend(terminal.projection_changes);
        Ok(ModelAvailabilityCommandCompletion { projection_changes })
    }

    pub(crate) fn terminalize_model_availability_error(
        &self,
        model_availability_test_id: ModelAvailabilityTestId,
        error: &ModelAvailabilityServiceError,
        requested_cancellation: Option<aura_model_domain::ModelOperationCancellationReason>,
    ) -> Result<ModelAvailabilityCommandCompletion, ModelAvailabilityServiceError> {
        let Some(mut record) =
            self.persistence.model_availability_test(model_availability_test_id)?
        else {
            return Ok(ModelAvailabilityCommandCompletion { projection_changes: Vec::new() });
        };
        if record.state.is_terminal() {
            return Ok(ModelAvailabilityCommandCompletion { projection_changes: Vec::new() });
        }

        if let Some(reason) = requested_cancellation
            .or_else(|| self.model_availability_error_cancellation_reason(&record, error))
        {
            return self.finish_availability_cancelled(record, Vec::new(), reason);
        }

        let completed_at = UtcTimestamp::now();
        let blocked = blocked_check();
        let issue = match error {
            ModelAvailabilityServiceError::Prerequisite(
                ModelAvailabilityPrerequisiteFailure::CredentialNotConfigured,
            ) => "credential.unavailable",
            ModelAvailabilityServiceError::Persistence(_)
            | ModelAvailabilityServiceError::ConfigJournal(_)
            | ModelAvailabilityServiceError::Transport(
                ConfigurationTransportError::Persistence(_)
                | ConfigurationTransportError::OperationBinding(_),
            ) => "persistence.operation_failed",
            ModelAvailabilityServiceError::Transport(_) => "configuration.invalid",
            ModelAvailabilityServiceError::Adapter(_) => "provider.invalid_response",
            ModelAvailabilityServiceError::OutputTokenFieldAliasConflict => {
                "compatibility.output_token_field_conflict"
            }
            ModelAvailabilityServiceError::Prerequisite(_)
            | ModelAvailabilityServiceError::RevisionOverflow
            | ModelAvailabilityServiceError::MissingProjection
            | ModelAvailabilityServiceError::OperationNotFound
            | ModelAvailabilityServiceError::WorkerTerminated
            | ModelAvailabilityServiceError::OperationRevisionConflict { .. } => {
                "internal.unexpected"
            }
        };
        let availability_result = result(
            &record.endpoint_snapshot,
            &record.model_profile_snapshot,
            record.started_at,
            completed_at,
            AvailabilityChecks {
                config: failed_check(issue, None),
                network: blocked.clone(),
                tls: blocked.clone(),
                http: blocked.clone(),
                authentication: blocked.clone(),
                model_generation: blocked,
            },
            Vec::new(),
        );
        let expected = record.revision;
        record.revision = next_revision(expected)?;
        record.state = ModelAvailabilityTestState::Completed;
        record.stage = Some(ModelAvailabilityTestStage::Completed);
        record.result = Some(availability_result);
        record.completed_at = Some(completed_at);
        let terminal = self.persistence.transition_model_availability_test(
            expected,
            &record,
            None,
            self.availability_profile_projection_drafts(&record)?,
        )?;
        Ok(ModelAvailabilityCommandCompletion { projection_changes: terminal.projection_changes })
    }

    fn model_availability_error_cancellation_reason(
        &self,
        record: &ModelAvailabilityTestRecord,
        error: &ModelAvailabilityServiceError,
    ) -> Option<aura_model_domain::ModelOperationCancellationReason> {
        let snapshot = self.model_config.snapshot().ok()?;
        let Some(endpoint) = snapshot.registry.endpoints.get(&record.endpoint_id) else {
            return Some(aura_model_domain::ModelOperationCancellationReason::TargetDeleted);
        };
        let Some(profile) = snapshot.registry.models.get(&record.model_profile_id) else {
            return Some(aura_model_domain::ModelOperationCancellationReason::TargetDeleted);
        };
        if snapshot.revision != record.models_revision_at_start.value()
            || endpoint.revision != record.endpoint_revision_at_start
            || profile.revision != record.model_profile_revision_at_start
            || endpoint_fingerprint(endpoint).ok().as_ref()
                != Some(&record.endpoint_security_fingerprint)
            || matches!(
                error,
                ModelAvailabilityServiceError::Prerequisite(
                    ModelAvailabilityPrerequisiteFailure::ConfigurationRevisionChanged
                        | ModelAvailabilityPrerequisiteFailure::ModelConfigMutationInProgress
                )
            )
        {
            return Some(aura_model_domain::ModelOperationCancellationReason::ConfigurationChanged);
        }
        None
    }

    pub(crate) async fn cancel_model_availability_test(
        &self,
        request: aura_model_domain::ModelAvailabilityTestCancelRequest,
    ) -> Result<
        (aura_model_domain::ModelAvailabilityTestCancelResult, Vec<ProjectionChange>),
        ModelAvailabilityServiceError,
    > {
        let record = self
            .persistence
            .model_availability_test(request.model_availability_test_id)?
            .ok_or(ModelAvailabilityServiceError::OperationNotFound)?;
        if record.revision.value() != request.expected_operational_revision.value() {
            return Err(ModelAvailabilityServiceError::OperationRevisionConflict {
                expected: request.expected_operational_revision.value(),
                actual: record.revision.value(),
            });
        }
        if record.state.is_terminal() {
            if let Some(control) = self.operation_cancellations.availability(record.id) {
                control.wait_for_terminal_projection().await;
            }
            return Ok((
                aura_model_domain::ModelAvailabilityTestCancelResult {
                    model_availability_test_id: record.id,
                    operational_revision: record.revision,
                    state: record.state,
                },
                Vec::new(),
            ));
        }
        let Some(control) = self.operation_cancellations.availability(record.id) else {
            let current = self
                .persistence
                .model_availability_test(record.id)?
                .ok_or(ModelAvailabilityServiceError::OperationNotFound)?;
            if current.state.is_terminal() {
                return Ok((
                    aura_model_domain::ModelAvailabilityTestCancelResult {
                        model_availability_test_id: current.id,
                        operational_revision: current.revision,
                        state: current.state,
                    },
                    Vec::new(),
                ));
            }
            return Err(ModelAvailabilityServiceError::WorkerTerminated);
        };
        control.cancel(aura_model_domain::ModelOperationCancellationReason::UserRequested);

        if record.state == ModelAvailabilityTestState::Queued {
            // This is a strict Queued CAS. If the Worker attached an Attempt
            // and advanced the Operation meanwhile, it owns terminalization so
            // that the Attempt cancellation is persisted before the Operation.
            match self.commit_availability_cancelled(
                record,
                Vec::new(),
                aura_model_domain::ModelOperationCancellationReason::UserRequested,
            ) {
                Ok(completion) => {
                    self.persistence
                        .publish_model_projection_changes(&completion.projection_changes)?;
                    control.mark_terminal_projection_published();
                }
                Err(ModelAvailabilityServiceError::Persistence(
                    PersistenceError::ModelAvailabilityTestStateConflict,
                )) => {}
                Err(error) => return Err(error),
            }
        }

        control.wait_for_terminal_projection().await;
        let record = self
            .persistence
            .model_availability_test(request.model_availability_test_id)?
            .ok_or(ModelAvailabilityServiceError::OperationNotFound)?;
        if !record.state.is_terminal() {
            return Err(ModelAvailabilityServiceError::WorkerTerminated);
        }
        Ok((
            aura_model_domain::ModelAvailabilityTestCancelResult {
                model_availability_test_id: record.id,
                operational_revision: record.revision,
                state: record.state,
            },
            Vec::new(),
        ))
    }

    fn persist_learned_output_token_alias(
        &self,
        prepared: &PreparedModelAvailabilityTest,
        command_invocation_id: CommandInvocationId,
        correlation_id: CorrelationId,
        alias: &ChatOutputTokenFieldAlias,
    ) -> Result<AliasPersistenceOutcome, ModelAvailabilityServiceError> {
        let completion =
            self.config_journal.execute(command_invocation_id, correlation_id, |journal_id| {
                let mutation = self.model_config.prepare_update(
                    journal_id.to_string(),
                    prepared.models_revision.value(),
                    |registry| {
                        learn_chat_output_token_field_alias(
                            registry,
                            prepared.endpoint.id,
                            &prepared.profile.provider_model_name,
                            alias.configured_field,
                            alias.effective_field,
                            alias.learned_from_transport_attempt_id,
                            alias.learned_at,
                        )
                    },
                )?;
                let persisted_alias = match &mutation.value {
                    LearnChatOutputTokenFieldAliasResult::Learned(alias)
                    | LearnChatOutputTokenFieldAliasResult::AlreadyPresent(alias) => alias.clone(),
                };
                let commit = AliasPersistenceCommit {
                    learned: matches!(
                        mutation.value,
                        LearnChatOutputTokenFieldAliasResult::Learned(_)
                    ),
                    alias: persisted_alias,
                };
                if !mutation.changed {
                    return Ok(ModelConfigCommandPlan::NoChange {
                        success_result: serde_json::to_value(commit)?,
                    });
                }
                if !commit.learned {
                    return Err(ModelConfigError::InternalInvariant(
                        "changed output-token alias mutation was not a new learning",
                    )
                    .into());
                }

                let snapshot = prepared_snapshot(&mutation);
                let previous_endpoint = mutation
                    .previous
                    .registry
                    .endpoints
                    .get(&prepared.endpoint.id)
                    .ok_or(ModelConfigError::EndpointIdNotFound(prepared.endpoint.id))?;
                let endpoint = mutation
                    .candidate_registry
                    .endpoints
                    .get(&prepared.endpoint.id)
                    .ok_or(ModelConfigError::EndpointIdNotFound(prepared.endpoint.id))?;
                let projection_status = BTreeMap::new();
                let projection_drafts = vec![
                    ModelProjectionDraft::ModelSettings(
                        self.projections.model_settings(&snapshot, &projection_status)?,
                    ),
                    ModelProjectionDraft::EndpointDetail(Box::new(
                        self.projections.endpoint_detail(
                            &snapshot,
                            endpoint.id,
                            &projection_status,
                        )?,
                    )),
                    ModelProjectionDraft::ModelProfile(Box::new(self.projections.model_profile(
                        &snapshot,
                        prepared.profile.id,
                        &projection_status,
                    )?)),
                ];
                let prepared_bundle =
                    mutation.prepared_bundle.ok_or(ModelConfigError::InternalInvariant(
                        "changed output-token alias mutation lacks a prepared model bundle",
                    ))?;
                Ok(ModelConfigCommandPlan::Publish {
                    prepared_bundle: Box::new(prepared_bundle),
                    expected_models_revision: prepared.models_revision,
                    reserved_models_revision: Some(mutation.candidate_revision),
                    expected_aura_revision: None,
                    reserved_aura_revision: None,
                    credential_slot_creations: Vec::new(),
                    session_binding_clears: Vec::new(),
                    metadata_events: vec![user_event(
                        "ModelChatOutputTokenFieldAliasLearned",
                        correlation_id,
                        None,
                        &ModelChatOutputTokenFieldAliasLearnedPayload {
                            endpoint_id: endpoint.id,
                            provider_model_name: commit.alias.provider_model_name.clone(),
                            configured_field: commit.alias.configured_field,
                            effective_field: commit.alias.effective_field,
                            learned_from_transport_attempt_id: commit
                                .alias
                                .learned_from_transport_attempt_id,
                            previous_endpoint_revision: previous_endpoint.revision,
                            endpoint_revision: endpoint.revision,
                            previous_models_revision: prepared.models_revision,
                            models_revision: mutation.candidate_revision,
                        },
                    )?],
                    projection_drafts,
                    success_result: serde_json::to_value(commit)?,
                })
            });

        match completion {
            Ok(completion) => {
                let commit: AliasPersistenceCommit =
                    serde_json::from_value(completion.success_result).map_err(|error| {
                        ModelAvailabilityServiceError::ConfigJournal(
                            ModelConfigJournalError::InvalidPayload(error),
                        )
                    })?;
                if commit.learned {
                    Ok(AliasPersistenceOutcome::Persisted(commit.alias))
                } else {
                    Ok(AliasPersistenceOutcome::AlreadyPresent(commit.alias))
                }
            }
            Err(ModelConfigJournalError::ModelConfig(
                ModelConfigError::OutputTokenFieldAliasConflict { .. },
            )) => Err(ModelAvailabilityServiceError::OutputTokenFieldAliasConflict),
            Err(error) => {
                let journal = self
                    .persistence
                    .model_config_mutation_journal_by_command(command_invocation_id)?;
                if journal
                    .is_none_or(|journal| journal.state == ModelConfigMutationJournalState::Aborted)
                {
                    self.resolve_concurrent_alias_state(prepared, alias)
                } else {
                    Err(ModelAvailabilityServiceError::ConfigJournal(error))
                }
            }
        }
    }

    fn resolve_concurrent_alias_state(
        &self,
        prepared: &PreparedModelAvailabilityTest,
        attempted: &ChatOutputTokenFieldAlias,
    ) -> Result<AliasPersistenceOutcome, ModelAvailabilityServiceError> {
        let Ok(snapshot) = self.model_config.snapshot() else {
            return Ok(AliasPersistenceOutcome::NotPersistedBeforeForwardPoint);
        };
        let existing = snapshot
            .registry
            .endpoints
            .get(&prepared.endpoint.id)
            .and_then(|endpoint| endpoint.compatibility.as_ref())
            .and_then(|compatibility| {
                compatibility
                    .output_token_field_aliases
                    .iter()
                    .find(|alias| alias.provider_model_name == attempted.provider_model_name)
            })
            .cloned();
        match existing {
            Some(existing)
                if existing.configured_field == attempted.configured_field
                    && existing.effective_field == attempted.effective_field =>
            {
                Ok(AliasPersistenceOutcome::AlreadyPresent(existing))
            }
            Some(_) => Err(ModelAvailabilityServiceError::OutputTokenFieldAliasConflict),
            None => Ok(AliasPersistenceOutcome::NotPersistedBeforeForwardPoint),
        }
    }

    fn availability_profile_projection_drafts(
        &self,
        record: &ModelAvailabilityTestRecord,
    ) -> Result<Vec<ModelProjectionDraft>, ModelAvailabilityServiceError> {
        let snapshot = self.model_config.snapshot().map_err(|_| {
            prerequisite(ModelAvailabilityPrerequisiteFailure::ConfigurationRevisionChanged)
        })?;
        if !snapshot.registry.models.contains_key(&record.model_profile_id) {
            return Ok(Vec::new());
        }
        let model_profile = self
            .projections
            .model_profile_with_latest_availability(
                &snapshot,
                record.model_profile_id,
                &std::collections::BTreeMap::new(),
                Some(aura_model_domain::ModelAvailabilityTestView::from_record(record)),
            )
            .map_err(|_| ModelAvailabilityServiceError::MissingProjection)?;
        Ok(vec![ModelProjectionDraft::ModelProfile(Box::new(model_profile))])
    }

    fn revalidate_prepared_availability_test(
        &self,
        prepared: &PreparedModelAvailabilityTest,
    ) -> Result<(), ModelAvailabilityServiceError> {
        ensure_model_config_is_stable(&self.persistence)?;
        let snapshot = self.model_config.snapshot().map_err(|_| {
            prerequisite(ModelAvailabilityPrerequisiteFailure::ConfigurationRevisionChanged)
        })?;
        let endpoint = snapshot.registry.endpoints.get(&prepared.endpoint.id);
        let profile = snapshot.registry.models.get(&prepared.profile.id);
        if snapshot.revision != prepared.models_revision.value()
            || endpoint != Some(&prepared.endpoint)
            || profile != Some(&prepared.profile)
        {
            return Err(prerequisite(
                ModelAvailabilityPrerequisiteFailure::ConfigurationRevisionChanged,
            ));
        }
        Ok(())
    }
}

fn ensure_model_config_is_stable(
    persistence: &Arc<PersistenceCoordinator>,
) -> Result<(), ModelAvailabilityServiceError> {
    if persistence
        .active_mutation_journals()?
        .iter()
        .any(|journal| journal.journal_kind == MODEL_CONFIG_MUTATION_JOURNAL_KIND)
    {
        return Err(prerequisite(
            ModelAvailabilityPrerequisiteFailure::ModelConfigMutationInProgress,
        ));
    }
    Ok(())
}

fn configured_output_token_field(
    endpoint: &ModelEndpointConfig,
    profile: &ModelProfileConfig,
) -> Result<Option<ChatOutputTokenField>, ModelAvailabilityServiceError> {
    if endpoint.api_format != ModelApiFormat::OpenAiChatCompletions {
        return Ok(None);
    }
    profile
        .request_defaults
        .chat_output_token_field
        .or(endpoint.chat_output_token_field)
        .map(Some)
        .ok_or_else(|| {
            prerequisite(ModelAvailabilityPrerequisiteFailure::RequestContractUnresolved)
        })
}

fn configured_output_token_field_alias(
    endpoint: &ModelEndpointConfig,
    profile: &ModelProfileConfig,
    configured: Option<ChatOutputTokenField>,
) -> Result<Option<ChatOutputTokenFieldAlias>, ModelAvailabilityServiceError> {
    if endpoint.api_format != ModelApiFormat::OpenAiChatCompletions {
        return Ok(None);
    }
    let configured = configured.ok_or_else(|| {
        prerequisite(ModelAvailabilityPrerequisiteFailure::RequestContractUnresolved)
    })?;
    let Some(compatibility) = &endpoint.compatibility else {
        return Ok(None);
    };
    let alias = compatibility
        .output_token_field_aliases
        .iter()
        .find(|alias| alias.provider_model_name == profile.provider_model_name)
        .cloned();
    match alias {
        Some(alias)
            if alias.configured_field == configured && alias.effective_field != configured =>
        {
            Ok(Some(alias))
        }
        Some(_) => {
            Err(prerequisite(ModelAvailabilityPrerequisiteFailure::RequestContractUnresolved))
        }
        None => Ok(None),
    }
}

fn retry_alias_for_explicit_field_rejection(
    prepared: &PreparedModelAvailabilityTest,
    outcome: &ConfigurationTransportOutcome,
) -> Option<ChatOutputTokenFieldAlias> {
    if prepared.endpoint.api_format != ModelApiFormat::OpenAiChatCompletions
        || prepared.applied_output_token_field_alias.is_some()
    {
        return None;
    }
    let configured_field = prepared.configured_output_token_field?;
    let ConfigurationTransportOutcome::Response(response) = outcome else {
        return None;
    };
    if (200..300).contains(&response.status)
        || !response_explicitly_rejects_field(&response.body, configured_field)
    {
        return None;
    }
    Some(ChatOutputTokenFieldAlias {
        provider_model_name: prepared.profile.provider_model_name.clone(),
        configured_field,
        effective_field: alternate_output_token_field(configured_field),
        learned_at: UtcTimestamp::now(),
        learned_from_transport_attempt_id: response.attempt.id,
    })
}

fn existing_alias_was_explicitly_rejected(
    prepared: &PreparedModelAvailabilityTest,
    outcome: &ConfigurationTransportOutcome,
) -> bool {
    let Some(alias) = &prepared.applied_output_token_field_alias else {
        return false;
    };
    let ConfigurationTransportOutcome::Response(response) = outcome else {
        return false;
    };
    !(200..300).contains(&response.status)
        && response_explicitly_rejects_field(&response.body, alias.effective_field)
}

fn configuration_outcome_attempt(
    outcome: &ConfigurationTransportOutcome,
) -> &TransportAttemptRecord {
    match outcome {
        ConfigurationTransportOutcome::Response(response) => &response.attempt,
        ConfigurationTransportOutcome::Failed(failure) => &failure.attempt,
        ConfigurationTransportOutcome::Cancelled(cancelled) => &cancelled.attempt,
    }
}

const fn transport_cancellation_source(
    reason: aura_model_domain::ModelOperationCancellationReason,
) -> CancellationSource {
    match reason {
        aura_model_domain::ModelOperationCancellationReason::UserRequested => {
            CancellationSource::UpstreamCancellationToken
        }
        aura_model_domain::ModelOperationCancellationReason::ApplicationRestarted => {
            CancellationSource::RecoveryDecision
        }
        aura_model_domain::ModelOperationCancellationReason::ConfigurationChanged
        | aura_model_domain::ModelOperationCancellationReason::TargetDeleted => {
            CancellationSource::EndpointConfigurationRevoked
        }
    }
}

fn response_explicitly_rejects_field(body: &[u8], field: ChatOutputTokenField) -> bool {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return false;
    };
    value
        .get("error")
        .and_then(serde_json::Value::as_object)
        .and_then(|error| error.get("param"))
        .and_then(serde_json::Value::as_str)
        == Some(output_token_field_wire_name(field))
}

const fn alternate_output_token_field(field: ChatOutputTokenField) -> ChatOutputTokenField {
    match field {
        ChatOutputTokenField::MaxCompletionTokens => ChatOutputTokenField::MaxTokens,
        ChatOutputTokenField::MaxTokens => ChatOutputTokenField::MaxCompletionTokens,
    }
}

const fn output_token_field_wire_name(field: ChatOutputTokenField) -> &'static str {
    match field {
        ChatOutputTokenField::MaxCompletionTokens => "max_completion_tokens",
        ChatOutputTokenField::MaxTokens => "max_tokens",
    }
}

fn availability_outcome_succeeded(
    prepared: &PreparedModelAvailabilityTest,
    started_at: UtcTimestamp,
    outcome: &ConfigurationTransportOutcome,
) -> bool {
    match outcome {
        ConfigurationTransportOutcome::Response(response) => {
            availability_result_for_response(
                &prepared.endpoint,
                &prepared.profile,
                started_at,
                response,
            )
            .outcome
                == ModelAvailabilityTestOutcome::Available
        }
        ConfigurationTransportOutcome::Failed(_) => false,
        ConfigurationTransportOutcome::Cancelled(_) => false,
    }
}

fn validate_availability_identity(
    prepared: &PreparedModelAvailabilityTest,
    identity: &UserConfigurationOperationIdentity,
) -> Result<(), ModelAvailabilityServiceError> {
    if identity.operation_kind() != ConfigurationOperationKind::ModelAvailabilityTest
        || identity.target_endpoint_id() != prepared.endpoint.id
        || identity.config_revision() != prepared.models_revision
    {
        return Err(prerequisite(
            ModelAvailabilityPrerequisiteFailure::ConfigurationRevisionChanged,
        ));
    }
    Ok(())
}

fn new_availability_record(
    prepared: &PreparedModelAvailabilityTest,
    correlation_id: CorrelationId,
) -> ModelAvailabilityTestRecord {
    ModelAvailabilityTestRecord {
        id: prepared.id,
        correlation_id,
        endpoint_id: prepared.endpoint.id,
        model_profile_id: prepared.profile.id,
        models_revision_at_start: prepared.models_revision,
        endpoint_revision_at_start: prepared.endpoint.revision,
        model_profile_revision_at_start: prepared.profile.revision,
        endpoint_security_fingerprint: prepared.endpoint_security_fingerprint.clone(),
        endpoint_snapshot: prepared.endpoint.clone(),
        model_profile_snapshot: prepared.profile.clone(),
        credential_status_at_start: prepared.credential_status.clone(),
        credential_binding_fingerprint: prepared
            .credential_binding
            .as_ref()
            .map(|binding| binding.credential_binding_fingerprint.clone()),
        revision: Revision::INITIAL,
        state: ModelAvailabilityTestState::Queued,
        stage: None,
        cancellation_reason: None,
        direct_transport_attempt_ids: Vec::new(),
        applied_output_token_field_alias: prepared.applied_output_token_field_alias.clone(),
        result: None,
        raw_response_artifact_id: None,
        started_at: UtcTimestamp::now(),
        completed_at: None,
    }
}

fn provider_status_from_result(result: &ModelAvailabilityTestResult) -> Option<u16> {
    match &result.model_generation {
        ModelAvailabilityCheckResult::Failed { provider_status, .. } => *provider_status,
        ModelAvailabilityCheckResult::Passed { .. }
        | ModelAvailabilityCheckResult::NotRun { .. } => None,
    }
}

fn issue_code(value: &str) -> NormalizedIssueCode {
    NormalizedIssueCode::new(value).expect("P05 uses only registered normalized issue codes")
}

fn compile_probe(
    api_format: ModelApiFormat,
    input: ModelAvailabilityProbeCompilationInput<'_>,
) -> Result<ProviderRequestPlan, ModelAdapterError> {
    match api_format {
        ModelApiFormat::OpenAiResponses => OpenAiResponsesAdapter.compile_generation_probe(input),
        ModelApiFormat::OpenAiChatCompletions => {
            OpenAiChatCompletionsAdapter.compile_generation_probe(input)
        }
        ModelApiFormat::AnthropicMessages => {
            AnthropicMessagesAdapter.compile_generation_probe(input)
        }
    }
}

fn validate_success_body(api_format: ModelApiFormat, body: &[u8]) -> Result<(), ModelAdapterError> {
    match api_format {
        ModelApiFormat::OpenAiResponses => {
            OpenAiResponsesAdapter.validate_generation_probe_success_body(body)
        }
        ModelApiFormat::OpenAiChatCompletions => {
            OpenAiChatCompletionsAdapter.validate_generation_probe_success_body(body)
        }
        ModelApiFormat::AnthropicMessages => {
            AnthropicMessagesAdapter.validate_generation_probe_success_body(body)
        }
    }
}

fn availability_result_for_response(
    endpoint: &ModelEndpointConfig,
    profile: &ModelProfileConfig,
    started_at: UtcTimestamp,
    response: &ConfigurationTransportResponse,
) -> ModelAvailabilityTestResult {
    let completed_at = UtcTimestamp::now();
    let passed = passed_check();
    let blocked = blocked_check();
    let tls = tls_passed_or_not_applicable(endpoint);
    let (authentication, model_generation) = match response.status {
        401 | 403 => (failed_check("authentication.rejected", Some(response.status)), blocked),
        status if !(200..300).contains(&status) => (
            ModelAvailabilityCheckResult::NotRun {
                reason_code: ModelAvailabilityCheckNotRunReason::AuthenticationNotConfirmed,
            },
            failed_check(provider_status_issue(status), Some(status)),
        ),
        _ => {
            let authentication = authentication_passed_or_not_required(endpoint);
            let generation = validate_success_body(endpoint.api_format, &response.body)
                .map_or_else(
                    |error| failed_check(adapter_failure_issue(&error), Some(response.status)),
                    |()| passed_check_with_latency(response.elapsed_ms),
                );
            (authentication, generation)
        }
    };
    let mut result = result(
        endpoint,
        profile,
        started_at,
        completed_at,
        AvailabilityChecks {
            config: passed.clone(),
            network: passed.clone(),
            tls,
            http: passed,
            authentication,
            model_generation,
        },
        Vec::new(),
    );
    if !(200..300).contains(&response.status) {
        result.provider_error = Some(summarize_provider_error(
            issue_code(provider_status_issue(response.status)),
            Some(response.status),
            Some(&response.body),
        ));
    }
    result
}

fn availability_result_for_transport_failure(
    endpoint: &ModelEndpointConfig,
    profile: &ModelProfileConfig,
    started_at: UtcTimestamp,
    failure: &ConfigurationTransportFailure,
) -> ModelAvailabilityTestResult {
    let completed_at = UtcTimestamp::now();
    let blocked = blocked_check();
    let issue = failure.issue_code.as_str();
    let provider_status = failure.failure.failure.provider_status;

    let checks = if failure.http_response_received {
        let status = provider_status.expect("observed HTTP failure records provider status");
        let (authentication, model_generation) = match status {
            401 | 403 => (failed_check("authentication.rejected", Some(status)), blocked.clone()),
            status if !(200..300).contains(&status) => (
                ModelAvailabilityCheckResult::NotRun {
                    reason_code: ModelAvailabilityCheckNotRunReason::AuthenticationNotConfirmed,
                },
                failed_check(provider_status_issue(status), Some(status)),
            ),
            _ => {
                (authentication_passed_or_not_required(endpoint), failed_check(issue, Some(status)))
            }
        };
        (
            passed_check(),
            passed_check(),
            tls_passed_or_not_applicable(endpoint),
            passed_check(),
            authentication,
            model_generation,
        )
    } else if issue.starts_with("credential.")
        || issue.starts_with("configuration.")
        || issue == "internal.unexpected"
    {
        (
            failed_check(issue, None),
            blocked.clone(),
            blocked.clone(),
            blocked.clone(),
            blocked.clone(),
            blocked,
        )
    } else if issue.starts_with("tls.") {
        (
            passed_check(),
            passed_check(),
            failed_check(issue, None),
            blocked.clone(),
            blocked.clone(),
            blocked,
        )
    } else if issue == "network.redirect_blocked" {
        (
            passed_check(),
            passed_check(),
            tls_passed_or_not_applicable(endpoint),
            failed_check(issue, None),
            blocked.clone(),
            blocked,
        )
    } else {
        (
            passed_check(),
            failed_check(issue, None),
            blocked.clone(),
            blocked.clone(),
            blocked.clone(),
            blocked,
        )
    };
    let mut result = result(
        endpoint,
        profile,
        started_at,
        completed_at,
        AvailabilityChecks {
            config: checks.0,
            network: checks.1,
            tls: checks.2,
            http: checks.3,
            authentication: checks.4,
            model_generation: checks.5,
        },
        Vec::new(),
    );
    if failure.http_response_received {
        result.provider_error =
            Some(summarize_provider_error(failure.issue_code.clone(), provider_status, None));
    }
    result
}

struct AvailabilityChecks {
    config: ModelAvailabilityCheckResult,
    network: ModelAvailabilityCheckResult,
    tls: ModelAvailabilityCheckResult,
    http: ModelAvailabilityCheckResult,
    authentication: ModelAvailabilityCheckResult,
    model_generation: ModelAvailabilityCheckResult,
}

fn result(
    endpoint: &ModelEndpointConfig,
    profile: &ModelProfileConfig,
    started_at: UtcTimestamp,
    completed_at: UtcTimestamp,
    checks: AvailabilityChecks,
    warnings: Vec<NormalizedIssueCode>,
) -> ModelAvailabilityTestResult {
    let outcome = if checks.model_generation.is_passed() {
        ModelAvailabilityTestOutcome::Available
    } else {
        ModelAvailabilityTestOutcome::Unavailable
    };
    ModelAvailabilityTestResult {
        outcome,
        config: checks.config,
        network: checks.network,
        tls: checks.tls,
        http: checks.http,
        authentication: checks.authentication,
        model_generation: checks.model_generation,
        tested_endpoint_id: endpoint.id,
        tested_model_profile_id: profile.id,
        tested_model_name: profile.provider_model_name.clone(),
        started_at,
        completed_at,
        warnings,
        provider_error: None,
    }
}

fn passed_check() -> ModelAvailabilityCheckResult {
    ModelAvailabilityCheckResult::Passed { latency_ms: None }
}

fn passed_check_with_latency(latency_ms: u64) -> ModelAvailabilityCheckResult {
    ModelAvailabilityCheckResult::Passed { latency_ms: Some(latency_ms) }
}

fn blocked_check() -> ModelAvailabilityCheckResult {
    ModelAvailabilityCheckResult::NotRun {
        reason_code: ModelAvailabilityCheckNotRunReason::BlockedByPreviousStage,
    }
}

fn failed_check(code: &str, provider_status: Option<u16>) -> ModelAvailabilityCheckResult {
    ModelAvailabilityCheckResult::Failed {
        error_code: NormalizedIssueCode::new(code)
            .expect("P05 availability uses only registered normalized issue codes"),
        provider_status,
    }
}

fn tls_passed_or_not_applicable(endpoint: &ModelEndpointConfig) -> ModelAvailabilityCheckResult {
    if endpoint.base_url.starts_with("http://") {
        ModelAvailabilityCheckResult::NotRun {
            reason_code: ModelAvailabilityCheckNotRunReason::NotApplicableForScheme,
        }
    } else {
        passed_check()
    }
}

fn authentication_passed_or_not_required(
    endpoint: &ModelEndpointConfig,
) -> ModelAvailabilityCheckResult {
    if endpoint.auth_scheme == EndpointAuthScheme::None {
        ModelAvailabilityCheckResult::NotRun {
            reason_code: ModelAvailabilityCheckNotRunReason::NoAuthenticationRequired,
        }
    } else {
        passed_check()
    }
}

const fn provider_status_issue(status: u16) -> &'static str {
    if status == 429 {
        "provider.rate_limited"
    } else if status >= 500 {
        "provider.server_error"
    } else {
        "provider.request_rejected"
    }
}

fn adapter_failure_issue(error: &ModelAdapterError) -> &'static str {
    if error.code.ends_with("visible_text_missing") {
        "provider.visible_text_missing"
    } else {
        "provider.invalid_response"
    }
}

fn prerequisite(reason: ModelAvailabilityPrerequisiteFailure) -> ModelAvailabilityServiceError {
    ModelAvailabilityServiceError::Prerequisite(reason)
}

fn next_revision(revision: Revision) -> Result<Revision, ModelAvailabilityServiceError> {
    revision
        .value()
        .checked_add(1)
        .map(Revision::new)
        .ok_or(ModelAvailabilityServiceError::RevisionOverflow)
}

pub(crate) fn model_availability_identity(
    command_invocation_id: CommandInvocationId,
    correlation_id: CorrelationId,
    channel: aura_network_control::AuthenticatedConfigurationChannelIdentity,
    prepared: &PreparedModelAvailabilityTest,
) -> UserConfigurationOperationIdentity {
    UserConfigurationOperationIdentity::from_authenticated_settings_command(
        command_invocation_id,
        correlation_id,
        channel,
        ConfigurationOperationKind::ModelAvailabilityTest,
        prepared.endpoint.id,
        prepared.models_revision,
        UtcTimestamp::now(),
    )
}

#[cfg(test)]
mod tests {
    use aura_model_domain::ChatOutputTokenField;

    use super::{
        alternate_output_token_field, provider_status_issue, response_explicitly_rejects_field,
    };

    #[test]
    fn non_auth_http_statuses_use_the_frozen_provider_issue_codes() {
        assert_eq!(provider_status_issue(429), "provider.rate_limited");
        assert_eq!(provider_status_issue(500), "provider.server_error");
        assert_eq!(provider_status_issue(404), "provider.request_rejected");
    }

    #[test]
    fn alias_retry_requires_an_exact_structured_error_param() {
        let explicit = br#"{"error":{"message":"unsupported","param":"max_tokens"}}"#;
        assert!(response_explicitly_rejects_field(explicit, ChatOutputTokenField::MaxTokens));
        assert!(!response_explicitly_rejects_field(
            explicit,
            ChatOutputTokenField::MaxCompletionTokens
        ));

        let message_only = br#"{"error":{"message":"max_tokens is unsupported","param":null}}"#;
        assert!(!response_explicitly_rejects_field(message_only, ChatOutputTokenField::MaxTokens));
        assert!(!response_explicitly_rejects_field(
            b"not-json max_tokens",
            ChatOutputTokenField::MaxTokens
        ));
    }

    #[test]
    fn alias_retry_switches_only_between_the_two_frozen_fields() {
        assert_eq!(
            alternate_output_token_field(ChatOutputTokenField::MaxTokens),
            ChatOutputTokenField::MaxCompletionTokens
        );
        assert_eq!(
            alternate_output_token_field(ChatOutputTokenField::MaxCompletionTokens),
            ChatOutputTokenField::MaxTokens
        );
    }
}
