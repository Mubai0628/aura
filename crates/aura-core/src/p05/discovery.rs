use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use aura_domain_common::{
    CommandInvocationId, ContentHash, CorrelationId, EndpointId, ModelDiscoveryRunId,
    ModelFailureId, Revision, UtcTimestamp,
};
use aura_model_catalog::{
    CatalogCache, CatalogError, ModelDiscoveryAccumulator, ModelDiscoveryPageAction,
    PersistenceCatalogCache, parse_model_discovery_page,
};
use aura_model_config::{
    ModelConfigError, endpoint_fingerprint, merge_discovery_results, resolve_models_url,
};
use aura_model_domain::{
    CancellationSource, FailureFingerprint, ModelDiscoveryMergeResult, ModelDiscoveryOutcome,
    ModelDiscoveryRunRecord, ModelDiscoveryRunView, ModelDiscoveryStage, ModelDiscoveryState,
    ModelDiscoveryUnsupportedReason, ModelEndpointConfig, ModelEndpointUpdatedPayload,
    ModelFailureCategory, ModelProfileCreatedPayload, ModelProfileUpdatedPayload, ModelRetryClass,
    NormalizedModelFailure, PersistedModelFailure, TransportAttemptKind, TransportAttemptRecord,
    TransportOperationContext,
};
use aura_model_transport::{
    ModelDiscoveryCompilationInput, ProviderRequestPlan, build_model_discovery_request_plan,
};
use aura_network_control::{
    AuthenticatedConfigurationChannelIdentity, ConfigurationOperationKind,
    ConfigurationRequestCredentialBindingV1, ConfigurationRequestPurpose,
    UserConfigurationOperationIdentity,
};
use aura_persistence::{
    MODEL_CONFIG_MUTATION_JOURNAL_KIND, ModelDiscoveryTerminalEventContext,
    ModelDiscoveryTransportTransition, ModelProjectionDraft, PersistenceError,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

use super::{
    P05OperationControl, P05Services,
    commands::{endpoint_changed_fields, prepared_snapshot, profile_changed_fields, user_event},
    config_journal::{ModelConfigCommandPlan, ModelConfigJournalError},
    configuration_transport::{
        ConfigurationTransportError, ConfigurationTransportExecuteRequest,
        ConfigurationTransportOutcome,
    },
    provider_error::summarize_provider_error,
};

pub(crate) struct PreparedModelDiscovery {
    id: ModelDiscoveryRunId,
    source_models_revision: Revision,
    endpoint: ModelEndpointConfig,
    endpoint_fingerprint: ContentHash,
    initial_url: Option<Url>,
    operation_context: TransportOperationContext,
    credential_binding: Option<ConfigurationRequestCredentialBindingV1>,
}

pub(crate) struct ModelDiscoveryCommandCompletion {
    pub projection_changes: Vec<aura_projection_contract::ProjectionChange>,
}

pub(crate) struct ModelDiscoveryQueueCompletion {
    pub accepted: aura_model_domain::ModelDiscoveryStartAccepted,
    pub projection_changes: Vec<aura_projection_contract::ProjectionChange>,
    pub newly_queued: bool,
}

#[derive(Debug, Error)]
pub(crate) enum ModelDiscoveryServiceError {
    #[error("model discovery configuration is invalid")]
    InvalidConfiguration,
    #[error("model discovery configuration revision changed")]
    ConfigurationRevisionChanged,
    #[error("model discovery credential is unavailable")]
    CredentialUnavailable,
    #[error("another model-config mutation is active")]
    ModelConfigMutationInProgress,
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(transparent)]
    Transport(#[from] ConfigurationTransportError),
    #[error(transparent)]
    ConfigJournal(#[from] ModelConfigJournalError),
    #[error("model discovery revision overflow")]
    RevisionOverflow,
    #[error("model discovery projection could not be constructed")]
    MissingProjection,
    #[error("model discovery operation was not found")]
    OperationNotFound,
    #[error("model discovery worker terminated unexpectedly")]
    WorkerTerminated,
    #[error("model discovery operational revision conflict: expected {expected}, actual {actual}")]
    OperationRevisionConflict { expected: u64, actual: u64 },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DiscoveryMergeCommit {
    result: ModelDiscoveryMergeResult,
    models_revision: Revision,
}

impl P05Services {
    pub(crate) async fn cancel_model_discovery_for_deleted_endpoints(
        &self,
        endpoint_ids: &BTreeSet<EndpointId>,
    ) -> Result<Vec<aura_projection_contract::ProjectionChange>, ModelDiscoveryServiceError> {
        let targets = self
            .persistence
            .active_model_discovery_runs()?
            .into_iter()
            .filter(|record| endpoint_ids.contains(&record.endpoint_id))
            .map(|record| record.id)
            .collect::<Vec<_>>();
        for operation_id in targets {
            let Some(record) = self.persistence.model_discovery_run(operation_id)? else {
                continue;
            };
            if record.state.is_terminal() {
                continue;
            }
            let Some(control) = self.operation_cancellations.discovery(operation_id) else {
                let current = self
                    .persistence
                    .model_discovery_run(operation_id)?
                    .ok_or(ModelDiscoveryServiceError::OperationNotFound)?;
                if current.state.is_terminal() {
                    continue;
                }
                return Err(ModelDiscoveryServiceError::WorkerTerminated);
            };
            control.cancel(aura_model_domain::ModelOperationCancellationReason::TargetDeleted);

            if record.state == ModelDiscoveryState::Queued {
                // A concurrent Running transition delegates terminalization to
                // the Worker so transport cancellation remains first.
                match self.commit_discovery_cancelled(
                    record,
                    Vec::new(),
                    aura_model_domain::ModelOperationCancellationReason::TargetDeleted,
                ) {
                    Ok(completion) => {
                        self.persistence
                            .publish_model_projection_changes(&completion.projection_changes)?;
                        control.mark_terminal_projection_published();
                    }
                    Err(ModelDiscoveryServiceError::Persistence(
                        PersistenceError::ModelDiscoveryRunStateConflict,
                    )) => {}
                    Err(error) => return Err(error),
                }
            }

            control.wait_for_terminal_projection().await;
            let current = self
                .persistence
                .model_discovery_run(operation_id)?
                .ok_or(ModelDiscoveryServiceError::OperationNotFound)?;
            if !current.state.is_terminal() {
                return Err(ModelDiscoveryServiceError::WorkerTerminated);
            }
        }
        Ok(Vec::new())
    }

    pub(crate) fn queue_model_discovery(
        &self,
        prepared: &PreparedModelDiscovery,
        identity: &UserConfigurationOperationIdentity,
        correlation_id: CorrelationId,
    ) -> Result<ModelDiscoveryQueueCompletion, ModelDiscoveryServiceError> {
        validate_discovery_identity(prepared, identity)?;
        self.revalidate_prepared_discovery(prepared)?;
        if let Some(existing) = self
            .persistence
            .latest_model_discovery_run_for_endpoint(prepared.endpoint.id)?
            .filter(|record| !record.state.is_terminal())
        {
            return Ok(ModelDiscoveryQueueCompletion {
                accepted: aura_model_domain::ModelDiscoveryStartAccepted {
                    model_discovery_run_id: existing.id,
                    operational_revision: existing.revision,
                    state: existing.state,
                },
                projection_changes: Vec::new(),
                newly_queued: false,
            });
        }
        let record = new_discovery_record(prepared, correlation_id);
        let projection_changes = self
            .persistence
            .create_model_discovery_run(
                &record,
                self.discovery_endpoint_projection_drafts(&record)?,
            )?
            .projection_changes;
        Ok(ModelDiscoveryQueueCompletion {
            accepted: aura_model_domain::ModelDiscoveryStartAccepted {
                model_discovery_run_id: record.id,
                operational_revision: record.revision,
                state: record.state,
            },
            projection_changes,
            newly_queued: true,
        })
    }

    pub(crate) fn prepare_model_discovery(
        &self,
        request: aura_model_domain::ModelDiscoveryStartRequest,
    ) -> Result<PreparedModelDiscovery, ModelDiscoveryServiceError> {
        ensure_model_config_is_stable(self)?;
        let snapshot = self
            .model_config
            .snapshot()
            .map_err(|_| ModelDiscoveryServiceError::InvalidConfiguration)?;
        if snapshot.revision != request.expected_models_revision.value() {
            return Err(ModelDiscoveryServiceError::ConfigurationRevisionChanged);
        }
        let endpoint = snapshot
            .registry
            .endpoints
            .get(&request.endpoint_id)
            .cloned()
            .ok_or(ModelDiscoveryServiceError::InvalidConfiguration)?;
        if endpoint.revision.value() != request.expected_endpoint_revision.value() {
            return Err(ModelDiscoveryServiceError::ConfigurationRevisionChanged);
        }
        let endpoint_fingerprint = endpoint_fingerprint(&endpoint)
            .map_err(|_| ModelDiscoveryServiceError::InvalidConfiguration)?;
        let initial_url = resolve_models_url(&endpoint.base_url, &endpoint.discovery_mode)
            .map_err(|_| ModelDiscoveryServiceError::InvalidConfiguration)?;
        let credential_binding = if initial_url.is_some() {
            self.configuration_credential_preflight(&endpoint)
                .map_err(|_| ModelDiscoveryServiceError::CredentialUnavailable)?
                .credential_binding
        } else {
            None
        };
        let id = ModelDiscoveryRunId::new();
        Ok(PreparedModelDiscovery {
            id,
            source_models_revision: Revision::new(snapshot.revision),
            endpoint,
            endpoint_fingerprint,
            initial_url,
            operation_context: TransportOperationContext::ModelDiscovery { discovery_run_id: id },
            credential_binding,
        })
    }

    pub(crate) async fn execute_model_discovery(
        &self,
        prepared: PreparedModelDiscovery,
        identity: UserConfigurationOperationIdentity,
        correlation_id: CorrelationId,
        control: P05OperationControl,
    ) -> Result<ModelDiscoveryCommandCompletion, ModelDiscoveryServiceError> {
        validate_discovery_identity(&prepared, &identity)?;
        self.revalidate_prepared_discovery(&prepared)?;
        let cancellation = control.cancellation();
        let mut record = self
            .persistence
            .model_discovery_run(prepared.id)?
            .ok_or(ModelDiscoveryServiceError::OperationNotFound)?;
        let mut projection_changes = Vec::new();

        if cancellation.is_cancelled() {
            return self.finish_discovery_cancelled(
                record,
                projection_changes,
                control
                    .cancellation_reason()
                    .unwrap_or(aura_model_domain::ModelOperationCancellationReason::UserRequested),
            );
        }

        let Some(initial_url) = prepared.initial_url.clone() else {
            let expected = record.revision;
            record.revision = next_revision(expected)?;
            record.state = ModelDiscoveryState::Completed;
            record.stage = Some(ModelDiscoveryStage::Completed);
            record.outcome = Some(ModelDiscoveryOutcome::Unsupported);
            record.finished_at = Some(UtcTimestamp::now());
            let terminal = self.persistence.transition_model_discovery_run(
                expected,
                &record,
                Some(ModelDiscoveryTerminalEventContext::Unsupported {
                    reason_code:
                        ModelDiscoveryUnsupportedReason::ModelsEndpointDisabledByConfiguration,
                }),
                self.discovery_endpoint_projection_drafts(&record)?,
            )?;
            projection_changes.extend(terminal.projection_changes);
            return Ok(ModelDiscoveryCommandCompletion { projection_changes });
        };

        let mut accumulator =
            ModelDiscoveryAccumulator::new(prepared.endpoint.api_format, initial_url)
                .map_err(|_| ModelDiscoveryServiceError::InvalidConfiguration)?;
        let mut request_url = accumulator
            .initial_url()
            .map_err(|_| ModelDiscoveryServiceError::InvalidConfiguration)?;
        let mut attempt_number = 1_u32;
        let model_ids = loop {
            if cancellation.is_cancelled() {
                return self.finish_discovery_cancelled(
                    record,
                    projection_changes,
                    control.cancellation_reason().unwrap_or(
                        aura_model_domain::ModelOperationCancellationReason::UserRequested,
                    ),
                );
            }
            let plan = compile_discovery_request(
                &prepared.endpoint,
                &prepared.operation_context,
                &request_url,
            )
            .map_err(|_| ModelDiscoveryServiceError::InvalidConfiguration)?;
            let services = self.clone();
            let operation_id = prepared.id;
            let outcome = self
                .configuration_transport
                .execute(ConfigurationTransportExecuteRequest {
                    identity: identity.clone(),
                    correlation_id,
                    models_revision: prepared.source_models_revision,
                    endpoint_id: prepared.endpoint.id,
                    endpoint_security_fingerprint: prepared.endpoint_fingerprint.clone(),
                    purpose: ConfigurationRequestPurpose::ModelDiscovery {
                        model_discovery_run_id: prepared.id,
                    },
                    operation_context: prepared.operation_context.clone(),
                    request_plan: plan,
                    credential_binding: prepared.credential_binding.clone(),
                    attempt_number,
                    attempt_kind: TransportAttemptKind::Initial,
                    timeout_ms: prepared
                        .endpoint
                        .timeouts
                        .total_ms
                        .unwrap_or(prepared.endpoint.timeouts.first_event_ms),
                    cancellation: cancellation.clone(),
                    persist_and_bind_attempt: Arc::new(move |attempt| {
                        services
                            .bind_discovery_transport_attempt(operation_id, attempt)
                            .map_err(|error| error.to_string())
                    }),
                })
                .await?;
            record = self
                .persistence
                .model_discovery_run(prepared.id)?
                .ok_or(ModelDiscoveryServiceError::OperationNotFound)?;
            match outcome {
                ConfigurationTransportOutcome::Cancelled(cancelled) => {
                    debug_assert!(record.transport_attempt_ids.contains(&cancelled.attempt.id));
                    return self.finish_discovery_cancelled_with_attempt(
                        record,
                        projection_changes,
                        control.cancellation_reason().unwrap_or(
                            aura_model_domain::ModelOperationCancellationReason::UserRequested,
                        ),
                        cancelled.attempt,
                    );
                }
                ConfigurationTransportOutcome::Failed(failure) => {
                    debug_assert!(record.transport_attempt_ids.contains(&failure.attempt.id));
                    record.failure_id = Some(failure.failure.id);
                    if failure.http_response_received {
                        record.provider_error = Some(summarize_provider_error(
                            failure.issue_code.clone(),
                            failure.failure.failure.provider_status,
                            None,
                        ));
                    }
                    if record.raw_response_artifact_id.is_none() {
                        record.raw_response_artifact_id = failure.attempt.raw_stream_artifact_id;
                    }
                    return self.finish_or_partial_discovery_failure(
                        &identity,
                        correlation_id,
                        record,
                        projection_changes,
                        accumulator.collected_model_ids(),
                        failure.attempt,
                    );
                }
                ConfigurationTransportOutcome::Response(response) => {
                    debug_assert!(record.transport_attempt_ids.contains(&response.attempt.id));
                    if record.raw_response_artifact_id.is_none() {
                        record.raw_response_artifact_id = response.raw_response_artifact_id;
                    }
                    if !(200..300).contains(&response.status) {
                        record.provider_error = Some(summarize_provider_error(
                            aura_domain_common::NormalizedIssueCode::new(provider_status_issue(
                                response.status,
                            ))
                            .expect("static discovery Provider issue code is registered"),
                            Some(response.status),
                            Some(&response.body),
                        ));
                        record.failure_id = Some(self.store_discovery_failure(
                            &record,
                            provider_status_issue(response.status),
                            provider_status_category(response.status),
                            Some(response.status),
                            Some(response.attempt.id),
                            response.raw_response_artifact_id,
                        )?);
                        return self.finish_or_partial_discovery_failure(
                            &identity,
                            correlation_id,
                            record,
                            projection_changes,
                            accumulator.collected_model_ids(),
                            response.attempt,
                        );
                    }
                    let page = match parse_model_discovery_page(
                        prepared.endpoint.api_format,
                        &response.body,
                    ) {
                        Ok(page) => page,
                        Err(error) => {
                            let code = catalog_issue_code(&error);
                            record.failure_id = Some(self.store_discovery_failure(
                                &record,
                                code,
                                ModelFailureCategory::Protocol,
                                Some(response.status),
                                Some(response.attempt.id),
                                response.raw_response_artifact_id,
                            )?);
                            return self.finish_or_partial_discovery_failure(
                                &identity,
                                correlation_id,
                                record,
                                projection_changes,
                                accumulator.collected_model_ids(),
                                response.attempt,
                            );
                        }
                    };
                    match accumulator.accept_page(&response.body, page) {
                        Ok(ModelDiscoveryPageAction::Continue { next_url }) => {
                            let expected = record.revision;
                            record.revision = next_revision(expected)?;
                            let commit = self
                                .persistence
                                .transition_model_discovery_run_with_transport_attempt(
                                    ModelDiscoveryTransportTransition {
                                        correlation_id,
                                        expected_revision: expected,
                                        next: &record,
                                        terminal_event: None,
                                        attempt: &response.attempt,
                                        cancelled_by: None,
                                        projection_drafts: self
                                            .discovery_endpoint_projection_drafts(&record)?,
                                    },
                                )?;
                            projection_changes.extend(commit.projection_changes);
                            request_url = next_url;
                            attempt_number = attempt_number
                                .checked_add(1)
                                .ok_or(ModelDiscoveryServiceError::RevisionOverflow)?;
                        }
                        Ok(ModelDiscoveryPageAction::Complete { model_ids }) => {
                            let expected = record.revision;
                            record.revision = next_revision(expected)?;
                            record.stage = Some(ModelDiscoveryStage::Parsing);
                            let commit = self
                                .persistence
                                .transition_model_discovery_run_with_transport_attempt(
                                    ModelDiscoveryTransportTransition {
                                        correlation_id,
                                        expected_revision: expected,
                                        next: &record,
                                        terminal_event: None,
                                        attempt: &response.attempt,
                                        cancelled_by: None,
                                        projection_drafts: self
                                            .discovery_endpoint_projection_drafts(&record)?,
                                    },
                                )?;
                            projection_changes.extend(commit.projection_changes);
                            break model_ids;
                        }
                        Err(error) => {
                            let code = catalog_issue_code(&error);
                            record.failure_id = Some(self.store_discovery_failure(
                                &record,
                                code,
                                ModelFailureCategory::Protocol,
                                Some(response.status),
                                Some(response.attempt.id),
                                response.raw_response_artifact_id,
                            )?);
                            return self.finish_or_partial_discovery_failure(
                                &identity,
                                correlation_id,
                                record,
                                projection_changes,
                                accumulator.collected_model_ids(),
                                response.attempt,
                            );
                        }
                    }
                }
            }
        };

        transition_discovery_stage(self, &mut record, ModelDiscoveryStage::EnrichingCatalog)?;
        let merge = match self.merge_discovery(
            identity.command_invocation_id(),
            correlation_id,
            &record,
            &model_ids,
        ) {
            Ok(merge) => merge,
            Err(error @ ModelDiscoveryServiceError::ConfigurationRevisionChanged) => {
                record.failure_id = Some(self.store_discovery_failure(
                    &record,
                    "configuration.discovery_stale",
                    ModelFailureCategory::InvalidRequest,
                    None,
                    None,
                    None,
                )?);
                let _ = error;
                return self.finish_discovery_failed(record, projection_changes);
            }
            Err(error) => return Err(error),
        };
        projection_changes.extend(merge.projection_changes);

        let expected = record.revision;
        record.revision = next_revision(expected)?;
        record.state = ModelDiscoveryState::Completed;
        record.stage = Some(ModelDiscoveryStage::Completed);
        record.outcome = Some(ModelDiscoveryOutcome::Succeeded);
        record.result = Some(merge.commit.result);
        record.finished_at = Some(UtcTimestamp::now());
        let terminal = self.persistence.transition_model_discovery_run(
            expected,
            &record,
            Some(ModelDiscoveryTerminalEventContext::Completed {
                models_revision: merge.commit.models_revision,
            }),
            self.discovery_endpoint_projection_drafts(&record)?,
        )?;
        projection_changes.extend(terminal.projection_changes);
        Ok(ModelDiscoveryCommandCompletion { projection_changes })
    }

    fn bind_discovery_transport_attempt(
        &self,
        model_discovery_run_id: ModelDiscoveryRunId,
        transport_attempt: &TransportAttemptRecord,
    ) -> Result<(), ModelDiscoveryServiceError> {
        let mut record = self
            .persistence
            .model_discovery_run(model_discovery_run_id)?
            .ok_or(ModelDiscoveryServiceError::OperationNotFound)?;
        if record.state.is_terminal() {
            return Err(ModelDiscoveryServiceError::Persistence(
                PersistenceError::ModelDiscoveryRunStateConflict,
            ));
        }
        let expected = record.revision;
        record.revision = next_revision(expected)?;
        record.state = ModelDiscoveryState::Running;
        record.stage = Some(ModelDiscoveryStage::Requesting);
        record.transport_attempt_ids.push(transport_attempt.id);
        let commit = self.persistence.start_model_discovery_transport_attempt(
            record.correlation_id,
            expected,
            &record,
            transport_attempt,
            self.discovery_endpoint_projection_drafts(&record)?,
        )?;
        self.persistence.publish_model_projection_changes(&commit.projection_changes)?;
        Ok(())
    }

    fn revalidate_prepared_discovery(
        &self,
        prepared: &PreparedModelDiscovery,
    ) -> Result<(), ModelDiscoveryServiceError> {
        ensure_model_config_is_stable(self)?;
        let snapshot = self
            .model_config
            .snapshot()
            .map_err(|_| ModelDiscoveryServiceError::ConfigurationRevisionChanged)?;
        let endpoint = snapshot.registry.endpoints.get(&prepared.endpoint.id);
        if snapshot.revision != prepared.source_models_revision.value()
            || endpoint != Some(&prepared.endpoint)
        {
            return Err(ModelDiscoveryServiceError::ConfigurationRevisionChanged);
        }
        Ok(())
    }

    fn discovery_endpoint_projection_drafts(
        &self,
        record: &ModelDiscoveryRunRecord,
    ) -> Result<Vec<ModelProjectionDraft>, ModelDiscoveryServiceError> {
        let snapshot = self
            .model_config
            .snapshot()
            .map_err(|_| ModelDiscoveryServiceError::MissingProjection)?;
        if !snapshot.registry.endpoints.contains_key(&record.endpoint_id) {
            return Ok(Vec::new());
        }
        let detail = self
            .projections
            .endpoint_detail_with_latest(
                &snapshot,
                record.endpoint_id,
                &BTreeMap::new(),
                Some(self.discovery_view(record)?),
            )
            .map_err(|_| ModelDiscoveryServiceError::MissingProjection)?;
        Ok(vec![ModelProjectionDraft::EndpointDetail(Box::new(detail))])
    }

    fn store_discovery_failure(
        &self,
        record: &ModelDiscoveryRunRecord,
        code: &str,
        category: ModelFailureCategory,
        provider_status: Option<u16>,
        transport_attempt_id: Option<aura_domain_common::TransportAttemptId>,
        provider_error_ref: Option<aura_domain_common::ArtifactId>,
    ) -> Result<ModelFailureId, ModelDiscoveryServiceError> {
        let id = ModelFailureId::new();
        let failure = PersistedModelFailure {
            id,
            fingerprint: FailureFingerprint(ContentHash::sha256(
                format!("{}|{}|{:?}", record.endpoint_id, code, provider_status).as_bytes(),
            )),
            failure: NormalizedModelFailure {
                category,
                code: code.to_owned(),
                message:
                    "Model discovery failed before a complete valid model catalog was committed."
                        .to_owned(),
                retry_class: if code == "provider.rate_limited"
                    || code == "provider.server_error"
                    || code.starts_with("network.")
                {
                    ModelRetryClass::TransparentRetry { retry_after_ms: None }
                } else {
                    ModelRetryClass::NotRetryable
                },
                provider_status,
                provider_request_id: None,
                provider_error_ref,
                partial_output_ref: None,
                transport_attempt_id,
            },
            created_at: UtcTimestamp::now(),
        };
        self.persistence.store_model_failure(&failure)?;
        Ok(id)
    }

    fn finish_discovery_failed(
        &self,
        record: ModelDiscoveryRunRecord,
        projection_changes: Vec<aura_projection_contract::ProjectionChange>,
    ) -> Result<ModelDiscoveryCommandCompletion, ModelDiscoveryServiceError> {
        self.commit_discovery_failed(record, projection_changes, None)
    }

    fn finish_discovery_failed_with_attempt(
        &self,
        record: ModelDiscoveryRunRecord,
        projection_changes: Vec<aura_projection_contract::ProjectionChange>,
        attempt: TransportAttemptRecord,
    ) -> Result<ModelDiscoveryCommandCompletion, ModelDiscoveryServiceError> {
        self.commit_discovery_failed(record, projection_changes, Some(attempt))
    }

    fn commit_discovery_failed(
        &self,
        mut record: ModelDiscoveryRunRecord,
        mut projection_changes: Vec<aura_projection_contract::ProjectionChange>,
        attempt: Option<TransportAttemptRecord>,
    ) -> Result<ModelDiscoveryCommandCompletion, ModelDiscoveryServiceError> {
        let expected = record.revision;
        record.revision = next_revision(expected)?;
        record.state = ModelDiscoveryState::Completed;
        record.stage = Some(ModelDiscoveryStage::Completed);
        record.outcome = Some(ModelDiscoveryOutcome::Failed);
        record.finished_at = Some(UtcTimestamp::now());
        let drafts = self.discovery_endpoint_projection_drafts(&record)?;
        let terminal = if let Some(attempt) = attempt {
            self.persistence.transition_model_discovery_run_with_transport_attempt(
                ModelDiscoveryTransportTransition {
                    correlation_id: record.correlation_id,
                    expected_revision: expected,
                    next: &record,
                    terminal_event: Some(ModelDiscoveryTerminalEventContext::Failed),
                    attempt: &attempt,
                    cancelled_by: None,
                    projection_drafts: drafts,
                },
            )?
        } else {
            self.persistence.transition_model_discovery_run(
                expected,
                &record,
                Some(ModelDiscoveryTerminalEventContext::Failed),
                drafts,
            )?
        };
        projection_changes.extend(terminal.projection_changes);
        Ok(ModelDiscoveryCommandCompletion { projection_changes })
    }

    pub(crate) fn terminalize_model_discovery_error(
        &self,
        model_discovery_run_id: ModelDiscoveryRunId,
        error: &ModelDiscoveryServiceError,
        requested_cancellation: Option<aura_model_domain::ModelOperationCancellationReason>,
    ) -> Result<ModelDiscoveryCommandCompletion, ModelDiscoveryServiceError> {
        let Some(mut record) = self.persistence.model_discovery_run(model_discovery_run_id)? else {
            return Ok(ModelDiscoveryCommandCompletion { projection_changes: Vec::new() });
        };
        if record.state.is_terminal() {
            return Ok(ModelDiscoveryCommandCompletion { projection_changes: Vec::new() });
        }

        let cancellation_reason = requested_cancellation
            .or_else(|| self.model_discovery_error_cancellation_reason(&record, error));
        if let Some(reason) = cancellation_reason {
            return self.finish_discovery_cancelled(record, Vec::new(), reason);
        }

        let (issue_code, category) = match error {
            ModelDiscoveryServiceError::CredentialUnavailable => {
                ("credential.unavailable", ModelFailureCategory::Authentication)
            }
            ModelDiscoveryServiceError::InvalidConfiguration => {
                ("configuration.invalid", ModelFailureCategory::InvalidRequest)
            }
            ModelDiscoveryServiceError::Persistence(_)
            | ModelDiscoveryServiceError::ConfigJournal(_)
            | ModelDiscoveryServiceError::Transport(
                ConfigurationTransportError::Persistence(_)
                | ConfigurationTransportError::OperationBinding(_),
            ) => ("persistence.operation_failed", ModelFailureCategory::Persistence),
            ModelDiscoveryServiceError::Transport(_) => {
                ("configuration.invalid", ModelFailureCategory::InvalidRequest)
            }
            ModelDiscoveryServiceError::RevisionOverflow
            | ModelDiscoveryServiceError::MissingProjection
            | ModelDiscoveryServiceError::OperationNotFound
            | ModelDiscoveryServiceError::WorkerTerminated
            | ModelDiscoveryServiceError::OperationRevisionConflict { .. }
            | ModelDiscoveryServiceError::ConfigurationRevisionChanged
            | ModelDiscoveryServiceError::ModelConfigMutationInProgress => {
                ("internal.unexpected", ModelFailureCategory::Unknown)
            }
        };
        record.failure_id = Some(self.store_discovery_failure(
            &record,
            issue_code,
            category,
            None,
            None,
            record.raw_response_artifact_id,
        )?);
        self.finish_discovery_failed(record, Vec::new())
    }

    fn model_discovery_error_cancellation_reason(
        &self,
        record: &ModelDiscoveryRunRecord,
        error: &ModelDiscoveryServiceError,
    ) -> Option<aura_model_domain::ModelOperationCancellationReason> {
        let snapshot = self.model_config.snapshot().ok()?;
        let Some(endpoint) = snapshot.registry.endpoints.get(&record.endpoint_id) else {
            return Some(aura_model_domain::ModelOperationCancellationReason::TargetDeleted);
        };
        if snapshot.revision != record.source_models_revision.value()
            || endpoint.revision != record.endpoint_revision_at_start
            || endpoint_fingerprint(endpoint).ok().as_ref() != Some(&record.endpoint_fingerprint)
            || matches!(
                error,
                ModelDiscoveryServiceError::ConfigurationRevisionChanged
                    | ModelDiscoveryServiceError::ModelConfigMutationInProgress
            )
        {
            return Some(aura_model_domain::ModelOperationCancellationReason::ConfigurationChanged);
        }
        None
    }

    fn finish_or_partial_discovery_failure(
        &self,
        identity: &UserConfigurationOperationIdentity,
        correlation_id: CorrelationId,
        mut record: ModelDiscoveryRunRecord,
        mut projection_changes: Vec<aura_projection_contract::ProjectionChange>,
        model_ids: Vec<String>,
        attempt: TransportAttemptRecord,
    ) -> Result<ModelDiscoveryCommandCompletion, ModelDiscoveryServiceError> {
        if model_ids.is_empty() {
            return self.finish_discovery_failed_with_attempt(record, projection_changes, attempt);
        }
        let merge = match self.merge_discovery(
            identity.command_invocation_id(),
            correlation_id,
            &record,
            &model_ids,
        ) {
            Ok(merge) => merge,
            Err(ModelDiscoveryServiceError::ConfigurationRevisionChanged) => {
                return self.finish_discovery_failed_with_attempt(
                    record,
                    projection_changes,
                    attempt,
                );
            }
            Err(error) => return Err(error),
        };
        projection_changes.extend(merge.projection_changes);
        let expected = record.revision;
        record.revision = next_revision(expected)?;
        record.state = ModelDiscoveryState::Completed;
        record.stage = Some(ModelDiscoveryStage::Completed);
        record.outcome = Some(ModelDiscoveryOutcome::Partial);
        record.result = Some(merge.commit.result);
        record.finished_at = Some(UtcTimestamp::now());
        let terminal = self.persistence.transition_model_discovery_run_with_transport_attempt(
            ModelDiscoveryTransportTransition {
                correlation_id: record.correlation_id,
                expected_revision: expected,
                next: &record,
                terminal_event: Some(ModelDiscoveryTerminalEventContext::Completed {
                    models_revision: merge.commit.models_revision,
                }),
                attempt: &attempt,
                cancelled_by: None,
                projection_drafts: self.discovery_endpoint_projection_drafts(&record)?,
            },
        )?;
        projection_changes.extend(terminal.projection_changes);
        Ok(ModelDiscoveryCommandCompletion { projection_changes })
    }

    fn finish_discovery_cancelled(
        &self,
        record: ModelDiscoveryRunRecord,
        projection_changes: Vec<aura_projection_contract::ProjectionChange>,
        reason: aura_model_domain::ModelOperationCancellationReason,
    ) -> Result<ModelDiscoveryCommandCompletion, ModelDiscoveryServiceError> {
        if let Some(current) = self.persistence.model_discovery_run(record.id)? {
            if current.state.is_terminal() {
                return Ok(ModelDiscoveryCommandCompletion { projection_changes });
            }
            return self.commit_discovery_cancelled(current, projection_changes, reason);
        }
        self.commit_discovery_cancelled(record, projection_changes, reason)
    }

    fn finish_discovery_cancelled_with_attempt(
        &self,
        record: ModelDiscoveryRunRecord,
        projection_changes: Vec<aura_projection_contract::ProjectionChange>,
        reason: aura_model_domain::ModelOperationCancellationReason,
        attempt: TransportAttemptRecord,
    ) -> Result<ModelDiscoveryCommandCompletion, ModelDiscoveryServiceError> {
        if let Some(current) = self.persistence.model_discovery_run(record.id)? {
            if current.state.is_terminal() {
                return Ok(ModelDiscoveryCommandCompletion { projection_changes });
            }
            return self.commit_discovery_cancelled_with_attempt(
                current,
                projection_changes,
                reason,
                attempt,
            );
        }
        self.commit_discovery_cancelled_with_attempt(record, projection_changes, reason, attempt)
    }

    fn commit_discovery_cancelled_with_attempt(
        &self,
        mut record: ModelDiscoveryRunRecord,
        mut projection_changes: Vec<aura_projection_contract::ProjectionChange>,
        reason: aura_model_domain::ModelOperationCancellationReason,
        attempt: TransportAttemptRecord,
    ) -> Result<ModelDiscoveryCommandCompletion, ModelDiscoveryServiceError> {
        let expected = record.revision;
        record.revision = next_revision(expected)?;
        record.state = ModelDiscoveryState::Cancelled;
        record.outcome = None;
        record.cancellation_reason = Some(reason);
        record.result = None;
        record.failure_id = None;
        record.provider_error = None;
        record.finished_at = Some(UtcTimestamp::now());
        let terminal = self.persistence.transition_model_discovery_run_with_transport_attempt(
            ModelDiscoveryTransportTransition {
                correlation_id: record.correlation_id,
                expected_revision: expected,
                next: &record,
                terminal_event: Some(ModelDiscoveryTerminalEventContext::Cancelled {
                    cancelled_by: reason,
                }),
                attempt: &attempt,
                cancelled_by: Some(transport_cancellation_source(reason)),
                projection_drafts: self.discovery_endpoint_projection_drafts(&record)?,
            },
        )?;
        projection_changes.extend(terminal.projection_changes);
        Ok(ModelDiscoveryCommandCompletion { projection_changes })
    }

    fn commit_discovery_cancelled(
        &self,
        mut record: ModelDiscoveryRunRecord,
        mut projection_changes: Vec<aura_projection_contract::ProjectionChange>,
        reason: aura_model_domain::ModelOperationCancellationReason,
    ) -> Result<ModelDiscoveryCommandCompletion, ModelDiscoveryServiceError> {
        let expected = record.revision;
        record.revision = next_revision(expected)?;
        record.state = ModelDiscoveryState::Cancelled;
        record.outcome = None;
        record.cancellation_reason = Some(reason);
        record.result = None;
        record.failure_id = None;
        record.provider_error = None;
        record.finished_at = Some(UtcTimestamp::now());
        let terminal = self.persistence.transition_model_discovery_run(
            expected,
            &record,
            Some(ModelDiscoveryTerminalEventContext::Cancelled { cancelled_by: reason }),
            self.discovery_endpoint_projection_drafts(&record)?,
        )?;
        projection_changes.extend(terminal.projection_changes);
        Ok(ModelDiscoveryCommandCompletion { projection_changes })
    }

    pub(crate) async fn cancel_model_discovery(
        &self,
        request: aura_model_domain::ModelDiscoveryCancelRequest,
    ) -> Result<
        (
            aura_model_domain::ModelDiscoveryCancelResult,
            Vec<aura_projection_contract::ProjectionChange>,
        ),
        ModelDiscoveryServiceError,
    > {
        let record = self
            .persistence
            .model_discovery_run(request.model_discovery_run_id)?
            .ok_or(ModelDiscoveryServiceError::OperationNotFound)?;
        if record.revision.value() != request.expected_operational_revision.value() {
            return Err(ModelDiscoveryServiceError::OperationRevisionConflict {
                expected: request.expected_operational_revision.value(),
                actual: record.revision.value(),
            });
        }
        if record.state.is_terminal() {
            if let Some(control) = self.operation_cancellations.discovery(record.id) {
                control.wait_for_terminal_projection().await;
            }
            return Ok((
                aura_model_domain::ModelDiscoveryCancelResult {
                    model_discovery_run_id: record.id,
                    operational_revision: record.revision,
                    state: record.state,
                },
                Vec::new(),
            ));
        }
        let Some(control) = self.operation_cancellations.discovery(record.id) else {
            let current = self
                .persistence
                .model_discovery_run(record.id)?
                .ok_or(ModelDiscoveryServiceError::OperationNotFound)?;
            if current.state.is_terminal() {
                return Ok((
                    aura_model_domain::ModelDiscoveryCancelResult {
                        model_discovery_run_id: current.id,
                        operational_revision: current.revision,
                        state: current.state,
                    },
                    Vec::new(),
                ));
            }
            return Err(ModelDiscoveryServiceError::WorkerTerminated);
        };
        control.cancel(aura_model_domain::ModelOperationCancellationReason::UserRequested);

        if record.state == ModelDiscoveryState::Queued {
            // Strict Queued CAS: once an Attempt has advanced the Operation to
            // Running, the Worker must persist the Attempt terminal event
            // before committing the Operation terminal event.
            match self.commit_discovery_cancelled(
                record,
                Vec::new(),
                aura_model_domain::ModelOperationCancellationReason::UserRequested,
            ) {
                Ok(completion) => {
                    self.persistence
                        .publish_model_projection_changes(&completion.projection_changes)?;
                    control.mark_terminal_projection_published();
                }
                Err(ModelDiscoveryServiceError::Persistence(
                    PersistenceError::ModelDiscoveryRunStateConflict,
                )) => {}
                Err(error) => return Err(error),
            }
        }

        control.wait_for_terminal_projection().await;
        let record = self
            .persistence
            .model_discovery_run(request.model_discovery_run_id)?
            .ok_or(ModelDiscoveryServiceError::OperationNotFound)?;
        if !record.state.is_terminal() {
            return Err(ModelDiscoveryServiceError::WorkerTerminated);
        }
        Ok((
            aura_model_domain::ModelDiscoveryCancelResult {
                model_discovery_run_id: record.id,
                operational_revision: record.revision,
                state: record.state,
            },
            Vec::new(),
        ))
    }

    fn discovery_view(
        &self,
        record: &ModelDiscoveryRunRecord,
    ) -> Result<ModelDiscoveryRunView, ModelDiscoveryServiceError> {
        self.projections
            .model_discovery_run_view(record)
            .map_err(|_| ModelDiscoveryServiceError::MissingProjection)
    }

    fn merge_discovery(
        &self,
        command_invocation_id: CommandInvocationId,
        correlation_id: CorrelationId,
        record: &ModelDiscoveryRunRecord,
        model_ids: &[String],
    ) -> Result<DiscoveryMergeCompletion, ModelDiscoveryServiceError> {
        ensure_model_config_is_stable(self)?;
        let current = self
            .model_config
            .snapshot()
            .map_err(|_| ModelDiscoveryServiceError::ConfigurationRevisionChanged)?;
        let endpoint = current
            .registry
            .endpoints
            .get(&record.endpoint_id)
            .ok_or(ModelDiscoveryServiceError::ConfigurationRevisionChanged)?;
        if endpoint_fingerprint(endpoint)
            .map_err(|_| ModelDiscoveryServiceError::ConfigurationRevisionChanged)?
            != record.endpoint_fingerprint
        {
            return Err(ModelDiscoveryServiceError::ConfigurationRevisionChanged);
        }
        let catalog = PersistenceCatalogCache::new(self.persistence.clone())
            .load()
            .ok()
            .and_then(|cached| cached.snapshot);
        let completion =
            self.config_journal.execute(command_invocation_id, correlation_id, |journal_id| {
                let prepared = self.model_config.prepare_update(
                    journal_id.to_string(),
                    current.revision,
                    |registry| {
                        merge_discovery_results(
                            registry,
                            record.endpoint_id,
                            model_ids,
                            catalog.as_ref(),
                            UtcTimestamp::now(),
                        )
                    },
                )?;
                let commit = DiscoveryMergeCommit {
                    result: prepared.value.clone(),
                    models_revision: prepared.candidate_revision,
                };
                if !prepared.changed {
                    return Ok(ModelConfigCommandPlan::NoChange {
                        success_result: serde_json::to_value(commit)?,
                    });
                }
                let snapshot = prepared_snapshot(&prepared);
                let previous_endpoint = prepared
                    .previous
                    .registry
                    .endpoints
                    .get(&record.endpoint_id)
                    .ok_or(ModelConfigError::EndpointIdNotFound(record.endpoint_id))?;
                let endpoint = prepared
                    .candidate_registry
                    .endpoints
                    .get(&record.endpoint_id)
                    .ok_or(ModelConfigError::EndpointIdNotFound(record.endpoint_id))?;
                let credential_status = self.projections.active_credential_status(endpoint)?;
                let mut events = Vec::new();
                if previous_endpoint.revision != endpoint.revision {
                    events.push(user_event(
                        "ModelEndpointUpdated",
                        correlation_id,
                        Some(journal_id.to_string()),
                        &ModelEndpointUpdatedPayload {
                            endpoint: endpoint.clone(),
                            credential_status,
                            previous_endpoint_fingerprint: endpoint_fingerprint(previous_endpoint)
                                .map_err(ModelConfigError::from)?,
                            endpoint_fingerprint: endpoint_fingerprint(endpoint)
                                .map_err(ModelConfigError::from)?,
                            credential_binding_mismatch: false,
                            changed_fields: endpoint_changed_fields(previous_endpoint, endpoint),
                            models_revision: prepared.candidate_revision,
                        },
                    )?);
                }
                let mut returned = prepared.value.returned_profiles.clone();
                returned.sort_by_key(|item| item.model_profile_id);
                let mut changed_profile_ids = BTreeSet::new();
                for item in returned {
                    let profile = prepared
                        .candidate_registry
                        .models
                        .get(&item.model_profile_id)
                        .ok_or(ModelConfigError::ProfileNotFound(item.model_profile_id))?;
                    if item.created {
                        changed_profile_ids.insert(profile.id);
                        events.push(user_event(
                            "ModelProfileCreated",
                            correlation_id,
                            Some(journal_id.to_string()),
                            &ModelProfileCreatedPayload {
                                provider_id: endpoint.provider_id,
                                endpoint_id: endpoint.id,
                                profile: profile.clone(),
                                models_revision: prepared.candidate_revision,
                            },
                        )?);
                    } else if let Some(previous) =
                        prepared.previous.registry.models.get(&item.model_profile_id)
                        && previous.revision != profile.revision
                    {
                        changed_profile_ids.insert(profile.id);
                        events.push(user_event(
                            "ModelProfileUpdated",
                            correlation_id,
                            Some(journal_id.to_string()),
                            &ModelProfileUpdatedPayload {
                                provider_id: endpoint.provider_id,
                                endpoint_id: endpoint.id,
                                profile: profile.clone(),
                                changed_fields: profile_changed_fields(previous, profile),
                                models_revision: prepared.candidate_revision,
                            },
                        )?);
                    }
                }
                let mut projection_drafts = vec![
                    ModelProjectionDraft::ModelSettings(
                        self.projections.model_settings(&snapshot, &BTreeMap::new())?,
                    ),
                    ModelProjectionDraft::EndpointDetail(Box::new(
                        self.projections.endpoint_detail(
                            &snapshot,
                            endpoint.id,
                            &BTreeMap::new(),
                        )?,
                    )),
                ];
                for profile_id in changed_profile_ids {
                    projection_drafts.push(ModelProjectionDraft::ModelProfile(Box::new(
                        self.projections.model_profile(&snapshot, profile_id, &BTreeMap::new())?,
                    )));
                }
                let prepared_bundle =
                    prepared.prepared_bundle.ok_or(ModelConfigError::InternalInvariant(
                        "changed Discovery merge lacks a prepared model bundle",
                    ))?;
                Ok(ModelConfigCommandPlan::Publish {
                    prepared_bundle: Box::new(prepared_bundle),
                    expected_models_revision: Revision::new(current.revision),
                    reserved_models_revision: Some(prepared.candidate_revision),
                    expected_aura_revision: None,
                    reserved_aura_revision: None,
                    credential_slot_creations: Vec::new(),
                    session_binding_clears: Vec::new(),
                    metadata_events: events,
                    projection_drafts,
                    success_result: serde_json::to_value(commit)?,
                })
            })?;
        let commit: DiscoveryMergeCommit = serde_json::from_value(completion.success_result)
            .map_err(ModelConfigJournalError::from)?;
        Ok(DiscoveryMergeCompletion { commit, projection_changes: completion.projection_changes })
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

struct DiscoveryMergeCompletion {
    commit: DiscoveryMergeCommit,
    projection_changes: Vec<aura_projection_contract::ProjectionChange>,
}

fn compile_discovery_request(
    endpoint: &ModelEndpointConfig,
    operation_context: &TransportOperationContext,
    request_url: &Url,
) -> Result<ProviderRequestPlan, aura_model_transport::ModelAdapterError> {
    build_model_discovery_request_plan(ModelDiscoveryCompilationInput {
        endpoint,
        resolved_request_url: request_url.as_str(),
        operation_context,
    })
}

fn transition_discovery_stage(
    services: &P05Services,
    record: &mut ModelDiscoveryRunRecord,
    stage: ModelDiscoveryStage,
) -> Result<(), ModelDiscoveryServiceError> {
    let expected = record.revision;
    record.revision = next_revision(expected)?;
    record.state = ModelDiscoveryState::Running;
    record.stage = Some(stage);
    let commit = services.persistence.transition_model_discovery_run(
        expected,
        record,
        None,
        services.discovery_endpoint_projection_drafts(record)?,
    )?;
    services.persistence.publish_model_projection_changes(&commit.projection_changes)?;
    Ok(())
}

fn validate_discovery_identity(
    prepared: &PreparedModelDiscovery,
    identity: &UserConfigurationOperationIdentity,
) -> Result<(), ModelDiscoveryServiceError> {
    if identity.operation_kind() != ConfigurationOperationKind::ModelDiscovery
        || identity.target_endpoint_id() != prepared.endpoint.id
        || identity.config_revision() != prepared.source_models_revision
    {
        return Err(ModelDiscoveryServiceError::ConfigurationRevisionChanged);
    }
    Ok(())
}

fn new_discovery_record(
    prepared: &PreparedModelDiscovery,
    correlation_id: CorrelationId,
) -> ModelDiscoveryRunRecord {
    ModelDiscoveryRunRecord {
        id: prepared.id,
        correlation_id,
        endpoint_id: prepared.endpoint.id,
        source_models_revision: prepared.source_models_revision,
        endpoint_revision_at_start: prepared.endpoint.revision,
        endpoint_fingerprint: prepared.endpoint_fingerprint.clone(),
        revision: Revision::INITIAL,
        state: ModelDiscoveryState::Queued,
        stage: None,
        outcome: None,
        cancellation_reason: None,
        transport_attempt_ids: Vec::new(),
        result: None,
        failure_id: None,
        provider_error: None,
        raw_response_artifact_id: None,
        started_at: UtcTimestamp::now(),
        finished_at: None,
    }
}

fn ensure_model_config_is_stable(services: &P05Services) -> Result<(), ModelDiscoveryServiceError> {
    if services
        .persistence
        .active_mutation_journals()?
        .iter()
        .any(|journal| journal.journal_kind == MODEL_CONFIG_MUTATION_JOURNAL_KIND)
    {
        return Err(ModelDiscoveryServiceError::ModelConfigMutationInProgress);
    }
    Ok(())
}

fn next_revision(revision: Revision) -> Result<Revision, ModelDiscoveryServiceError> {
    revision
        .value()
        .checked_add(1)
        .map(Revision::new)
        .ok_or(ModelDiscoveryServiceError::RevisionOverflow)
}

fn catalog_issue_code(error: &CatalogError) -> &'static str {
    match error {
        CatalogError::DiscoveryLimitExceeded => "discovery.limit_exceeded",
        _ => "discovery.invalid_response",
    }
}

const fn provider_status_issue(status: u16) -> &'static str {
    if status == 401 || status == 403 {
        "authentication.rejected"
    } else if status == 429 {
        "provider.rate_limited"
    } else if status >= 500 {
        "provider.server_error"
    } else {
        "provider.request_rejected"
    }
}

const fn provider_status_category(status: u16) -> ModelFailureCategory {
    if status == 401 || status == 403 {
        ModelFailureCategory::Authentication
    } else if status == 429 {
        ModelFailureCategory::RateLimit
    } else {
        ModelFailureCategory::InvalidRequest
    }
}

pub(crate) fn model_discovery_identity(
    command_invocation_id: CommandInvocationId,
    correlation_id: CorrelationId,
    channel: AuthenticatedConfigurationChannelIdentity,
    prepared: &PreparedModelDiscovery,
) -> UserConfigurationOperationIdentity {
    UserConfigurationOperationIdentity::from_authenticated_settings_command(
        command_invocation_id,
        correlation_id,
        channel,
        ConfigurationOperationKind::ModelDiscovery,
        prepared.endpoint.id,
        prepared.source_models_revision,
        UtcTimestamp::now(),
    )
}

#[cfg(test)]
mod tests {
    use super::{provider_status_category, provider_status_issue};
    use aura_model_domain::ModelFailureCategory;

    #[test]
    fn actual_non_success_http_status_is_failed_not_unsupported() {
        assert_eq!(provider_status_issue(404), "provider.request_rejected");
        assert_eq!(provider_status_issue(429), "provider.rate_limited");
        assert_eq!(provider_status_issue(503), "provider.server_error");
        assert_eq!(provider_status_category(401), ModelFailureCategory::Authentication);
        assert_eq!(provider_status_category(429), ModelFailureCategory::RateLimit);
    }
}
