use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use base64::Engine as _;

use aura_domain_common::{
    CommandInvocationId, CorrelationId, CredentialId, ErrorCategory, ErrorCode, EventId,
    ModelAvailabilityTestId, ModelDiscoveryRunId, NormalizedIssueCode, Revision, UtcTimestamp,
};
use aura_model_config::{
    ModelConfigError, ModelConfigValidationError, UpdateModelProfileInput, add_manual_model,
    apply_catalog_update, create_endpoint, create_model_from_catalog, create_provider,
    credential_binding_fingerprint, credential_ref_for_credential, endpoint_fingerprint,
    model_credential_final_target, normalize_base_url, rename_provider,
    reset_endpoint_compatibility, set_model_subagent_availability,
    set_provider_current_models_subagent_availability, update_endpoint, update_model_profile,
};
use aura_model_domain::{
    DefaultModelChangedPayload, DefaultModelMutationResult, DefaultModelSelection,
    EndpointAuthScheme, EndpointCompatibilitySnapshot, EndpointCredentialState,
    EndpointCredentialStatus, ModelAvailabilityTestCancelRequest,
    ModelAvailabilityTestStartRequest, ModelCatalogEnsureFreshRequest, ModelCatalogGetRequest,
    ModelCatalogProviderGetRequest, ModelCatalogRefreshRequest, ModelCompatibilityGetRequest,
    ModelCompatibilityResetPayload, ModelCompatibilityResetRequest, ModelCompatibilityResetResult,
    ModelCredentialSlot, ModelDefaultSetRequest, ModelDeletionMutationResult,
    ModelDiscoveryCancelRequest, ModelDiscoveryStartRequest, ModelEndpointCreateRequest,
    ModelEndpointCreatedPayload, ModelEndpointDeleteRequest, ModelEndpointDeletedPayload,
    ModelEndpointMutationResult, ModelEndpointUpdateRequest, ModelEndpointUpdatedPayload,
    ModelExecutionReadinessView, ModelProfileApplyCatalogUpdateRequest,
    ModelProfileCreateFromCatalogRequest, ModelProfileCreateManualRequest,
    ModelProfileCreatedPayload, ModelProfileDeleteRequest, ModelProfileDeletedPayload,
    ModelProfileMutationResult, ModelProfileSetSubagentAvailabilityRequest,
    ModelProfileUpdateRequest, ModelProfileUpdatedPayload, ModelProviderCreateRequest,
    ModelProviderCreatedPayload, ModelProviderDeleteRequest, ModelProviderDeletedPayload,
    ModelProviderMutationResult, ModelProviderRenameRequest, ModelProviderRenamedPayload,
    ModelProviderSetAllCurrentModelsSubagentAvailabilityRequest,
    ModelSubagentAvailabilityChangedPayload, ProviderBulkAvailabilityResult,
    ProviderCatalogProjection, ProviderIconReadRequest, ProviderIconReadResult,
    SessionModelBindingChangedPayload, SessionModelBindingMutationResult,
    SessionModelBindingSetRequest, model_ipc_method,
};
use aura_network_control::{
    AuthenticatedConfigurationChannelIdentity, UserConfigurationOperationIdentity,
};
use aura_permission_domain::NormalizedOrigin;
use aura_persistence::{
    CommitSessionModelBinding, CreateModelCredentialSlot, EventActor,
    ModelCredentialDeletionReason, ModelCredentialRecordState, ModelCredentialStorageBinding,
    ModelProjectionDraft, PersistentEventDraft, PreparedSessionModelBindingClear,
};
use aura_projection_contract::{ProjectionChange, ProjectionKind, ProjectionScope};
use aura_runtime_contract::{IpcErrorEnvelope, IpcRequestTerminal};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{
    P05OperationControl, P05Services,
    availability::{
        ModelAvailabilityServiceError, PreparedModelAvailabilityTest, model_availability_identity,
    },
    catalog::{
        P05CatalogError, project_catalog, project_catalog_provider_page, spawn_catalog_refresh,
    },
    config_journal::{
        ModelConfigCommandPlan, ModelConfigCredentialDeleteCommandPlan, ModelConfigJournalError,
    },
    discovery::{ModelDiscoveryServiceError, PreparedModelDiscovery, model_discovery_identity},
    projection::P05ProjectionError,
};

mod connections;
mod context;
mod credential;
mod window_trial;

pub struct P05CommandDispatch {
    pub value: Value,
    pub projection_changes: Vec<ProjectionChange>,
}

impl P05Services {
    #[must_use]
    pub fn handles_main_command(method_name: &str) -> bool {
        aura_ipc::routes::core_route(method_name).is_some_and(|r| {
            r.owner == aura_ipc::routes::RouteOwner::Model
                && r.auth == aura_ipc::routes::RouteAuth::AuthenticatedMain
        })
    }

    pub async fn dispatch_main_command(
        &self,
        method_name: &str,
        params: &Value,
        command_invocation_id: Option<CommandInvocationId>,
        correlation_id: CorrelationId,
        authenticated_channel: AuthenticatedConfigurationChannelIdentity,
    ) -> Option<Result<P05CommandDispatch, IpcErrorEnvelope>> {
        if !Self::handles_main_command(method_name) {
            return None;
        }
        Some(
            self.dispatch_known_main_command(
                method_name,
                params,
                command_invocation_id,
                correlation_id,
                authenticated_channel,
            )
            .await,
        )
    }

    async fn dispatch_known_main_command(
        &self,
        method_name: &str,
        params: &Value,
        command_invocation_id: Option<CommandInvocationId>,
        correlation_id: CorrelationId,
        authenticated_channel: AuthenticatedConfigurationChannelIdentity,
    ) -> Result<P05CommandDispatch, IpcErrorEnvelope> {
        match method_name {
            model_ipc_method::PROVIDER_ICON_READ => {
                let request: ProviderIconReadRequest = decode(params, correlation_id)?;
                let bytes = self
                    .provider_icons
                    .read_png(request.artifact_id, &request.content_hash)
                    .map_err(|_| provider_icon_error(correlation_id))?;
                let result = ProviderIconReadResult {
                    content_hash: request.content_hash,
                    byte_length: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                    png_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
                };
                Ok(P05CommandDispatch {
                    value: encode(result, correlation_id)?,
                    projection_changes: Vec::new(),
                })
            }
            model_ipc_method::CATALOG_GET => {
                let _: ModelCatalogGetRequest = decode(params, correlation_id)?;
                self.provider_icons.schedule_missing();
                let projection = self.current_provider_catalog_projection(correlation_id)?;
                Ok(P05CommandDispatch {
                    value: encode(projection, correlation_id)?,
                    projection_changes: Vec::new(),
                })
            }
            model_ipc_method::CATALOG_PROVIDER_GET => {
                let request: ModelCatalogProviderGetRequest = decode(params, correlation_id)?;
                self.provider_icons.schedule_missing();
                let status = self
                    .provider_catalog
                    .status()
                    .map_err(|error| catalog_error(error.into(), correlation_id))?;
                let snapshot = self
                    .model_config
                    .snapshot()
                    .map_err(|error| journal_error(error.into(), correlation_id))?;
                let page =
                    project_catalog_provider_page(&status, &snapshot, &request, &self.persistence)
                        .map_err(|error| catalog_error(error, correlation_id))?;
                Ok(P05CommandDispatch {
                    value: encode(page, correlation_id)?,
                    projection_changes: Vec::new(),
                })
            }
            model_ipc_method::CATALOG_ENSURE_FRESH | model_ipc_method::CATALOG_REFRESH => {
                let force = method_name == model_ipc_method::CATALOG_REFRESH;
                if force {
                    let _: ModelCatalogRefreshRequest = decode(params, correlation_id)?;
                    let _command_invocation_id =
                        require_command_id(command_invocation_id, correlation_id)?;
                } else {
                    let _: ModelCatalogEnsureFreshRequest = decode(params, correlation_id)?;
                }
                let should_begin =
                    force || self.provider_catalog.needs_refresh(UtcTimestamp::now());
                // The occupancy's one owner is taken first and held across every fallible step
                // below. Each of those steps returns with `?`: dropping the handle on any of
                // those exits is what gives the slot back, so a failed manual refresh cannot
                // leave the catalog "refreshing" until the process ends.
                let ownership = if should_begin {
                    self.provider_catalog
                        .begin_refresh()
                        .map_err(|error| catalog_error(error.into(), correlation_id))?
                } else {
                    None
                };
                let started = ownership.is_some();
                let status = self
                    .provider_catalog
                    .status()
                    .map_err(|error| catalog_error(error.into(), correlation_id))?;
                let snapshot = self
                    .model_config
                    .snapshot()
                    .map_err(|error| journal_error(error.into(), correlation_id))?;
                let projection = project_catalog(&status, &snapshot, &self.persistence)
                    .map_err(|error| catalog_error(error, correlation_id))?;
                let changes = if started {
                    self.persistence
                        .commit_model_projection_snapshots(vec![
                            ModelProjectionDraft::ProviderCatalog(projection.clone()),
                        ])
                        .map_err(|error| journal_error(error.into(), correlation_id))?
                } else {
                    Vec::new()
                };
                if let Some(ownership) = ownership {
                    drop(spawn_catalog_refresh(
                        self.sync.clone(),
                        Arc::clone(&self.provider_catalog),
                        Arc::clone(&self.model_config),
                        Arc::clone(&self.persistence),
                        self.provider_icons.clone(),
                        ownership,
                    ));
                }
                self.provider_icons.schedule_missing();
                let projection = self.current_provider_catalog_projection(correlation_id)?;
                Ok(P05CommandDispatch {
                    value: encode(projection, correlation_id)?,
                    projection_changes: changes,
                })
            }
            model_ipc_method::SETTINGS_GET_SNAPSHOT => {
                let _: aura_model_domain::ModelSettingsGetSnapshotRequest =
                    decode(params, correlation_id)?;
                let value = encode(
                    self.projections
                        .current_model_settings()
                        .map_err(|error| projection_error(error, correlation_id))?,
                    correlation_id,
                )?;
                Ok(P05CommandDispatch { value, projection_changes: Vec::new() })
            }
            model_ipc_method::COMPATIBILITY_GET => {
                let request: ModelCompatibilityGetRequest = decode(params, correlation_id)?;
                let snapshot = self
                    .model_config
                    .snapshot()
                    .map_err(|error| journal_error(error.into(), correlation_id))?;
                let endpoint =
                    snapshot.registry.endpoints.get(&request.endpoint_id).ok_or_else(|| {
                        journal_error(
                            ModelConfigError::EndpointIdNotFound(request.endpoint_id).into(),
                            correlation_id,
                        )
                    })?;
                let value = encode(
                    EndpointCompatibilitySnapshot {
                        endpoint_id: endpoint.id,
                        revision: Revision::new(snapshot.revision),
                        compatibility: endpoint.compatibility.clone(),
                    },
                    correlation_id,
                )?;
                Ok(P05CommandDispatch { value, projection_changes: Vec::new() })
            }
            model_ipc_method::PROFILE_TEST_AVAILABILITY => {
                let request: ModelAvailabilityTestStartRequest = decode(params, correlation_id)?;
                let command_invocation_id =
                    require_command_id(command_invocation_id, correlation_id)?;
                let prepared = self
                    .prepare_model_availability_test(request)
                    .map_err(|error| availability_error(error, correlation_id))?;
                let identity = model_availability_identity(
                    command_invocation_id,
                    correlation_id,
                    authenticated_channel,
                    &prepared,
                );
                let queued = self
                    .queue_model_availability_test(&prepared, &identity, correlation_id)
                    .map_err(|error| availability_error(error, correlation_id))?;
                if queued.newly_queued {
                    let operation_id = queued.accepted.model_availability_test_id;
                    let control = self.operation_cancellations.register_availability(operation_id);
                    self.spawn_model_availability_supervisor(
                        prepared,
                        identity,
                        correlation_id,
                        operation_id,
                        control,
                    );
                }
                Ok(P05CommandDispatch {
                    value: encode(queued.accepted, correlation_id)?,
                    projection_changes: queued.projection_changes,
                })
            }
            model_ipc_method::AVAILABILITY_TEST_CANCEL => {
                let request: ModelAvailabilityTestCancelRequest = decode(params, correlation_id)?;
                let _command_invocation_id =
                    require_command_id(command_invocation_id, correlation_id)?;
                let (result, projection_changes) = self
                    .cancel_model_availability_test(request)
                    .await
                    .map_err(|error| availability_error(error, correlation_id))?;
                Ok(P05CommandDispatch {
                    value: encode(result, correlation_id)?,
                    projection_changes,
                })
            }
            model_ipc_method::DISCOVERY_START => {
                let request: ModelDiscoveryStartRequest = decode(params, correlation_id)?;
                let command_invocation_id =
                    require_command_id(command_invocation_id, correlation_id)?;
                let prepared = self
                    .prepare_model_discovery(request)
                    .map_err(|error| discovery_error(error, correlation_id))?;
                let identity = model_discovery_identity(
                    command_invocation_id,
                    correlation_id,
                    authenticated_channel,
                    &prepared,
                );
                let queued = self
                    .queue_model_discovery(&prepared, &identity, correlation_id)
                    .map_err(|error| discovery_error(error, correlation_id))?;
                if queued.newly_queued {
                    let operation_id = queued.accepted.model_discovery_run_id;
                    let control = self.operation_cancellations.register_discovery(operation_id);
                    self.spawn_model_discovery_supervisor(
                        prepared,
                        identity,
                        correlation_id,
                        operation_id,
                        control,
                    );
                }
                Ok(P05CommandDispatch {
                    value: encode(queued.accepted, correlation_id)?,
                    projection_changes: queued.projection_changes,
                })
            }
            model_ipc_method::DISCOVERY_CANCEL => {
                let request: ModelDiscoveryCancelRequest = decode(params, correlation_id)?;
                let _command_invocation_id =
                    require_command_id(command_invocation_id, correlation_id)?;
                let (result, projection_changes) = self
                    .cancel_model_discovery(request)
                    .await
                    .map_err(|error| discovery_error(error, correlation_id))?;
                Ok(P05CommandDispatch {
                    value: encode(result, correlation_id)?,
                    projection_changes,
                })
            }
            model_ipc_method::PROVIDER_CREATE => {
                let request: ModelProviderCreateRequest = decode(params, correlation_id)?;
                let command_invocation_id =
                    require_command_id(command_invocation_id, correlation_id)?;
                let completion = self
                    .config_journal
                    .execute(command_invocation_id, correlation_id, |journal_id| {
                        let prepared = self.model_config.prepare_update(
                            journal_id.to_string(),
                            request.expected_revision.value(),
                            |registry| {
                                create_provider(registry, request.input, UtcTimestamp::now())
                            },
                        )?;
                        let provider = prepared
                            .candidate_registry
                            .providers
                            .get(&prepared.value)
                            .cloned()
                            .ok_or(ModelConfigError::InternalInvariant(
                                "created Provider is absent from the candidate registry",
                            ))?;
                        let result = ModelProviderMutationResult {
                            changed: prepared.changed,
                            revision: prepared.candidate_revision,
                            value: provider.clone(),
                        };
                        if !prepared.changed {
                            return Ok(ModelConfigCommandPlan::NoChange {
                                success_result: serde_json::to_value(result)?,
                            });
                        }
                        let model_settings = self
                            .projections
                            .model_settings(&prepared_snapshot(&prepared), &BTreeMap::new())
                            .map_err(ModelConfigJournalError::from)?;
                        let prepared_bundle =
                            prepared.prepared_bundle.ok_or(ModelConfigError::InternalInvariant(
                                "changed Provider mutation lacks a prepared bundle",
                            ))?;
                        Ok(ModelConfigCommandPlan::Publish {
                            prepared_bundle: Box::new(prepared_bundle),
                            expected_models_revision: Revision::new(
                                request.expected_revision.value(),
                            ),
                            reserved_models_revision: Some(prepared.candidate_revision),
                            expected_aura_revision: None,
                            reserved_aura_revision: None,
                            credential_slot_creations: Vec::new(),
                            session_binding_clears: Vec::new(),
                            metadata_events: vec![user_event(
                                "ModelProviderCreated",
                                correlation_id,
                                None,
                                &ModelProviderCreatedPayload {
                                    provider,
                                    models_revision: prepared.candidate_revision,
                                },
                            )?],
                            projection_drafts: vec![ModelProjectionDraft::ModelSettings(
                                model_settings,
                            )],
                            success_result: serde_json::to_value(result)?,
                        })
                    })
                    .map_err(|error| journal_error(error, correlation_id))?;
                Ok(P05CommandDispatch {
                    value: completion.success_result,
                    projection_changes: completion.projection_changes,
                })
            }
            model_ipc_method::PROVIDER_RENAME => {
                let request: ModelProviderRenameRequest = decode(params, correlation_id)?;
                let command_invocation_id =
                    require_command_id(command_invocation_id, correlation_id)?;
                let completion = self
                    .config_journal
                    .execute(command_invocation_id, correlation_id, |journal_id| {
                        let prepared = self.model_config.prepare_update(
                            journal_id.to_string(),
                            request.expected_revision.value(),
                            |registry| {
                                rename_provider(
                                    registry,
                                    request.provider_id,
                                    request.display_name,
                                    UtcTimestamp::now(),
                                )
                            },
                        )?;
                        let previous = prepared
                            .previous
                            .registry
                            .providers
                            .get(&request.provider_id)
                            .cloned()
                            .ok_or(ModelConfigError::ProviderNotFoundById(request.provider_id))?;
                        let provider = prepared
                            .candidate_registry
                            .providers
                            .get(&request.provider_id)
                            .cloned()
                            .ok_or(ModelConfigError::ProviderNotFoundById(request.provider_id))?;
                        let result = ModelProviderMutationResult {
                            changed: prepared.changed,
                            revision: prepared.candidate_revision,
                            value: provider.clone(),
                        };
                        if !prepared.changed {
                            return Ok(ModelConfigCommandPlan::NoChange {
                                success_result: serde_json::to_value(result)?,
                            });
                        }
                        let model_settings = self
                            .projections
                            .model_settings(&prepared_snapshot(&prepared), &BTreeMap::new())
                            .map_err(ModelConfigJournalError::from)?;
                        let prepared_bundle =
                            prepared.prepared_bundle.ok_or(ModelConfigError::InternalInvariant(
                                "changed Provider mutation lacks a prepared bundle",
                            ))?;
                        Ok(ModelConfigCommandPlan::Publish {
                            prepared_bundle: Box::new(prepared_bundle),
                            expected_models_revision: Revision::new(
                                request.expected_revision.value(),
                            ),
                            reserved_models_revision: Some(prepared.candidate_revision),
                            expected_aura_revision: None,
                            reserved_aura_revision: None,
                            credential_slot_creations: Vec::new(),
                            session_binding_clears: Vec::new(),
                            metadata_events: vec![user_event(
                                "ModelProviderRenamed",
                                correlation_id,
                                None,
                                &ModelProviderRenamedPayload {
                                    provider_id: request.provider_id,
                                    previous_display_name: previous.display_name,
                                    display_name: provider.display_name.clone(),
                                    models_revision: prepared.candidate_revision,
                                },
                            )?],
                            projection_drafts: vec![ModelProjectionDraft::ModelSettings(
                                model_settings,
                            )],
                            success_result: serde_json::to_value(result)?,
                        })
                    })
                    .map_err(|error| journal_error(error, correlation_id))?;
                Ok(P05CommandDispatch {
                    value: completion.success_result,
                    projection_changes: completion.projection_changes,
                })
            }
            model_ipc_method::PROVIDER_DELETE => {
                self.dispatch_delete_provider(params, command_invocation_id, correlation_id).await
            }
            model_ipc_method::ENDPOINT_CREATE => {
                let request: ModelEndpointCreateRequest = decode(params, correlation_id)?;
                let command_invocation_id =
                    require_command_id(command_invocation_id, correlation_id)?;
                let expected_revision = request.expected_revision;
                let input = request.into_input();
                let completion = self
                    .config_journal
                    .execute(command_invocation_id, correlation_id, |journal_id| {
                        let prepared = self.model_config.prepare_update(
                            journal_id.to_string(),
                            expected_revision.value(),
                            |registry| create_endpoint(registry, input, UtcTimestamp::now()),
                        )?;
                        let endpoint = prepared
                            .candidate_registry
                            .endpoints
                            .get(&prepared.value)
                            .cloned()
                            .ok_or(ModelConfigError::InternalInvariant(
                                "created Endpoint is absent from the candidate registry",
                            ))?;
                        let (credential_slot_creations, credential_status) =
                            self.prepare_endpoint_credential_slot(&endpoint)?;
                        let status_overrides = credential_status
                            .clone()
                            .map(|status| BTreeMap::from([(endpoint.id, status)]))
                            .unwrap_or_default();
                        let snapshot = prepared_snapshot(&prepared);
                        let model_settings = self
                            .projections
                            .model_settings(&snapshot, &status_overrides)
                            .map_err(ModelConfigJournalError::from)?;
                        let endpoint_detail = self
                            .projections
                            .endpoint_detail(&snapshot, endpoint.id, &status_overrides)
                            .map_err(ModelConfigJournalError::from)?;
                        let result = ModelEndpointMutationResult {
                            changed: prepared.changed,
                            revision: prepared.candidate_revision,
                            value: endpoint.clone(),
                        };
                        let prepared_bundle =
                            prepared.prepared_bundle.ok_or(ModelConfigError::InternalInvariant(
                                "created Endpoint mutation lacks a prepared bundle",
                            ))?;
                        Ok(ModelConfigCommandPlan::Publish {
                            prepared_bundle: Box::new(prepared_bundle),
                            expected_models_revision: Revision::new(expected_revision.value()),
                            reserved_models_revision: Some(prepared.candidate_revision),
                            expected_aura_revision: None,
                            reserved_aura_revision: None,
                            credential_slot_creations,
                            session_binding_clears: Vec::new(),
                            metadata_events: vec![user_event(
                                "ModelEndpointCreated",
                                correlation_id,
                                None,
                                &ModelEndpointCreatedPayload {
                                    endpoint,
                                    credential_status,
                                    models_revision: prepared.candidate_revision,
                                },
                            )?],
                            projection_drafts: vec![
                                ModelProjectionDraft::ModelSettings(model_settings),
                                ModelProjectionDraft::EndpointDetail(Box::new(endpoint_detail)),
                            ],
                            success_result: serde_json::to_value(result)?,
                        })
                    })
                    .map_err(|error| journal_error(error, correlation_id))?;
                Ok(P05CommandDispatch {
                    value: completion.success_result,
                    projection_changes: completion.projection_changes,
                })
            }
            model_ipc_method::ENDPOINT_UPDATE => {
                let request: ModelEndpointUpdateRequest = decode(params, correlation_id)?;
                let command_invocation_id =
                    require_command_id(command_invocation_id, correlation_id)?;
                let expected_models_revision = request.expected_models_revision;
                let expected_endpoint_revision = request.expected_endpoint_revision;
                let endpoint_id = request.endpoint_id;
                let input = request.into_input();
                let completion = self
                    .config_journal
                    .execute(command_invocation_id, correlation_id, |journal_id| {
                        let prepared = self.model_config.prepare_update(
                            journal_id.to_string(),
                            expected_models_revision.value(),
                            |registry| {
                                let current = registry
                                    .endpoints
                                    .get(&endpoint_id)
                                    .ok_or(ModelConfigError::EndpointIdNotFound(endpoint_id))?;
                                if current.revision.value() != expected_endpoint_revision.value() {
                                    return Err(ModelConfigError::EndpointObjectRevisionConflict {
                                        expected: expected_endpoint_revision.value(),
                                        actual: current.revision.value(),
                                    });
                                }
                                update_endpoint(registry, input, UtcTimestamp::now())
                            },
                        )?;
                        let previous = prepared
                            .previous
                            .registry
                            .endpoints
                            .get(&endpoint_id)
                            .cloned()
                            .ok_or(ModelConfigError::EndpointIdNotFound(endpoint_id))?;
                        let endpoint = prepared
                            .candidate_registry
                            .endpoints
                            .get(&endpoint_id)
                            .cloned()
                            .ok_or(ModelConfigError::EndpointIdNotFound(endpoint_id))?;
                        let result = ModelEndpointMutationResult {
                            changed: prepared.changed,
                            revision: prepared.candidate_revision,
                            value: endpoint.clone(),
                        };
                        if !prepared.changed {
                            return Ok(ModelConfigCommandPlan::NoChange {
                                success_result: serde_json::to_value(result)?,
                            });
                        }

                        let existing_credential =
                            self.persistence.model_credential_record_for_endpoint(
                                endpoint.id,
                                ModelCredentialSlot::ApiKey,
                            )?;
                        let (credential_slot_creations, credential_override) =
                            if endpoint.auth_scheme != EndpointAuthScheme::None
                                && existing_credential.is_none()
                            {
                                self.prepare_endpoint_credential_slot(&endpoint)?
                            } else {
                                (Vec::new(), None)
                            };
                        let status_overrides = credential_override
                            .clone()
                            .map(|status| BTreeMap::from([(endpoint.id, status)]))
                            .unwrap_or_default();
                        let credential_status = match credential_override {
                            Some(status) => Some(status),
                            None => self.projections.active_credential_status(&endpoint)?,
                        };
                        let previous_fingerprint =
                            endpoint_fingerprint(&previous).map_err(ModelConfigError::from)?;
                        let current_fingerprint =
                            endpoint_fingerprint(&endpoint).map_err(ModelConfigError::from)?;
                        let credential_binding_mismatch =
                            credential_status.as_ref().is_some_and(|status| {
                                status.error_code.as_ref().is_some_and(|code| {
                                    code.as_str() == "credential.binding_mismatch"
                                })
                            });
                        let snapshot = prepared_snapshot(&prepared);
                        let model_settings = self
                            .projections
                            .model_settings(&snapshot, &status_overrides)
                            .map_err(ModelConfigJournalError::from)?;
                        let endpoint_detail = self
                            .projections
                            .endpoint_detail(&snapshot, endpoint.id, &status_overrides)
                            .map_err(ModelConfigJournalError::from)?;
                        let mut events = vec![user_event(
                            "ModelEndpointUpdated",
                            correlation_id,
                            Some(journal_id.to_string()),
                            &ModelEndpointUpdatedPayload {
                                endpoint: endpoint.clone(),
                                credential_status,
                                previous_endpoint_fingerprint: previous_fingerprint,
                                endpoint_fingerprint: current_fingerprint,
                                credential_binding_mismatch,
                                changed_fields: endpoint_changed_fields(&previous, &endpoint),
                                models_revision: prepared.candidate_revision,
                            },
                        )?];
                        let mut projections = vec![
                            ModelProjectionDraft::ModelSettings(model_settings),
                            ModelProjectionDraft::EndpointDetail(Box::new(endpoint_detail)),
                        ];
                        for profile_id in &endpoint.model_order {
                            let Some(previous_profile) =
                                prepared.previous.registry.models.get(profile_id)
                            else {
                                continue;
                            };
                            let profile = prepared
                                .candidate_registry
                                .models
                                .get(profile_id)
                                .ok_or(ModelConfigError::ProfileNotFound(*profile_id))?;
                            let changed_fields = profile_changed_fields(previous_profile, profile);
                            if changed_fields.is_empty() {
                                continue;
                            }
                            events.push(user_event(
                                "ModelProfileUpdated",
                                correlation_id,
                                Some(journal_id.to_string()),
                                &ModelProfileUpdatedPayload {
                                    provider_id: endpoint.provider_id,
                                    endpoint_id: endpoint.id,
                                    profile: profile.clone(),
                                    changed_fields,
                                    models_revision: prepared.candidate_revision,
                                },
                            )?);
                            projections.push(ModelProjectionDraft::ModelProfile(Box::new(
                                self.projections
                                    .model_profile(&snapshot, *profile_id, &status_overrides)
                                    .map_err(ModelConfigJournalError::from)?,
                            )));
                        }
                        let prepared_bundle =
                            prepared.prepared_bundle.ok_or(ModelConfigError::InternalInvariant(
                                "changed Endpoint mutation lacks a prepared bundle",
                            ))?;
                        Ok(ModelConfigCommandPlan::Publish {
                            prepared_bundle: Box::new(prepared_bundle),
                            expected_models_revision: Revision::new(
                                expected_models_revision.value(),
                            ),
                            reserved_models_revision: Some(prepared.candidate_revision),
                            expected_aura_revision: None,
                            reserved_aura_revision: None,
                            credential_slot_creations,
                            session_binding_clears: Vec::new(),
                            metadata_events: events,
                            projection_drafts: projections,
                            success_result: serde_json::to_value(result)?,
                        })
                    })
                    .map_err(|error| journal_error(error, correlation_id))?;
                Ok(P05CommandDispatch {
                    value: completion.success_result,
                    projection_changes: completion.projection_changes,
                })
            }
            model_ipc_method::ENDPOINT_DELETE => {
                self.dispatch_delete_endpoint(params, command_invocation_id, correlation_id).await
            }
            model_ipc_method::PROFILE_CREATE_FROM_CATALOG => {
                let request: ModelProfileCreateFromCatalogRequest = decode(params, correlation_id)?;
                let command_invocation_id =
                    require_command_id(command_invocation_id, correlation_id)?;
                let catalog = self.provider_catalog.current_snapshot().ok_or_else(|| {
                    catalog_error(
                        aura_model_catalog::CatalogError::Unavailable.into(),
                        correlation_id,
                    )
                })?;
                if catalog.catalog_revision != request.expected_catalog_revision {
                    return Err(catalog_error(
                        aura_model_catalog::CatalogError::RevisionConflict {
                            expected: request.expected_catalog_revision.value(),
                            actual: catalog.catalog_revision.value(),
                        }
                        .into(),
                        correlation_id,
                    ));
                }
                let completion = self
                    .config_journal
                    .execute(command_invocation_id, correlation_id, |journal_id| {
                        let prepared = self.model_config.prepare_update(
                            journal_id.to_string(),
                            request.expected_models_revision.value(),
                            |registry| {
                                let endpoint = registry
                                    .endpoints
                                    .get(&request.endpoint_id)
                                    .ok_or(ModelConfigError::EndpointIdNotFound(
                                        request.endpoint_id,
                                    ))?;
                                if endpoint.revision.value()
                                    != request.expected_endpoint_revision.value()
                                {
                                    return Err(ModelConfigError::EndpointObjectRevisionConflict {
                                        expected: request.expected_endpoint_revision.value(),
                                        actual: endpoint.revision.value(),
                                    });
                                }
                                create_model_from_catalog(
                                    registry,
                                    request.endpoint_id,
                                    &catalog,
                                    &request.catalog_provider_id,
                                    &request.catalog_model_id,
                                    &request.expected_catalog_object_hash,
                                    UtcTimestamp::now(),
                                )
                            },
                        )?;
                        let profile = prepared
                            .candidate_registry
                            .models
                            .get(&prepared.value)
                            .cloned()
                            .ok_or(ModelConfigError::InternalInvariant(
                                "catalog-created Model Profile is absent from the candidate registry",
                            ))?;
                        let endpoint = prepared
                            .candidate_registry
                            .endpoints
                            .get(&profile.endpoint_id)
                            .ok_or(ModelConfigError::EndpointNotFound(profile.id))?;
                        let snapshot = prepared_snapshot(&prepared);
                        let status_overrides = BTreeMap::new();
                        let result = ModelProfileMutationResult {
                            changed: prepared.changed,
                            revision: prepared.candidate_revision,
                            value: profile.clone(),
                        };
                        let prepared_bundle = prepared.prepared_bundle.ok_or(
                            ModelConfigError::InternalInvariant(
                                "catalog-created Model Profile mutation lacks a prepared bundle",
                            ),
                        )?;
                        Ok(ModelConfigCommandPlan::Publish {
                            prepared_bundle: Box::new(prepared_bundle),
                            expected_models_revision: Revision::new(
                                request.expected_models_revision.value(),
                            ),
                            reserved_models_revision: Some(prepared.candidate_revision),
                            expected_aura_revision: None,
                            reserved_aura_revision: None,
                            credential_slot_creations: Vec::new(),
                            session_binding_clears: Vec::new(),
                            metadata_events: vec![user_event(
                                "ModelProfileCreated",
                                correlation_id,
                                None,
                                &ModelProfileCreatedPayload {
                                    provider_id: endpoint.provider_id,
                                    endpoint_id: endpoint.id,
                                    profile: profile.clone(),
                                    models_revision: prepared.candidate_revision,
                                },
                            )?],
                            projection_drafts: vec![
                                ModelProjectionDraft::ModelSettings(
                                    self.projections
                                        .model_settings(&snapshot, &status_overrides)
                                        .map_err(ModelConfigJournalError::from)?,
                                ),
                                ModelProjectionDraft::EndpointDetail(Box::new(
                                    self.projections
                                        .endpoint_detail(
                                            &snapshot,
                                            endpoint.id,
                                            &status_overrides,
                                        )
                                        .map_err(ModelConfigJournalError::from)?,
                                )),
                                ModelProjectionDraft::ModelProfile(Box::new(
                                    self.projections
                                        .model_profile(&snapshot, profile.id, &status_overrides)
                                        .map_err(ModelConfigJournalError::from)?,
                                )),
                            ],
                            success_result: serde_json::to_value(result)?,
                        })
                    })
                    .map_err(|error| journal_error(error, correlation_id))?;
                Ok(P05CommandDispatch {
                    value: completion.success_result,
                    projection_changes: completion.projection_changes,
                })
            }
            model_ipc_method::PROFILE_APPLY_CATALOG_UPDATE => {
                let request: ModelProfileApplyCatalogUpdateRequest =
                    decode(params, correlation_id)?;
                let command_invocation_id =
                    require_command_id(command_invocation_id, correlation_id)?;
                let catalog = self.provider_catalog.current_snapshot().ok_or_else(|| {
                    catalog_error(
                        aura_model_catalog::CatalogError::Unavailable.into(),
                        correlation_id,
                    )
                })?;
                if catalog.catalog_revision != request.expected_catalog_revision {
                    return Err(catalog_error(
                        aura_model_catalog::CatalogError::RevisionConflict {
                            expected: request.expected_catalog_revision.value(),
                            actual: catalog.catalog_revision.value(),
                        }
                        .into(),
                        correlation_id,
                    ));
                }
                let completion = self
                    .config_journal
                    .execute(command_invocation_id, correlation_id, |journal_id| {
                        let prepared = self.model_config.prepare_update(
                            journal_id.to_string(),
                            request.expected_models_revision.value(),
                            |registry| {
                                let profile =
                                    registry.models.get(&request.model_profile_id).ok_or(
                                        ModelConfigError::ProfileNotFound(request.model_profile_id),
                                    )?;
                                if profile.revision.value()
                                    != request.expected_model_profile_revision.value()
                                {
                                    return Err(ModelConfigError::ProfileObjectRevisionConflict {
                                        expected: request.expected_model_profile_revision.value(),
                                        actual: profile.revision.value(),
                                    });
                                }
                                apply_catalog_update(
                                    registry,
                                    request.model_profile_id,
                                    &catalog,
                                    &request.expected_catalog_object_hash,
                                    UtcTimestamp::now(),
                                )
                            },
                        )?;
                        let previous = prepared
                            .previous
                            .registry
                            .models
                            .get(&request.model_profile_id)
                            .cloned()
                            .ok_or(ModelConfigError::ProfileNotFound(request.model_profile_id))?;
                        let profile = prepared
                            .candidate_registry
                            .models
                            .get(&request.model_profile_id)
                            .cloned()
                            .ok_or(ModelConfigError::ProfileNotFound(request.model_profile_id))?;
                        let result = ModelProfileMutationResult {
                            changed: prepared.changed,
                            revision: prepared.candidate_revision,
                            value: profile.clone(),
                        };
                        if !prepared.changed {
                            return Ok(ModelConfigCommandPlan::NoChange {
                                success_result: serde_json::to_value(result)?,
                            });
                        }
                        let endpoint = prepared
                            .candidate_registry
                            .endpoints
                            .get(&profile.endpoint_id)
                            .ok_or(ModelConfigError::EndpointNotFound(profile.id))?;
                        let snapshot = prepared_snapshot(&prepared);
                        let status_overrides = BTreeMap::new();
                        let prepared_bundle =
                            prepared.prepared_bundle.ok_or(ModelConfigError::InternalInvariant(
                                "catalog update mutation lacks a prepared bundle",
                            ))?;
                        Ok(ModelConfigCommandPlan::Publish {
                            prepared_bundle: Box::new(prepared_bundle),
                            expected_models_revision: Revision::new(
                                request.expected_models_revision.value(),
                            ),
                            reserved_models_revision: Some(prepared.candidate_revision),
                            expected_aura_revision: None,
                            reserved_aura_revision: None,
                            credential_slot_creations: Vec::new(),
                            session_binding_clears: Vec::new(),
                            metadata_events: vec![user_event(
                                "ModelProfileUpdated",
                                correlation_id,
                                None,
                                &ModelProfileUpdatedPayload {
                                    provider_id: endpoint.provider_id,
                                    endpoint_id: endpoint.id,
                                    profile: profile.clone(),
                                    changed_fields: profile_changed_fields(&previous, &profile),
                                    models_revision: prepared.candidate_revision,
                                },
                            )?],
                            projection_drafts: vec![
                                ModelProjectionDraft::ModelSettings(
                                    self.projections
                                        .model_settings(&snapshot, &status_overrides)
                                        .map_err(ModelConfigJournalError::from)?,
                                ),
                                ModelProjectionDraft::EndpointDetail(Box::new(
                                    self.projections
                                        .endpoint_detail(&snapshot, endpoint.id, &status_overrides)
                                        .map_err(ModelConfigJournalError::from)?,
                                )),
                                ModelProjectionDraft::ModelProfile(Box::new(
                                    self.projections
                                        .model_profile(&snapshot, profile.id, &status_overrides)
                                        .map_err(ModelConfigJournalError::from)?,
                                )),
                            ],
                            success_result: serde_json::to_value(result)?,
                        })
                    })
                    .map_err(|error| journal_error(error, correlation_id))?;
                Ok(P05CommandDispatch {
                    value: completion.success_result,
                    projection_changes: completion.projection_changes,
                })
            }
            model_ipc_method::PROFILE_CREATE_MANUAL => {
                let request: ModelProfileCreateManualRequest = decode(params, correlation_id)?;
                let command_invocation_id =
                    require_command_id(command_invocation_id, correlation_id)?;
                let expected_revision = request.expected_revision;
                let input = request.input;
                let completion = self
                    .config_journal
                    .execute(command_invocation_id, correlation_id, |journal_id| {
                        let prepared = self.model_config.prepare_update(
                            journal_id.to_string(),
                            expected_revision.value(),
                            |registry| add_manual_model(registry, input, UtcTimestamp::now()),
                        )?;
                        let profile = prepared
                            .candidate_registry
                            .models
                            .get(&prepared.value.model_profile_id)
                            .cloned()
                            .ok_or(ModelConfigError::InternalInvariant(
                                "created Model Profile is absent from the candidate registry",
                            ))?;
                        let endpoint = prepared
                            .candidate_registry
                            .endpoints
                            .get(&profile.endpoint_id)
                            .cloned()
                            .ok_or(ModelConfigError::EndpointNotFound(profile.id))?;
                        let provider_id = endpoint.provider_id;
                        let snapshot = prepared_snapshot(&prepared);
                        let status_overrides = BTreeMap::new();
                        let model_settings = self
                            .projections
                            .model_settings(&snapshot, &status_overrides)
                            .map_err(ModelConfigJournalError::from)?;
                        let endpoint_detail = self
                            .projections
                            .endpoint_detail(&snapshot, profile.endpoint_id, &status_overrides)
                            .map_err(ModelConfigJournalError::from)?;
                        let model_profile = self
                            .projections
                            .model_profile(&snapshot, profile.id, &status_overrides)
                            .map_err(ModelConfigJournalError::from)?;
                        let result = ModelProfileMutationResult {
                            changed: prepared.changed,
                            revision: prepared.candidate_revision,
                            value: profile.clone(),
                        };
                        let prepared_bundle =
                            prepared.prepared_bundle.ok_or(ModelConfigError::InternalInvariant(
                                "created Model Profile mutation lacks a prepared bundle",
                            ))?;
                        Ok(ModelConfigCommandPlan::Publish {
                            prepared_bundle: Box::new(prepared_bundle),
                            expected_models_revision: Revision::new(expected_revision.value()),
                            reserved_models_revision: Some(prepared.candidate_revision),
                            expected_aura_revision: None,
                            reserved_aura_revision: None,
                            credential_slot_creations: Vec::new(),
                            session_binding_clears: Vec::new(),
                            metadata_events: vec![user_event(
                                "ModelProfileCreated",
                                correlation_id,
                                None,
                                &ModelProfileCreatedPayload {
                                    provider_id,
                                    endpoint_id: profile.endpoint_id,
                                    profile,
                                    models_revision: prepared.candidate_revision,
                                },
                            )?],
                            projection_drafts: vec![
                                ModelProjectionDraft::ModelSettings(model_settings),
                                ModelProjectionDraft::EndpointDetail(Box::new(endpoint_detail)),
                                ModelProjectionDraft::ModelProfile(Box::new(model_profile)),
                            ],
                            success_result: serde_json::to_value(result)?,
                        })
                    })
                    .map_err(|error| journal_error(error, correlation_id))?;
                Ok(P05CommandDispatch {
                    value: completion.success_result,
                    projection_changes: completion.projection_changes,
                })
            }
            model_ipc_method::PROFILE_DELETE => {
                let request: ModelProfileDeleteRequest = decode(params, correlation_id)?;
                let command_invocation_id =
                    require_command_id(command_invocation_id, correlation_id)?;
                let completion = self
                    .config_journal
                    .execute(command_invocation_id, correlation_id, |journal_id| {
                        let prepared = self.model_config.prepare_delete_model_profile(
                            journal_id.to_string(),
                            request.expected_revision.value(),
                            request.model_profile_id,
                        )?;
                        let deleted_profile = prepared
                            .previous
                            .registry
                            .models
                            .get(&request.model_profile_id)
                            .cloned()
                            .ok_or(ModelConfigError::ProfileNotFound(request.model_profile_id))?;
                        let endpoint = prepared
                            .previous
                            .registry
                            .endpoints
                            .get(&deleted_profile.endpoint_id)
                            .cloned()
                            .ok_or(ModelConfigError::EndpointNotFound(request.model_profile_id))?;
                        let deleted_ids = BTreeSet::from([request.model_profile_id]);
                        let bindings =
                            self.persistence.session_model_bindings_for_profiles(&deleted_ids)?;
                        let candidate_default = DefaultModelSelection {
                            model_profile_id: if prepared.value.default_model_cleared {
                                None
                            } else {
                                self.model_config.default_model_selection()?.model_profile_id
                            },
                            aura_config_revision: prepared.candidate_aura_revision.value(),
                        };
                        let candidate_snapshot = aura_model_config::ModelConfigSnapshot {
                            revision: prepared.candidate_revision.value(),
                            content_hash: prepared.previous.content_hash.clone(),
                            source: prepared.previous.source,
                            registry: prepared.candidate_registry.clone(),
                        };
                        let atomic_group_id = journal_id.to_string();
                        let mut events = Vec::new();
                        let mut session_binding_clears = Vec::new();
                        let mut projection_drafts = Vec::new();
                        for binding in bindings {
                            let next_revision = binding
                                .binding_revision
                                .value()
                                .checked_add(1)
                                .ok_or(ModelConfigError::InternalInvariant(
                                    "Session binding revision is exhausted during Profile deletion",
                                ))?;
                            let mut event = user_event(
                                "SessionModelBindingChanged",
                                correlation_id,
                                Some(atomic_group_id.clone()),
                                &SessionModelBindingChangedPayload {
                                    session_id: binding.session_id,
                                    previous_binding: Some(binding.clone()),
                                    binding: None,
                                    binding_revision: Revision::new(next_revision),
                                },
                            )?;
                            event.session_id = Some(binding.session_id);
                            events.push(event);
                            session_binding_clears.push(PreparedSessionModelBindingClear {
                                session_id: binding.session_id,
                                expected_binding_revision: binding.binding_revision,
                                expected_model_profile_id: binding.model_profile_id,
                            });
                            projection_drafts.push(ModelProjectionDraft::SessionModelSelector(
                                self.projections
                                    .session_model_selector_with_default(
                                        &candidate_snapshot,
                                        binding.session_id,
                                        None,
                                        &candidate_default,
                                    )
                                    .map_err(ModelConfigJournalError::from)?,
                            ));
                        }
                        if prepared.value.default_model_cleared {
                            events.push(user_event(
                                "DefaultModelChanged",
                                correlation_id,
                                Some(atomic_group_id.clone()),
                                &DefaultModelChangedPayload {
                                    previous_model_profile_id: Some(request.model_profile_id),
                                    model_profile_id: None,
                                    aura_revision: prepared.candidate_aura_revision,
                                },
                            )?);
                        }
                        events.push(user_event(
                            "ModelProfileDeleted",
                            correlation_id,
                            Some(atomic_group_id),
                            &ModelProfileDeletedPayload {
                                provider_id: endpoint.provider_id,
                                endpoint_id: endpoint.id,
                                model_profile_id: request.model_profile_id,
                                default_model_cleared: prepared.value.default_model_cleared,
                                models_revision: prepared.candidate_revision,
                            },
                        )?);
                        let status_overrides = BTreeMap::new();
                        projection_drafts.insert(
                            0,
                            ModelProjectionDraft::ModelSettings(
                                self.projections
                                    .model_settings_with_default(
                                        &candidate_snapshot,
                                        &status_overrides,
                                        &candidate_default,
                                    )
                                    .map_err(ModelConfigJournalError::from)?,
                            ),
                        );
                        projection_drafts.insert(
                            1,
                            ModelProjectionDraft::EndpointDetail(Box::new(
                                self.projections
                                    .endpoint_detail(
                                        &candidate_snapshot,
                                        endpoint.id,
                                        &status_overrides,
                                    )
                                    .map_err(ModelConfigJournalError::from)?,
                            )),
                        );
                        let result = ModelDeletionMutationResult {
                            changed: true,
                            revision: prepared.candidate_revision,
                            value: prepared.value.clone(),
                        };
                        Ok(ModelConfigCommandPlan::Publish {
                            prepared_bundle: Box::new(prepared.prepared_bundle),
                            expected_models_revision: Revision::new(
                                request.expected_revision.value(),
                            ),
                            reserved_models_revision: Some(prepared.candidate_revision),
                            expected_aura_revision: Some(prepared.expected_aura_revision),
                            reserved_aura_revision: prepared
                                .value
                                .default_model_cleared
                                .then_some(prepared.candidate_aura_revision),
                            credential_slot_creations: Vec::new(),
                            session_binding_clears,
                            metadata_events: events,
                            projection_drafts,
                            success_result: serde_json::to_value(result)?,
                        })
                    })
                    .map_err(|error| journal_error(error, correlation_id))?;
                let mut projection_changes = completion.projection_changes;
                match self
                    .cancel_model_availability_for_deleted_targets(
                        &BTreeSet::new(),
                        &BTreeSet::from([request.model_profile_id]),
                    )
                    .await
                {
                    Ok(changes) => projection_changes.extend(changes),
                    Err(error) => aura_core_base::append_core_log(
                        "p05_target_delete_operation_cancellation_failed",
                        serde_json::json!({
                            "target_kind": "model_profile",
                            "target_id": request.model_profile_id,
                            "failure_class": availability_failure_class(&error),
                        }),
                    ),
                }
                Ok(P05CommandDispatch { value: completion.success_result, projection_changes })
            }
            model_ipc_method::PROFILE_UPDATE => {
                let request: ModelProfileUpdateRequest = decode(params, correlation_id)?;
                let command_invocation_id =
                    require_command_id(command_invocation_id, correlation_id)?;
                let completion = self
                    .config_journal
                    .execute(command_invocation_id, correlation_id, |journal_id| {
                        let prepared = self.model_config.prepare_update(
                            journal_id.to_string(),
                            request.expected_revision.value(),
                            |registry| {
                                update_model_profile(
                                    registry,
                                    UpdateModelProfileInput {
                                        model_profile_id: request.model_profile_id,
                                        display_name: request.display_name,
                                        notes: request.notes,
                                        enabled: request.enabled,
                                        request_defaults: request.request_defaults,
                                        historical_snapshot_max_tokens: request
                                            .historical_snapshot_max_tokens,
                                    },
                                    UtcTimestamp::now(),
                                )
                            },
                        )?;
                        let previous = prepared
                            .previous
                            .registry
                            .models
                            .get(&request.model_profile_id)
                            .cloned()
                            .ok_or(ModelConfigError::ProfileNotFound(request.model_profile_id))?;
                        let profile = prepared
                            .candidate_registry
                            .models
                            .get(&request.model_profile_id)
                            .cloned()
                            .ok_or(ModelConfigError::ProfileNotFound(request.model_profile_id))?;
                        let result = ModelProfileMutationResult {
                            changed: prepared.changed,
                            revision: prepared.candidate_revision,
                            value: profile.clone(),
                        };
                        if !prepared.changed {
                            return Ok(ModelConfigCommandPlan::NoChange {
                                success_result: serde_json::to_value(result)?,
                            });
                        }
                        let previous_endpoint = prepared
                            .previous
                            .registry
                            .endpoints
                            .get(&profile.endpoint_id)
                            .cloned()
                            .ok_or(ModelConfigError::EndpointNotFound(profile.id))?;
                        let endpoint = prepared
                            .candidate_registry
                            .endpoints
                            .get(&profile.endpoint_id)
                            .cloned()
                            .ok_or(ModelConfigError::EndpointNotFound(profile.id))?;
                        let snapshot = prepared_snapshot(&prepared);
                        let status_overrides = BTreeMap::new();
                        let model_settings = self
                            .projections
                            .model_settings(&snapshot, &status_overrides)
                            .map_err(ModelConfigJournalError::from)?;
                        let endpoint_detail = self
                            .projections
                            .endpoint_detail(&snapshot, profile.endpoint_id, &status_overrides)
                            .map_err(ModelConfigJournalError::from)?;
                        let model_profile = self
                            .projections
                            .model_profile(&snapshot, profile.id, &status_overrides)
                            .map_err(ModelConfigJournalError::from)?;
                        let mut events = Vec::new();
                        if previous_endpoint != endpoint {
                            let credential_status =
                                self.projections.active_credential_status(&endpoint)?;
                            let credential_binding_mismatch =
                                credential_status.as_ref().is_some_and(|status| {
                                    status.error_code.as_ref().is_some_and(|code| {
                                        code.as_str() == "credential.binding_mismatch"
                                    })
                                });
                            events.push(user_event(
                                "ModelEndpointUpdated",
                                correlation_id,
                                Some(journal_id.to_string()),
                                &ModelEndpointUpdatedPayload {
                                    endpoint: endpoint.clone(),
                                    credential_status,
                                    previous_endpoint_fingerprint: endpoint_fingerprint(
                                        &previous_endpoint,
                                    )
                                    .map_err(ModelConfigError::from)?,
                                    endpoint_fingerprint: endpoint_fingerprint(&endpoint)
                                        .map_err(ModelConfigError::from)?,
                                    credential_binding_mismatch,
                                    changed_fields: endpoint_changed_fields(
                                        &previous_endpoint,
                                        &endpoint,
                                    ),
                                    models_revision: prepared.candidate_revision,
                                },
                            )?);
                        }
                        events.push(user_event(
                            "ModelProfileUpdated",
                            correlation_id,
                            Some(journal_id.to_string()),
                            &ModelProfileUpdatedPayload {
                                provider_id: endpoint.provider_id,
                                endpoint_id: profile.endpoint_id,
                                profile,
                                changed_fields: profile_changed_fields(&previous, &result.value),
                                models_revision: prepared.candidate_revision,
                            },
                        )?);
                        let prepared_bundle =
                            prepared.prepared_bundle.ok_or(ModelConfigError::InternalInvariant(
                                "changed Model Profile mutation lacks a prepared bundle",
                            ))?;
                        Ok(ModelConfigCommandPlan::Publish {
                            prepared_bundle: Box::new(prepared_bundle),
                            expected_models_revision: Revision::new(
                                request.expected_revision.value(),
                            ),
                            reserved_models_revision: Some(prepared.candidate_revision),
                            expected_aura_revision: None,
                            reserved_aura_revision: None,
                            credential_slot_creations: Vec::new(),
                            session_binding_clears: Vec::new(),
                            metadata_events: events,
                            projection_drafts: vec![
                                ModelProjectionDraft::ModelSettings(model_settings),
                                ModelProjectionDraft::EndpointDetail(Box::new(endpoint_detail)),
                                ModelProjectionDraft::ModelProfile(Box::new(model_profile)),
                            ],
                            success_result: serde_json::to_value(result)?,
                        })
                    })
                    .map_err(|error| journal_error(error, correlation_id))?;
                Ok(P05CommandDispatch {
                    value: completion.success_result,
                    projection_changes: completion.projection_changes,
                })
            }
            model_ipc_method::PROFILE_SET_SUBAGENT_AVAILABILITY => {
                let request: ModelProfileSetSubagentAvailabilityRequest =
                    decode(params, correlation_id)?;
                let command_invocation_id =
                    require_command_id(command_invocation_id, correlation_id)?;
                let completion = self
                    .config_journal
                    .execute(command_invocation_id, correlation_id, |journal_id| {
                        let prepared = self.model_config.prepare_update(
                            journal_id.to_string(),
                            request.expected_revision.value(),
                            |registry| {
                                set_model_subagent_availability(
                                    registry,
                                    request.model_profile_id,
                                    request.available_for_subagents,
                                    UtcTimestamp::now(),
                                )
                            },
                        )?;
                        let previous = prepared
                            .previous
                            .registry
                            .models
                            .get(&request.model_profile_id)
                            .cloned()
                            .ok_or(ModelConfigError::ProfileNotFound(request.model_profile_id))?;
                        let profile = prepared
                            .candidate_registry
                            .models
                            .get(&request.model_profile_id)
                            .cloned()
                            .ok_or(ModelConfigError::ProfileNotFound(request.model_profile_id))?;
                        let result = ModelProfileMutationResult {
                            changed: prepared.changed,
                            revision: prepared.candidate_revision,
                            value: profile.clone(),
                        };
                        if !prepared.changed {
                            return Ok(ModelConfigCommandPlan::NoChange {
                                success_result: serde_json::to_value(result)?,
                            });
                        }
                        let endpoint = prepared
                            .candidate_registry
                            .endpoints
                            .get(&profile.endpoint_id)
                            .ok_or(ModelConfigError::EndpointNotFound(profile.id))?;
                        let snapshot = prepared_snapshot(&prepared);
                        let status_overrides = BTreeMap::new();
                        let prepared_bundle =
                            prepared.prepared_bundle.ok_or(ModelConfigError::InternalInvariant(
                                "changed subagent availability mutation lacks a prepared bundle",
                            ))?;
                        Ok(ModelConfigCommandPlan::Publish {
                            prepared_bundle: Box::new(prepared_bundle),
                            expected_models_revision: Revision::new(
                                request.expected_revision.value(),
                            ),
                            reserved_models_revision: Some(prepared.candidate_revision),
                            expected_aura_revision: None,
                            reserved_aura_revision: None,
                            credential_slot_creations: Vec::new(),
                            session_binding_clears: Vec::new(),
                            metadata_events: vec![user_event(
                                "ModelSubagentAvailabilityChanged",
                                correlation_id,
                                None,
                                &ModelSubagentAvailabilityChangedPayload {
                                    provider_id: endpoint.provider_id,
                                    model_profile_id: profile.id,
                                    previous_value: previous.available_for_subagents,
                                    available_for_subagents: profile.available_for_subagents,
                                    models_revision: prepared.candidate_revision,
                                },
                            )?],
                            projection_drafts: vec![
                                ModelProjectionDraft::ModelSettings(
                                    self.projections
                                        .model_settings(&snapshot, &status_overrides)
                                        .map_err(ModelConfigJournalError::from)?,
                                ),
                                ModelProjectionDraft::EndpointDetail(Box::new(
                                    self.projections
                                        .endpoint_detail(
                                            &snapshot,
                                            profile.endpoint_id,
                                            &status_overrides,
                                        )
                                        .map_err(ModelConfigJournalError::from)?,
                                )),
                                ModelProjectionDraft::ModelProfile(Box::new(
                                    self.projections
                                        .model_profile(&snapshot, profile.id, &status_overrides)
                                        .map_err(ModelConfigJournalError::from)?,
                                )),
                            ],
                            success_result: serde_json::to_value(result)?,
                        })
                    })
                    .map_err(|error| journal_error(error, correlation_id))?;
                Ok(P05CommandDispatch {
                    value: completion.success_result,
                    projection_changes: completion.projection_changes,
                })
            }
            model_ipc_method::PROVIDER_SET_ALL_CURRENT_MODELS_SUBAGENT_AVAILABILITY => {
                let request: ModelProviderSetAllCurrentModelsSubagentAvailabilityRequest =
                    decode(params, correlation_id)?;
                let command_invocation_id =
                    require_command_id(command_invocation_id, correlation_id)?;
                let completion = self
                    .config_journal
                    .execute(command_invocation_id, correlation_id, |journal_id| {
                        let prepared = self.model_config.prepare_update(
                            journal_id.to_string(),
                            request.expected_revision.value(),
                            |registry| {
                                set_provider_current_models_subagent_availability(
                                    registry,
                                    request.provider_id,
                                    request.available_for_subagents,
                                    UtcTimestamp::now(),
                                )
                            },
                        )?;
                        let mut changed_model_profile_ids = prepared.value.clone();
                        changed_model_profile_ids.sort_unstable();
                        let result = ProviderBulkAvailabilityResult {
                            provider_id: request.provider_id,
                            changed_model_profile_ids: changed_model_profile_ids.clone(),
                            revision: prepared.candidate_revision,
                        };
                        if !prepared.changed {
                            return Ok(ModelConfigCommandPlan::NoChange {
                                success_result: serde_json::to_value(result)?,
                            });
                        }
                        let snapshot = prepared_snapshot(&prepared);
                        let status_overrides = BTreeMap::new();
                        let mut events = Vec::with_capacity(changed_model_profile_ids.len());
                        let mut projection_drafts = vec![ModelProjectionDraft::ModelSettings(
                            self.projections
                                .model_settings(&snapshot, &status_overrides)
                                .map_err(ModelConfigJournalError::from)?,
                        )];
                        let mut affected_endpoint_ids = BTreeSet::new();
                        for model_profile_id in &changed_model_profile_ids {
                            let previous = prepared
                                .previous
                                .registry
                                .models
                                .get(model_profile_id)
                                .ok_or(ModelConfigError::ProfileNotFound(*model_profile_id))?;
                            let profile = prepared
                                .candidate_registry
                                .models
                                .get(model_profile_id)
                                .ok_or(ModelConfigError::ProfileNotFound(*model_profile_id))?;
                            affected_endpoint_ids.insert(profile.endpoint_id);
                            events.push(user_event(
                                "ModelSubagentAvailabilityChanged",
                                correlation_id,
                                Some(journal_id.to_string()),
                                &ModelSubagentAvailabilityChangedPayload {
                                    provider_id: request.provider_id,
                                    model_profile_id: *model_profile_id,
                                    previous_value: previous.available_for_subagents,
                                    available_for_subagents: profile.available_for_subagents,
                                    models_revision: prepared.candidate_revision,
                                },
                            )?);
                            projection_drafts.push(ModelProjectionDraft::ModelProfile(Box::new(
                                self.projections
                                    .model_profile(&snapshot, *model_profile_id, &status_overrides)
                                    .map_err(ModelConfigJournalError::from)?,
                            )));
                        }
                        for endpoint_id in affected_endpoint_ids {
                            projection_drafts.push(ModelProjectionDraft::EndpointDetail(Box::new(
                                self.projections
                                    .endpoint_detail(&snapshot, endpoint_id, &status_overrides)
                                    .map_err(ModelConfigJournalError::from)?,
                            )));
                        }
                        let prepared_bundle =
                            prepared.prepared_bundle.ok_or(ModelConfigError::InternalInvariant(
                                "changed Provider batch mutation lacks a prepared bundle",
                            ))?;
                        Ok(ModelConfigCommandPlan::Publish {
                            prepared_bundle: Box::new(prepared_bundle),
                            expected_models_revision: Revision::new(
                                request.expected_revision.value(),
                            ),
                            reserved_models_revision: Some(prepared.candidate_revision),
                            expected_aura_revision: None,
                            reserved_aura_revision: None,
                            credential_slot_creations: Vec::new(),
                            session_binding_clears: Vec::new(),
                            metadata_events: events,
                            projection_drafts,
                            success_result: serde_json::to_value(result)?,
                        })
                    })
                    .map_err(|error| journal_error(error, correlation_id))?;
                Ok(P05CommandDispatch {
                    value: completion.success_result,
                    projection_changes: completion.projection_changes,
                })
            }
            model_ipc_method::DEFAULT_SET => {
                let request: ModelDefaultSetRequest = decode(params, correlation_id)?;
                let command_invocation_id =
                    require_command_id(command_invocation_id, correlation_id)?;
                let completion = self
                    .config_journal
                    .execute(command_invocation_id, correlation_id, |journal_id| {
                        let prepared = self.model_config.prepare_set_default_model(
                            journal_id.to_string(),
                            request.expected_revision.value(),
                            request.model_profile_id,
                        )?;
                        let result = DefaultModelMutationResult {
                            changed: prepared.changed,
                            revision: Revision::new(prepared.candidate.aura_config_revision),
                            value: prepared.candidate.clone(),
                        };
                        if !prepared.changed {
                            return Ok(ModelConfigCommandPlan::NoChange {
                                success_result: serde_json::to_value(result)?,
                            });
                        }

                        let model_settings = self
                            .projections
                            .model_settings_with_default(
                                &prepared.models_snapshot,
                                &BTreeMap::new(),
                                &prepared.candidate,
                            )
                            .map_err(ModelConfigJournalError::from)?;
                        let prepared_bundle =
                            prepared.prepared_bundle.ok_or(ModelConfigError::InternalInvariant(
                                "changed default-model mutation lacks a prepared bundle",
                            ))?;
                        Ok(ModelConfigCommandPlan::Publish {
                            prepared_bundle: Box::new(prepared_bundle),
                            expected_models_revision: Revision::new(
                                prepared.models_snapshot.revision,
                            ),
                            reserved_models_revision: None,
                            expected_aura_revision: Some(Revision::new(
                                request.expected_revision.value(),
                            )),
                            reserved_aura_revision: Some(Revision::new(
                                prepared.candidate.aura_config_revision,
                            )),
                            credential_slot_creations: Vec::new(),
                            session_binding_clears: Vec::new(),
                            metadata_events: vec![user_event(
                                "DefaultModelChanged",
                                correlation_id,
                                None,
                                &DefaultModelChangedPayload {
                                    previous_model_profile_id: prepared.previous.model_profile_id,
                                    model_profile_id: prepared.candidate.model_profile_id,
                                    aura_revision: Revision::new(
                                        prepared.candidate.aura_config_revision,
                                    ),
                                },
                            )?],
                            projection_drafts: vec![ModelProjectionDraft::ModelSettings(
                                model_settings,
                            )],
                            success_result: serde_json::to_value(result)?,
                        })
                    })
                    .map_err(|error| journal_error(error, correlation_id))?;
                Ok(P05CommandDispatch {
                    value: completion.success_result,
                    projection_changes: completion.projection_changes,
                })
            }
            model_ipc_method::SESSION_SET_BINDING => {
                let request: SessionModelBindingSetRequest = decode(params, correlation_id)?;
                let _command_invocation_id =
                    require_command_id(command_invocation_id, correlation_id)?;
                let snapshot = self
                    .model_config
                    .snapshot()
                    .map_err(|error| journal_error(error.into(), correlation_id))?;
                let current = self
                    .persistence
                    .session_model_binding_state(request.session_id)
                    .map_err(|error| journal_error(error.into(), correlation_id))?;
                let readable_ref_at_binding = request
                    .model_profile_id
                    .map(|model_profile_id| -> Result<String, ModelConfigJournalError> {
                        let profile = snapshot
                            .registry
                            .models
                            .get(&model_profile_id)
                            .ok_or(ModelConfigError::ProfileNotFound(model_profile_id))?;
                        if !profile.enabled {
                            return Err(ModelConfigError::ProfileDisabled(model_profile_id).into());
                        }
                        if profile.effective_max_output_tokens().is_none() {
                            return Err(ModelConfigError::OutputLimitConfigurationRequired(
                                model_profile_id,
                            )
                            .into());
                        }
                        let endpoint = snapshot
                            .registry
                            .endpoints
                            .get(&profile.endpoint_id)
                            .ok_or(ModelConfigError::EndpointNotFound(model_profile_id))?;
                        let provider = snapshot
                            .registry
                            .providers
                            .get(&endpoint.provider_id)
                            .ok_or(ModelConfigError::ProviderNotFound(model_profile_id))?;
                        let credential_status =
                            self.projections.active_credential_status(endpoint)?;
                        if endpoint.auth_scheme != EndpointAuthScheme::None
                            && !credential_status.as_ref().is_some_and(|status| {
                                status.state == EndpointCredentialState::Configured
                            })
                        {
                            return Err(ModelConfigError::InvalidExecutionCredentialBinding.into());
                        }
                        Ok(format!(
                            "{}/{}/{}",
                            provider.slug, endpoint.slug, profile.provider_model_name
                        ))
                    })
                    .transpose()
                    .map_err(|error| journal_error(error, correlation_id))?;
                let mut selector = self
                    .projections
                    .session_model_selector(&snapshot, request.session_id, current.binding)
                    .map_err(|error| projection_error(error, correlation_id))?;
                selector.selected_model_profile_id = request.model_profile_id;
                selector.selected_model_execution_readiness = request
                    .model_profile_id
                    .and_then(|model_profile_id| {
                        selector
                            .models
                            .iter()
                            .find(|model| model.model_profile_id == model_profile_id)
                            .map(|model| model.execution_readiness.clone())
                    })
                    .unwrap_or_else(|| ModelExecutionReadinessView {
                        is_executable: false,
                        issue_code: Some(
                            NormalizedIssueCode::new("configuration.model_profile_not_selected")
                                .expect("frozen selector issue code"),
                        ),
                    });
                let commit = self
                    .persistence
                    .commit_session_model_binding(CommitSessionModelBinding {
                        session_id: request.session_id,
                        expected_revision: request.expected_revision,
                        model_profile_id: request.model_profile_id,
                        readable_ref_at_binding,
                        bound_at: UtcTimestamp::now(),
                        correlation_id,
                        atomic_group_id: None,
                        selector_projection: selector,
                    })
                    .map_err(|error| journal_error(error.into(), correlation_id))?;
                let result = SessionModelBindingMutationResult {
                    changed: commit.changed,
                    revision: commit.revision,
                    value: commit.binding,
                };
                Ok(P05CommandDispatch {
                    value: encode(result, correlation_id)?,
                    projection_changes: commit.projection_changes,
                })
            }
            model_ipc_method::COMPATIBILITY_RESET => {
                let request: ModelCompatibilityResetRequest = decode(params, correlation_id)?;
                let command_invocation_id =
                    require_command_id(command_invocation_id, correlation_id)?;
                let completion = self
                    .config_journal
                    .execute(command_invocation_id, correlation_id, |journal_id| {
                        let prepared = self.model_config.prepare_update(
                            journal_id.to_string(),
                            request.expected_revision.value(),
                            |registry| {
                                reset_endpoint_compatibility(
                                    registry,
                                    request.endpoint_id,
                                    UtcTimestamp::now(),
                                )
                            },
                        )?;
                        let endpoint = prepared
                            .candidate_registry
                            .endpoints
                            .get(&request.endpoint_id)
                            .ok_or(ModelConfigError::EndpointIdNotFound(request.endpoint_id))?;
                        let result = ModelCompatibilityResetResult {
                            endpoint_id: endpoint.id,
                            changed: prepared.changed,
                            revision: prepared.candidate_revision,
                            compatibility: endpoint.compatibility.clone(),
                        };
                        if !prepared.changed {
                            return Ok(ModelConfigCommandPlan::NoChange {
                                success_result: serde_json::to_value(result)?,
                            });
                        }
                        let previous_compatibility =
                            prepared.value.clone().ok_or(ModelConfigError::InternalInvariant(
                                "changed compatibility reset lacks previous state",
                            ))?;
                        let snapshot = prepared_snapshot(&prepared);
                        let status_overrides = BTreeMap::new();
                        let mut projection_drafts = vec![
                            ModelProjectionDraft::ModelSettings(
                                self.projections
                                    .model_settings(&snapshot, &status_overrides)
                                    .map_err(ModelConfigJournalError::from)?,
                            ),
                            ModelProjectionDraft::EndpointDetail(Box::new(
                                self.projections
                                    .endpoint_detail(
                                        &snapshot,
                                        request.endpoint_id,
                                        &status_overrides,
                                    )
                                    .map_err(ModelConfigJournalError::from)?,
                            )),
                        ];
                        for model_profile_id in &endpoint.model_order {
                            projection_drafts.push(ModelProjectionDraft::ModelProfile(Box::new(
                                self.projections
                                    .model_profile(&snapshot, *model_profile_id, &status_overrides)
                                    .map_err(ModelConfigJournalError::from)?,
                            )));
                        }
                        let prepared_bundle =
                            prepared.prepared_bundle.ok_or(ModelConfigError::InternalInvariant(
                                "changed compatibility reset lacks a prepared bundle",
                            ))?;
                        Ok(ModelConfigCommandPlan::Publish {
                            prepared_bundle: Box::new(prepared_bundle),
                            expected_models_revision: Revision::new(
                                request.expected_revision.value(),
                            ),
                            reserved_models_revision: Some(prepared.candidate_revision),
                            expected_aura_revision: None,
                            reserved_aura_revision: None,
                            credential_slot_creations: Vec::new(),
                            session_binding_clears: Vec::new(),
                            metadata_events: vec![user_event(
                                "ModelCompatibilityReset",
                                correlation_id,
                                None,
                                &ModelCompatibilityResetPayload {
                                    endpoint_id: request.endpoint_id,
                                    previous_compatibility,
                                    models_revision: prepared.candidate_revision,
                                },
                            )?],
                            projection_drafts,
                            success_result: serde_json::to_value(result)?,
                        })
                    })
                    .map_err(|error| journal_error(error, correlation_id))?;
                Ok(P05CommandDispatch {
                    value: completion.success_result,
                    projection_changes: completion.projection_changes,
                })
            }
            model_ipc_method::ENDPOINT_CLEAR_CREDENTIAL => {
                self.dispatch_clear_credential(params, command_invocation_id, correlation_id)
            }
            model_ipc_method::PROVIDER_SET_CONNECTIONS => {
                self.dispatch_provider_connections(params, command_invocation_id, correlation_id)
            }
            model_ipc_method::CONTEXT_WINDOW_CORRECT
            | model_ipc_method::CONTEXT_WINDOW_CLEAR_USER_OVERRIDE
            | model_ipc_method::CONTEXT_WINDOW_RESET_RUNTIME_CORRECTION => self
                .dispatch_context_window_command(
                    method_name,
                    params,
                    command_invocation_id,
                    correlation_id,
                ),
            _ => unreachable!("P05 command dispatch checked the method name"),
        }
    }

    fn current_provider_catalog_projection(
        &self,
        correlation_id: CorrelationId,
    ) -> Result<ProviderCatalogProjection, IpcErrorEnvelope> {
        let change = self
            .persistence
            .latest_model_projection(ProjectionKind::ProviderCatalog, ProjectionScope::Global)
            .map_err(|error| journal_error(error.into(), correlation_id))?;
        match change {
            Some(ProjectionChange::ProviderCatalog { payload, .. }) => Ok(*payload),
            _ => Err(projection_error(
                P05ProjectionError::Unavailable(
                    "the Provider Catalog projection has not been committed".to_owned(),
                ),
                correlation_id,
            )),
        }
    }

    async fn dispatch_delete_endpoint(
        &self,
        params: &Value,
        command_invocation_id: Option<CommandInvocationId>,
        correlation_id: CorrelationId,
    ) -> Result<P05CommandDispatch, IpcErrorEnvelope> {
        let request: ModelEndpointDeleteRequest = decode(params, correlation_id)?;
        let command_invocation_id = require_command_id(command_invocation_id, correlation_id)?;
        let completion = self
            .config_journal
            .execute_with_credential_deletes(
                &self.credential_journal,
                command_invocation_id,
                correlation_id,
                |journal_id| {
                    let previous_default = self.model_config.default_model_selection()?;
                    let prepared = self.model_config.prepare_delete_endpoint(
                        journal_id.to_string(),
                        request.expected_revision.value(),
                        request.endpoint_id,
                    )?;
                    let endpoint = prepared
                        .previous
                        .registry
                        .endpoints
                        .get(&request.endpoint_id)
                        .cloned()
                        .ok_or(ModelConfigError::EndpointIdNotFound(request.endpoint_id))?;
                    let mut deleted_profile_ids =
                        prepared.value.deleted.deleted_model_profile_ids.clone();
                    deleted_profile_ids.sort();
                    let deleted_profile_set =
                        deleted_profile_ids.iter().copied().collect::<BTreeSet<_>>();
                    let bindings = self
                        .persistence
                        .session_model_bindings_for_profiles(&deleted_profile_set)?;
                    let candidate_default = DefaultModelSelection {
                        model_profile_id: if prepared.value.default_model_cleared {
                            None
                        } else {
                            previous_default.model_profile_id
                        },
                        aura_config_revision: prepared.candidate_aura_revision.value(),
                    };
                    let candidate_snapshot = aura_model_config::ModelConfigSnapshot {
                        revision: prepared.candidate_revision.value(),
                        content_hash: prepared.previous.content_hash.clone(),
                        source: prepared.previous.source,
                        registry: prepared.candidate_registry.clone(),
                    };
                    let atomic_group_id = journal_id.to_string();
                    let mut events = Vec::new();
                    let mut session_binding_clears = Vec::new();
                    let mut projection_drafts = Vec::new();
                    for binding in bindings {
                        let next_revision = binding.binding_revision.value().checked_add(1).ok_or(
                            ModelConfigError::InternalInvariant(
                                "Session binding revision is exhausted during Endpoint deletion",
                            ),
                        )?;
                        let mut event = user_event(
                            "SessionModelBindingChanged",
                            correlation_id,
                            Some(atomic_group_id.clone()),
                            &SessionModelBindingChangedPayload {
                                session_id: binding.session_id,
                                previous_binding: Some(binding.clone()),
                                binding: None,
                                binding_revision: Revision::new(next_revision),
                            },
                        )?;
                        event.session_id = Some(binding.session_id);
                        events.push(event);
                        session_binding_clears.push(PreparedSessionModelBindingClear {
                            session_id: binding.session_id,
                            expected_binding_revision: binding.binding_revision,
                            expected_model_profile_id: binding.model_profile_id,
                        });
                        projection_drafts.push(ModelProjectionDraft::SessionModelSelector(
                            self.projections.session_model_selector_with_default(
                                &candidate_snapshot,
                                binding.session_id,
                                None,
                                &candidate_default,
                            )?,
                        ));
                    }
                    if let Some(previous_model_profile_id) = previous_default.model_profile_id
                        && prepared.value.default_model_cleared
                    {
                        events.push(user_event(
                            "DefaultModelChanged",
                            correlation_id,
                            Some(atomic_group_id.clone()),
                            &DefaultModelChangedPayload {
                                previous_model_profile_id: Some(previous_model_profile_id),
                                model_profile_id: None,
                                aura_revision: prepared.candidate_aura_revision,
                            },
                        )?);
                    }
                    for model_profile_id in &deleted_profile_ids {
                        let profile = prepared
                            .previous
                            .registry
                            .models
                            .get(model_profile_id)
                            .ok_or(ModelConfigError::ProfileNotFound(*model_profile_id))?;
                        events.push(user_event(
                            "ModelProfileDeleted",
                            correlation_id,
                            Some(atomic_group_id.clone()),
                            &ModelProfileDeletedPayload {
                                provider_id: endpoint.provider_id,
                                endpoint_id: endpoint.id,
                                model_profile_id: *model_profile_id,
                                default_model_cleared: previous_default.model_profile_id
                                    == Some(profile.id),
                                models_revision: prepared.candidate_revision,
                            },
                        )?);
                    }
                    let credential_record = self.persistence.model_credential_record_for_endpoint(
                        endpoint.id,
                        ModelCredentialSlot::ApiKey,
                    )?;
                    events.push(user_event(
                        "ModelEndpointDeleted",
                        correlation_id,
                        Some(atomic_group_id),
                        &ModelEndpointDeletedPayload {
                            endpoint_id: endpoint.id,
                            provider_id: endpoint.provider_id,
                            deleted_model_profile_ids: deleted_profile_ids,
                            credential_was_configured: credential_record.as_ref().is_some_and(
                                |record| record.state == ModelCredentialRecordState::Configured,
                            ),
                            models_revision: prepared.candidate_revision,
                        },
                    )?);
                    projection_drafts.insert(
                        0,
                        ModelProjectionDraft::ModelSettings(
                            self.projections.model_settings_with_default(
                                &candidate_snapshot,
                                &BTreeMap::new(),
                                &candidate_default,
                            )?,
                        ),
                    );
                    let credential_ids = credential_record
                        .map(|record| vec![record.credential_id])
                        .unwrap_or_default();
                    Ok(ModelConfigCredentialDeleteCommandPlan {
                        prepared_bundle: Box::new(prepared.prepared_bundle),
                        expected_models_revision: Revision::new(request.expected_revision.value()),
                        reserved_models_revision: prepared.candidate_revision,
                        expected_aura_revision: Some(prepared.expected_aura_revision),
                        reserved_aura_revision: prepared
                            .value
                            .default_model_cleared
                            .then_some(prepared.candidate_aura_revision),
                        credential_ids,
                        deletion_reason: ModelCredentialDeletionReason::EndpointDeleted,
                        session_binding_clears,
                        metadata_events: events,
                        projection_drafts,
                        success_result: serde_json::to_value(ModelDeletionMutationResult {
                            changed: true,
                            revision: prepared.candidate_revision,
                            value: prepared.value,
                        })?,
                    })
                },
            )
            .map_err(|error| journal_error(error, correlation_id))?;
        let endpoint_ids = BTreeSet::from([request.endpoint_id]);
        let mut projection_changes = completion.projection_changes;
        match self.cancel_model_discovery_for_deleted_endpoints(&endpoint_ids).await {
            Ok(changes) => projection_changes.extend(changes),
            Err(error) => aura_core_base::append_core_log(
                "p05_target_delete_operation_cancellation_failed",
                serde_json::json!({
                    "target_kind": "endpoint",
                    "target_id": request.endpoint_id,
                    "operation_kind": "model_discovery",
                    "failure_class": discovery_failure_class(&error),
                }),
            ),
        }
        match self
            .cancel_model_availability_for_deleted_targets(&endpoint_ids, &BTreeSet::new())
            .await
        {
            Ok(changes) => projection_changes.extend(changes),
            Err(error) => aura_core_base::append_core_log(
                "p05_target_delete_operation_cancellation_failed",
                serde_json::json!({
                    "target_kind": "endpoint",
                    "target_id": request.endpoint_id,
                    "operation_kind": "model_availability_test",
                    "failure_class": availability_failure_class(&error),
                }),
            ),
        }
        Ok(P05CommandDispatch { value: completion.success_result, projection_changes })
    }

    async fn dispatch_delete_provider(
        &self,
        params: &Value,
        command_invocation_id: Option<CommandInvocationId>,
        correlation_id: CorrelationId,
    ) -> Result<P05CommandDispatch, IpcErrorEnvelope> {
        let request: ModelProviderDeleteRequest = decode(params, correlation_id)?;
        let command_invocation_id = require_command_id(command_invocation_id, correlation_id)?;
        let provider_endpoint_ids = self
            .model_config
            .snapshot()
            .map_err(|error| journal_error(error.into(), correlation_id))?
            .registry
            .providers
            .get(&request.provider_id)
            .map(|provider| provider.endpoint_order.iter().copied().collect::<BTreeSet<_>>())
            .unwrap_or_default();
        let completion = self
            .config_journal
            .execute_with_credential_deletes(
                &self.credential_journal,
                command_invocation_id,
                correlation_id,
                |journal_id| {
                    let previous_default = self.model_config.default_model_selection()?;
                    let prepared = self.model_config.prepare_delete_provider(
                        journal_id.to_string(),
                        request.expected_revision.value(),
                        request.provider_id,
                    )?;
                    let provider = prepared
                        .previous
                        .registry
                        .providers
                        .get(&request.provider_id)
                        .cloned()
                        .ok_or(ModelConfigError::ProviderNotFoundById(request.provider_id))?;
                    let mut deleted_endpoint_ids =
                        prepared.value.deleted.deleted_endpoint_ids.clone();
                    deleted_endpoint_ids.sort();
                    let mut deleted_profile_ids =
                        prepared.value.deleted.deleted_model_profile_ids.clone();
                    deleted_profile_ids.sort();
                    let deleted_profile_set =
                        deleted_profile_ids.iter().copied().collect::<BTreeSet<_>>();
                    let bindings = self
                        .persistence
                        .session_model_bindings_for_profiles(&deleted_profile_set)?;
                    let candidate_default = DefaultModelSelection {
                        model_profile_id: if prepared.value.default_model_cleared {
                            None
                        } else {
                            previous_default.model_profile_id
                        },
                        aura_config_revision: prepared.candidate_aura_revision.value(),
                    };
                    let candidate_snapshot = aura_model_config::ModelConfigSnapshot {
                        revision: prepared.candidate_revision.value(),
                        content_hash: prepared.previous.content_hash.clone(),
                        source: prepared.previous.source,
                        registry: prepared.candidate_registry.clone(),
                    };
                    let atomic_group_id = journal_id.to_string();
                    let mut events = Vec::new();
                    let mut session_binding_clears = Vec::new();
                    let mut projection_drafts = Vec::new();
                    for binding in bindings {
                        let next_revision = binding.binding_revision.value().checked_add(1).ok_or(
                            ModelConfigError::InternalInvariant(
                                "Session binding revision is exhausted during Provider deletion",
                            ),
                        )?;
                        let mut event = user_event(
                            "SessionModelBindingChanged",
                            correlation_id,
                            Some(atomic_group_id.clone()),
                            &SessionModelBindingChangedPayload {
                                session_id: binding.session_id,
                                previous_binding: Some(binding.clone()),
                                binding: None,
                                binding_revision: Revision::new(next_revision),
                            },
                        )?;
                        event.session_id = Some(binding.session_id);
                        events.push(event);
                        session_binding_clears.push(PreparedSessionModelBindingClear {
                            session_id: binding.session_id,
                            expected_binding_revision: binding.binding_revision,
                            expected_model_profile_id: binding.model_profile_id,
                        });
                        projection_drafts.push(ModelProjectionDraft::SessionModelSelector(
                            self.projections.session_model_selector_with_default(
                                &candidate_snapshot,
                                binding.session_id,
                                None,
                                &candidate_default,
                            )?,
                        ));
                    }
                    if let Some(previous_model_profile_id) = previous_default.model_profile_id
                        && prepared.value.default_model_cleared
                    {
                        events.push(user_event(
                            "DefaultModelChanged",
                            correlation_id,
                            Some(atomic_group_id.clone()),
                            &DefaultModelChangedPayload {
                                previous_model_profile_id: Some(previous_model_profile_id),
                                model_profile_id: None,
                                aura_revision: prepared.candidate_aura_revision,
                            },
                        )?);
                    }
                    for model_profile_id in &deleted_profile_ids {
                        let profile = prepared
                            .previous
                            .registry
                            .models
                            .get(model_profile_id)
                            .ok_or(ModelConfigError::ProfileNotFound(*model_profile_id))?;
                        events.push(user_event(
                            "ModelProfileDeleted",
                            correlation_id,
                            Some(atomic_group_id.clone()),
                            &ModelProfileDeletedPayload {
                                provider_id: provider.id,
                                endpoint_id: profile.endpoint_id,
                                model_profile_id: *model_profile_id,
                                default_model_cleared: previous_default.model_profile_id
                                    == Some(profile.id),
                                models_revision: prepared.candidate_revision,
                            },
                        )?);
                    }
                    let mut credential_ids = Vec::new();
                    for endpoint_id in &deleted_endpoint_ids {
                        let endpoint = prepared
                            .previous
                            .registry
                            .endpoints
                            .get(endpoint_id)
                            .ok_or(ModelConfigError::EndpointIdNotFound(*endpoint_id))?;
                        let credential_record =
                            self.persistence.model_credential_record_for_endpoint(
                                *endpoint_id,
                                ModelCredentialSlot::ApiKey,
                            )?;
                        if let Some(record) = &credential_record {
                            credential_ids.push(record.credential_id);
                        }
                        let mut endpoint_profiles = endpoint.model_order.clone();
                        endpoint_profiles.sort();
                        events.push(user_event(
                            "ModelEndpointDeleted",
                            correlation_id,
                            Some(atomic_group_id.clone()),
                            &ModelEndpointDeletedPayload {
                                endpoint_id: *endpoint_id,
                                provider_id: provider.id,
                                deleted_model_profile_ids: endpoint_profiles,
                                credential_was_configured: credential_record.as_ref().is_some_and(
                                    |record| record.state == ModelCredentialRecordState::Configured,
                                ),
                                models_revision: prepared.candidate_revision,
                            },
                        )?);
                    }
                    events.push(user_event(
                        "ModelProviderDeleted",
                        correlation_id,
                        Some(atomic_group_id),
                        &ModelProviderDeletedPayload {
                            provider_id: provider.id,
                            deleted_endpoint_ids,
                            deleted_model_profile_ids: deleted_profile_ids,
                            default_model_cleared: prepared.value.default_model_cleared,
                            models_revision: prepared.candidate_revision,
                        },
                    )?);
                    projection_drafts.insert(
                        0,
                        ModelProjectionDraft::ModelSettings(
                            self.projections.model_settings_with_default(
                                &candidate_snapshot,
                                &BTreeMap::new(),
                                &candidate_default,
                            )?,
                        ),
                    );
                    Ok(ModelConfigCredentialDeleteCommandPlan {
                        prepared_bundle: Box::new(prepared.prepared_bundle),
                        expected_models_revision: Revision::new(request.expected_revision.value()),
                        reserved_models_revision: prepared.candidate_revision,
                        expected_aura_revision: Some(prepared.expected_aura_revision),
                        reserved_aura_revision: prepared
                            .value
                            .default_model_cleared
                            .then_some(prepared.candidate_aura_revision),
                        credential_ids,
                        deletion_reason: ModelCredentialDeletionReason::ProviderDeleted,
                        session_binding_clears,
                        metadata_events: events,
                        projection_drafts,
                        success_result: serde_json::to_value(ModelDeletionMutationResult {
                            changed: true,
                            revision: prepared.candidate_revision,
                            value: prepared.value,
                        })?,
                    })
                },
            )
            .map_err(|error| journal_error(error, correlation_id))?;
        let mut projection_changes = completion.projection_changes;
        match self.cancel_model_discovery_for_deleted_endpoints(&provider_endpoint_ids).await {
            Ok(changes) => projection_changes.extend(changes),
            Err(error) => aura_core_base::append_core_log(
                "p05_target_delete_operation_cancellation_failed",
                serde_json::json!({
                    "target_kind": "provider",
                    "target_id": request.provider_id,
                    "operation_kind": "model_discovery",
                    "failure_class": discovery_failure_class(&error),
                }),
            ),
        }
        match self
            .cancel_model_availability_for_deleted_targets(&provider_endpoint_ids, &BTreeSet::new())
            .await
        {
            Ok(changes) => projection_changes.extend(changes),
            Err(error) => aura_core_base::append_core_log(
                "p05_target_delete_operation_cancellation_failed",
                serde_json::json!({
                    "target_kind": "provider",
                    "target_id": request.provider_id,
                    "operation_kind": "model_availability_test",
                    "failure_class": availability_failure_class(&error),
                }),
            ),
        }
        Ok(P05CommandDispatch { value: completion.success_result, projection_changes })
    }

    fn prepare_endpoint_credential_slot(
        &self,
        endpoint: &aura_model_domain::ModelEndpointConfig,
    ) -> Result<
        (Vec<CreateModelCredentialSlot>, Option<EndpointCredentialStatus>),
        ModelConfigJournalError,
    > {
        if endpoint.auth_scheme == EndpointAuthScheme::None {
            return Ok((Vec::new(), None));
        }
        let credential_id = CredentialId::new();
        let binding = self.credential_storage_binding(endpoint, credential_id)?;
        let status = EndpointCredentialStatus {
            endpoint_id: endpoint.id,
            slot_name: ModelCredentialSlot::ApiKey,
            credential_id,
            state: EndpointCredentialState::Unconfigured,
            active_credential_revision: None,
            last_committed_credential_revision: None,
            row_revision: Revision::INITIAL,
            error_code: None,
        };
        status.validate().map_err(|_| {
            ModelConfigJournalError::ModelConfig(ModelConfigError::InternalInvariant(
                "new credential slot has an invalid Renderer status",
            ))
        })?;
        Ok((
            vec![CreateModelCredentialSlot { credential_id, endpoint_id: endpoint.id, binding }],
            Some(status),
        ))
    }

    pub(crate) fn credential_storage_binding(
        &self,
        endpoint: &aura_model_domain::ModelEndpointConfig,
        credential_id: CredentialId,
    ) -> Result<ModelCredentialStorageBinding, ModelConfigJournalError> {
        let credential_slot = ModelCredentialSlot::ApiKey;
        let base_url = normalize_base_url(&endpoint.base_url).map_err(ModelConfigError::from)?;
        let normalized_origin = NormalizedOrigin {
            scheme: base_url.scheme().to_owned(),
            host: base_url
                .host_str()
                .ok_or(ModelConfigError::InternalInvariant(
                    "validated model Endpoint lacks an origin host",
                ))?
                .to_ascii_lowercase(),
            port: base_url.port_or_known_default().ok_or(ModelConfigError::InternalInvariant(
                "validated model Endpoint lacks an effective origin port",
            ))?,
        };
        Ok(ModelCredentialStorageBinding {
            credential_ref: credential_ref_for_credential(&self.bundle_namespace, credential_id)
                .map_err(|_| {
                    ModelConfigError::InternalInvariant(
                        "Core-generated model CredentialRef is invalid",
                    )
                })?,
            credential_binding_fingerprint: credential_binding_fingerprint(
                &self.bundle_namespace,
                credential_id,
                credential_slot,
            )
            .map_err(ModelConfigError::from)?,
            bundle_namespace_key: self.bundle_namespace.object_name_hash(),
            backend_kind: "wincred".to_owned(),
            wincred_type: "generic".to_owned(),
            final_target: model_credential_final_target(&self.bundle_namespace, credential_id),
            normalized_origin,
            auth_scheme: endpoint.auth_scheme,
            credential_slot,
        })
    }
}

pub(super) fn profile_changed_fields(
    previous: &aura_model_domain::ModelProfileConfig,
    current: &aura_model_domain::ModelProfileConfig,
) -> BTreeSet<String> {
    let mut changed = BTreeSet::new();
    if previous.display_name != current.display_name {
        changed.insert("display_name".to_owned());
    }
    if previous.enabled != current.enabled {
        changed.insert("enabled".to_owned());
    }
    if previous.available_for_subagents != current.available_for_subagents {
        changed.insert("available_for_subagents".to_owned());
    }
    if previous.catalog != current.catalog {
        changed.insert("catalog".to_owned());
    }
    if previous.request_defaults != current.request_defaults {
        changed.insert("request_defaults".to_owned());
    }
    if previous.context != current.context {
        changed.insert("context".to_owned());
    }
    changed
}

pub(super) fn endpoint_changed_fields(
    previous: &aura_model_domain::ModelEndpointConfig,
    current: &aura_model_domain::ModelEndpointConfig,
) -> BTreeSet<String> {
    let mut changed = BTreeSet::new();
    macro_rules! track {
        ($field:ident) => {
            if previous.$field != current.$field {
                changed.insert(stringify!($field).to_owned());
            }
        };
    }
    track!(display_name);
    track!(api_format);
    track!(base_url);
    track!(discovery_mode);
    track!(chat_output_token_field);
    track!(chat_instruction_role);
    track!(auth_scheme);
    track!(openai_organization_id);
    track!(openai_project_id);
    track!(anthropic_workspace_id);
    track!(timeouts);
    track!(max_parallel_model_requests);
    track!(model_order);
    track!(compatibility);
    changed
}

pub(super) fn prepared_snapshot<T>(
    prepared: &aura_model_config::PreparedModelConfigMutation<T>,
) -> aura_model_config::ModelConfigSnapshot {
    aura_model_config::ModelConfigSnapshot {
        revision: prepared.candidate_revision.value(),
        content_hash: prepared.previous.content_hash.clone(),
        source: prepared.previous.source,
        registry: prepared.candidate_registry.clone(),
    }
}

pub(super) fn user_event<T: serde::Serialize>(
    event_type: &str,
    correlation_id: CorrelationId,
    atomic_group_id: Option<String>,
    payload: &T,
) -> Result<PersistentEventDraft, ModelConfigJournalError> {
    Ok(PersistentEventDraft {
        event_id: EventId::new(),
        workspace_id: None,
        session_id: None,
        turn_task_id: None,
        agent_run_id: None,
        event_type: event_type.to_owned(),
        event_version: 1,
        actor: EventActor::User,
        correlation_id,
        causation_id: None,
        atomic_group_id,
        payload_json: Some(serde_json::to_value(payload)?),
        payload_artifact_ref: None,
        searchable_text: None,
        created_at: UtcTimestamp::now(),
    })
}

impl P05Services {
    fn spawn_model_availability_supervisor(
        &self,
        prepared: PreparedModelAvailabilityTest,
        identity: UserConfigurationOperationIdentity,
        correlation_id: CorrelationId,
        operation_id: ModelAvailabilityTestId,
        control: P05OperationControl,
    ) {
        let services = self.clone();
        drop(tokio::spawn(async move {
            let worker_services = services.clone();
            let worker_control = control.clone();
            let worker = tokio::spawn(async move {
                worker_services
                    .execute_model_availability_test(
                        prepared,
                        identity,
                        correlation_id,
                        worker_control,
                    )
                    .await
            });
            let result = match worker.await {
                Ok(result) => result,
                Err(_) => Err(ModelAvailabilityServiceError::WorkerTerminated),
            };
            let completion = match result {
                Ok(completion) => completion,
                Err(error) => {
                    let mut retry_delay_ms = 25_u64;
                    loop {
                        match services.terminalize_model_availability_error(
                            operation_id,
                            &error,
                            control.cancellation_reason(),
                        ) {
                            Ok(completion) => break completion,
                            Err(terminalization_error) => {
                                aura_core_base::append_core_log(
                                    "p05_operation_terminalization_retry",
                                    serde_json::json!({
                                        "operation_kind": "model_availability_test",
                                        "operation_id": operation_id,
                                        "failure_class": availability_failure_class(
                                            &terminalization_error,
                                        ),
                                    }),
                                );
                                tokio::time::sleep(Duration::from_millis(retry_delay_ms)).await;
                                retry_delay_ms = retry_delay_ms.saturating_mul(2).min(1_000);
                            }
                        }
                    }
                }
            };
            let mut retry_delay_ms = 25_u64;
            loop {
                match services
                    .persistence
                    .publish_model_projection_changes(&completion.projection_changes)
                {
                    Ok(()) => break,
                    Err(_) => {
                        aura_core_base::append_core_log(
                            "p05_projection_publish_retry",
                            serde_json::json!({
                                "operation_kind": "model_availability_test",
                                "operation_id": operation_id,
                            }),
                        );
                        tokio::time::sleep(Duration::from_millis(retry_delay_ms)).await;
                        retry_delay_ms = retry_delay_ms.saturating_mul(2).min(1_000);
                    }
                }
            }
            control.mark_terminal_projection_published();
            services.operation_cancellations.remove_availability(operation_id);
        }));
    }

    fn spawn_model_discovery_supervisor(
        &self,
        prepared: PreparedModelDiscovery,
        identity: UserConfigurationOperationIdentity,
        correlation_id: CorrelationId,
        operation_id: ModelDiscoveryRunId,
        control: P05OperationControl,
    ) {
        let services = self.clone();
        drop(tokio::spawn(async move {
            let worker_services = services.clone();
            let worker_control = control.clone();
            let worker = tokio::spawn(async move {
                worker_services
                    .execute_model_discovery(prepared, identity, correlation_id, worker_control)
                    .await
            });
            let result = match worker.await {
                Ok(result) => result,
                Err(_) => Err(ModelDiscoveryServiceError::WorkerTerminated),
            };
            let completion = match result {
                Ok(completion) => completion,
                Err(error) => {
                    let mut retry_delay_ms = 25_u64;
                    loop {
                        match services.terminalize_model_discovery_error(
                            operation_id,
                            &error,
                            control.cancellation_reason(),
                        ) {
                            Ok(completion) => break completion,
                            Err(terminalization_error) => {
                                aura_core_base::append_core_log(
                                    "p05_operation_terminalization_retry",
                                    serde_json::json!({
                                        "operation_kind": "model_discovery",
                                        "operation_id": operation_id,
                                        "failure_class": discovery_failure_class(
                                            &terminalization_error,
                                        ),
                                    }),
                                );
                                tokio::time::sleep(Duration::from_millis(retry_delay_ms)).await;
                                retry_delay_ms = retry_delay_ms.saturating_mul(2).min(1_000);
                            }
                        }
                    }
                }
            };
            let mut retry_delay_ms = 25_u64;
            loop {
                match services
                    .persistence
                    .publish_model_projection_changes(&completion.projection_changes)
                {
                    Ok(()) => break,
                    Err(_) => {
                        aura_core_base::append_core_log(
                            "p05_projection_publish_retry",
                            serde_json::json!({
                                "operation_kind": "model_discovery",
                                "operation_id": operation_id,
                            }),
                        );
                        tokio::time::sleep(Duration::from_millis(retry_delay_ms)).await;
                        retry_delay_ms = retry_delay_ms.saturating_mul(2).min(1_000);
                    }
                }
            }
            control.mark_terminal_projection_published();
            services.operation_cancellations.remove_discovery(operation_id);
        }));
    }
}

fn availability_failure_class(error: &ModelAvailabilityServiceError) -> &'static str {
    match error {
        ModelAvailabilityServiceError::Persistence(_)
        | ModelAvailabilityServiceError::ConfigJournal(_) => "persistence",
        ModelAvailabilityServiceError::Transport(_) => "transport",
        ModelAvailabilityServiceError::Adapter(_) => "adapter",
        ModelAvailabilityServiceError::Prerequisite(_) => "prerequisite",
        ModelAvailabilityServiceError::OutputTokenFieldAliasConflict => "compatibility",
        ModelAvailabilityServiceError::RevisionOverflow
        | ModelAvailabilityServiceError::MissingProjection
        | ModelAvailabilityServiceError::OperationNotFound
        | ModelAvailabilityServiceError::WorkerTerminated
        | ModelAvailabilityServiceError::OperationRevisionConflict { .. } => "internal",
    }
}

fn discovery_failure_class(error: &ModelDiscoveryServiceError) -> &'static str {
    match error {
        ModelDiscoveryServiceError::Persistence(_)
        | ModelDiscoveryServiceError::ConfigJournal(_) => "persistence",
        ModelDiscoveryServiceError::Transport(_) => "transport",
        ModelDiscoveryServiceError::CredentialUnavailable => "credential",
        ModelDiscoveryServiceError::InvalidConfiguration
        | ModelDiscoveryServiceError::ConfigurationRevisionChanged
        | ModelDiscoveryServiceError::ModelConfigMutationInProgress => "configuration",
        ModelDiscoveryServiceError::RevisionOverflow
        | ModelDiscoveryServiceError::MissingProjection
        | ModelDiscoveryServiceError::OperationNotFound
        | ModelDiscoveryServiceError::WorkerTerminated
        | ModelDiscoveryServiceError::OperationRevisionConflict { .. } => "internal",
    }
}

fn decode<T: DeserializeOwned>(
    params: &Value,
    correlation_id: CorrelationId,
) -> Result<T, IpcErrorEnvelope> {
    serde_json::from_value(params.clone()).map_err(|_| IpcErrorEnvelope {
        code: ErrorCode::new("models.command.invalid_params").expect("static ErrorCode is valid"),
        category: ErrorCategory::Validation,
        message: "模型设置请求的参数无效，没有做任何修改。请重新打开模型设置后再试。".to_owned(),
        retryable: false,
        correlation_id,
        terminal: IpcRequestTerminal::Failed,
        details: BTreeMap::new(),
    })
}

fn encode<T: serde::Serialize>(
    value: T,
    correlation_id: CorrelationId,
) -> Result<Value, IpcErrorEnvelope> {
    serde_json::to_value(value).map_err(|_| IpcErrorEnvelope {
        code: ErrorCode::new("models.command.response_encoding_failed")
            .expect("static ErrorCode is valid"),
        category: ErrorCategory::Internal,
        message: "模型设置的结果无法返回界面。请重新打开模型设置查看当前状态。".to_owned(),
        retryable: false,
        correlation_id,
        terminal: IpcRequestTerminal::Failed,
        details: BTreeMap::new(),
    })
}

fn require_command_id(
    command_invocation_id: Option<CommandInvocationId>,
    correlation_id: CorrelationId,
) -> Result<CommandInvocationId, IpcErrorEnvelope> {
    command_invocation_id.ok_or_else(|| IpcErrorEnvelope {
        code: ErrorCode::new("ipc.command_invocation_id_required")
            .expect("static ErrorCode is valid"),
        category: ErrorCategory::Validation,
        message: "这次请求缺少操作编号，Aura Core 没有执行它。请重新打开设置后再试。".to_owned(),
        retryable: false,
        correlation_id,
        terminal: IpcRequestTerminal::Failed,
        details: BTreeMap::new(),
    })
}

fn catalog_error(error: P05CatalogError, correlation_id: CorrelationId) -> IpcErrorEnvelope {
    if let P05CatalogError::Catalog(aura_model_catalog::CatalogError::RevisionConflict {
        expected,
        actual,
    }) = error
    {
        return operation_revision_error(
            "catalog.revision_conflict",
            "服务商目录已经更新，请重新打开目录后再操作。",
            expected,
            actual,
            correlation_id,
        );
    }
    if matches!(
        error,
        P05CatalogError::Catalog(aura_model_catalog::CatalogError::Network(
            aura_network_control::NetworkControlError::ResponseTooLarge
        ))
    ) {
        return IpcErrorEnvelope {
            code: ErrorCode::new("provider.catalog_response_too_large")
                .expect("static ErrorCode is valid"),
            category: ErrorCategory::Validation,
            message: "服务商目录超过 5 MiB 的大小上限，没有采用；继续使用上一份可用的目录。"
                .to_owned(),
            retryable: false,
            correlation_id,
            terminal: IpcRequestTerminal::Failed,
            details: BTreeMap::new(),
        };
    }
    let retryable = true;
    IpcErrorEnvelope {
        code: ErrorCode::new(if retryable {
            "provider.catalog_unavailable"
        } else {
            "internal.unexpected"
        })
        .expect("static ErrorCode is valid"),
        category: if retryable { ErrorCategory::Unavailable } else { ErrorCategory::Internal },
        message: "服务商目录暂时取不到；已保存的模型配置照常可用，稍后可以再刷新目录。".to_owned(),
        retryable,
        correlation_id,
        terminal: IpcRequestTerminal::Failed,
        details: BTreeMap::new(),
    }
}

fn provider_icon_error(correlation_id: CorrelationId) -> IpcErrorEnvelope {
    IpcErrorEnvelope {
        code: ErrorCode::new("provider.icon_unavailable").expect("static ErrorCode is valid"),
        category: ErrorCategory::NotFound,
        message: "服务商图标暂时不可用。".to_owned(),
        retryable: true,
        correlation_id,
        terminal: IpcRequestTerminal::Failed,
        details: BTreeMap::new(),
    }
}

pub(super) fn journal_error(
    error: ModelConfigJournalError,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    #[cfg(any(test, feature = "test-hooks"))]
    eprintln!("P05 model config command failed: {error:?}");
    let (code, category, message, retryable, details) = match error {
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::MissingChatOutputTokenField { .. },
        )) => (
            "models.endpoint.chat_output_token_field_required",
            ErrorCategory::Validation,
            "OpenAI Chat Completions 连接必须选择 Output Token Field。",
            false,
            endpoint_field_details("chat_output_token_field"),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::MissingChatInstructionRole { .. },
        )) => (
            "models.endpoint.chat_instruction_role_required",
            ErrorCategory::Validation,
            "OpenAI Chat Completions 连接必须选择 Chat Instruction Role。",
            false,
            endpoint_field_details("chat_instruction_role"),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::InvalidRoutingField { field, .. },
        )) => (
            "models.endpoint.field_not_allowed",
            ErrorCategory::Validation,
            "该字段不适用于所选的 API 类型，请核对后重新保存。",
            false,
            endpoint_field_details(field),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::InvalidAuthScheme { .. },
        )) => (
            "models.endpoint.invalid_auth_scheme",
            ErrorCategory::Validation,
            "当前认证方式不适用于所选的 API 类型。",
            false,
            endpoint_field_details("auth_scheme"),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::InvalidBaseUrl { .. },
        )) => (
            "models.endpoint.invalid_base_url",
            ErrorCategory::Validation,
            "Base URL 无效，请填写不含用户信息、查询参数或片段的有效地址。",
            false,
            endpoint_field_details("base_url"),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::InvalidModelsPath { .. },
        )) => (
            "models.endpoint.invalid_models_path",
            ErrorCategory::Validation,
            "模型目录请求路径无效，请填写同一地址下的 HTTP 路径。",
            false,
            endpoint_field_details("models_path"),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::InvalidTimeout { field, .. },
        )) => (
            "models.endpoint.invalid_timeout",
            ErrorCategory::Validation,
            "超时必须是大于 0 的整数。",
            false,
            endpoint_field_details(field),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::InvalidParallelLimit { .. },
        )) => (
            "models.endpoint.invalid_parallel_limit",
            ErrorCategory::Validation,
            "并发请求上限必须是大于 0 的整数。",
            false,
            endpoint_field_details("max_parallel_model_requests"),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::BlankValue { path },
        )) if path == "endpoint.display_name" => (
            "models.endpoint.invalid_display_name",
            ErrorCategory::Validation,
            "请输入连接名称。",
            false,
            endpoint_field_details("display_name"),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::BlankValue { path },
        )) if path == "endpoint.base_url" => (
            "models.endpoint.invalid_base_url",
            ErrorCategory::Validation,
            "Base URL 无效，请填写不含用户信息、查询参数或片段的有效地址。",
            false,
            endpoint_field_details("base_url"),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::BlankValue { path },
        )) if path.starts_with("endpoint.") => (
            "models.endpoint.invalid_field",
            ErrorCategory::Validation,
            "有一个连接字段只有空白，请填写内容或清空后再保存。",
            false,
            endpoint_field_details(path.trim_start_matches("endpoint.")),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::ProviderNotFoundById(_)) => (
            "models.provider.not_found",
            ErrorCategory::NotFound,
            "这个服务商已不存在，请刷新模型设置。",
            false,
            BTreeMap::new(),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::DuplicateProviderModelName {
            ..
        }) => (
            "models.profile.already_exists",
            ErrorCategory::Conflict,
            "这个连接下已经添加过同一个模型。",
            false,
            BTreeMap::new(),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Settings(
            aura_config::SettingsFacadeError::RevisionConflict { .. },
        ))
        | ModelConfigJournalError::ModelConfig(
            ModelConfigError::EndpointObjectRevisionConflict { .. }
            | ModelConfigError::ProfileObjectRevisionConflict { .. },
        )
        | ModelConfigJournalError::Persistence(
            aura_persistence::PersistenceError::SessionModelBindingRevisionConflict,
        ) => (
            "models.configuration.revision_conflict",
            ErrorCategory::Conflict,
            "模型设置已在别处更新，请刷新后再试。",
            true,
            BTreeMap::new(),
        ),
        ModelConfigJournalError::Aborted(_) => (
            "models.configuration.aborted",
            ErrorCategory::Internal,
            "这次修改在生效前被中止，模型设置没有改变，请重试。",
            false,
            BTreeMap::new(),
        ),
        // Binding refusals name their real reason; the catch-all below would leave
        // the user with an unexplained "could not be committed" and no next step.
        ModelConfigJournalError::ModelConfig(
            ModelConfigError::InvalidExecutionCredentialBinding,
        ) => (
            "models.binding.credential_required",
            ErrorCategory::Permission,
            "这个模型所在的连接还没有可用的凭据，暂时不能选用；请在模型设置里保存 API Key 后再试。",
            false,
            BTreeMap::new(),
        ),
        ModelConfigJournalError::ModelConfig(
            ModelConfigError::OutputLimitConfigurationRequired(_),
        ) => (
            "models.binding.output_limit_required",
            ErrorCategory::Validation,
            "这个模型需要先设置最大输出 Token 上限，才能选用。",
            false,
            BTreeMap::new(),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::ProfileDisabled(_)) => (
            "models.binding.profile_disabled",
            ErrorCategory::Conflict,
            "这个模型已停用，请在模型设置中启用或选择其他模型。",
            false,
            BTreeMap::new(),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::ProfileNotFound(_)) => (
            "models.binding.profile_missing",
            ErrorCategory::NotFound,
            "这个模型已不存在，请在模型设置中重新选择。",
            false,
            BTreeMap::new(),
        ),
        ModelConfigJournalError::ModelConfig(
            ModelConfigError::EndpointNotFound(_) | ModelConfigError::ProviderNotFound(_),
        ) => (
            "models.binding.endpoint_missing",
            ErrorCategory::NotFound,
            "这个模型所属的连接已不存在，请在模型设置中重新配置。",
            false,
            BTreeMap::new(),
        ),
        ModelConfigJournalError::ReconciliationRequired(_) => (
            "persistence.reconciliation_required",
            ErrorCategory::Unavailable,
            "模型设置需要先完成恢复才能修改，请到设置中查看恢复选项。",
            false,
            BTreeMap::new(),
        ),
        _ => (
            "models.configuration.failed",
            ErrorCategory::Internal,
            "模型设置没有保存成功，配置保持不变；请重试，仍不行请查看本机日志。",
            false,
            BTreeMap::new(),
        ),
    };
    IpcErrorEnvelope {
        code: ErrorCode::new(code).expect("static ErrorCode is valid"),
        category,
        message: message.to_owned(),
        retryable,
        correlation_id,
        terminal: IpcRequestTerminal::Failed,
        details,
    }
}

fn endpoint_field_details(field: &str) -> BTreeMap<String, Value> {
    BTreeMap::from([("field".to_owned(), Value::String(field.to_owned()))])
}

fn availability_error(
    error: ModelAvailabilityServiceError,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    if let ModelAvailabilityServiceError::ConfigJournal(error) = error {
        return journal_error(error, correlation_id);
    }
    if let ModelAvailabilityServiceError::OperationRevisionConflict { expected, actual } = error {
        return operation_revision_error(
            "operation.revision_conflict",
            "可用性测试的状态已经变化，请刷新后再取消。",
            expected,
            actual,
            correlation_id,
        );
    }
    let (code, category, message, retryable, details) = match error {
        ModelAvailabilityServiceError::Prerequisite(reason) => {
            let mut details = BTreeMap::new();
            details.insert(
                "reason".to_owned(),
                serde_json::to_value(reason).expect("availability prerequisite reason serializes"),
            );
            (
                "configuration.availability_test_prerequisite_missing",
                ErrorCategory::Validation,
                "保存的模型配置还不完整，暂时不能测试可用性；请先补全连接与凭据。",
                false,
                details,
            )
        }
        ModelAvailabilityServiceError::Persistence(_) => (
            "persistence.operation_failed",
            ErrorCategory::Unavailable,
            "可用性测试没能记录下来，测试没有开始；请重试。",
            true,
            BTreeMap::new(),
        ),
        ModelAvailabilityServiceError::Transport(_) => (
            "transport.operation_failed",
            ErrorCategory::Unavailable,
            "可用性测试的网络请求没能开始；请检查网络与代理设置后重试。",
            true,
            BTreeMap::new(),
        ),
        ModelAvailabilityServiceError::Adapter(_) => (
            "provider.request_compilation_failed",
            ErrorCategory::Internal,
            "兼容性重试的请求没能生成，测试已停止；请重试。",
            false,
            BTreeMap::new(),
        ),
        ModelAvailabilityServiceError::OutputTokenFieldAliasConflict => (
            "compatibility.output_token_field_conflict",
            ErrorCategory::Conflict,
            "测出的输出 Token 字段与当前的兼容设置冲突，没有采用；请刷新后再测。",
            false,
            BTreeMap::new(),
        ),
        ModelAvailabilityServiceError::RevisionOverflow => (
            "persistence.revision_overflow",
            ErrorCategory::Internal,
            "可用性测试的状态没能更新，请刷新后重试。",
            false,
            BTreeMap::new(),
        ),
        ModelAvailabilityServiceError::MissingProjection => (
            "models.projection.unavailable",
            ErrorCategory::Unavailable,
            "暂时读不到可用性测试的状态，请刷新后再看。",
            true,
            BTreeMap::new(),
        ),
        ModelAvailabilityServiceError::OperationNotFound => (
            "operation.not_found",
            ErrorCategory::NotFound,
            "这次可用性测试已不存在，请刷新。",
            false,
            BTreeMap::new(),
        ),
        ModelAvailabilityServiceError::WorkerTerminated => (
            "internal.unexpected",
            ErrorCategory::Internal,
            "可用性测试意外中断，请重试。",
            true,
            BTreeMap::new(),
        ),
        ModelAvailabilityServiceError::OperationRevisionConflict { .. } => {
            unreachable!("availability revision conflicts return before generic mapping")
        }
        ModelAvailabilityServiceError::ConfigJournal(_) => {
            unreachable!("config journal errors return before availability error mapping")
        }
    };
    IpcErrorEnvelope {
        code: ErrorCode::new(code).expect("static ErrorCode is valid"),
        category,
        message: message.to_owned(),
        retryable,
        correlation_id,
        terminal: IpcRequestTerminal::Failed,
        details,
    }
}

fn discovery_error(
    error: ModelDiscoveryServiceError,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    if let ModelDiscoveryServiceError::ConfigJournal(error) = error {
        return journal_error(error, correlation_id);
    }
    if let ModelDiscoveryServiceError::OperationRevisionConflict { expected, actual } = error {
        return operation_revision_error(
            "operation.revision_conflict",
            "获取模型列表的状态已经变化，请刷新后再取消。",
            expected,
            actual,
            correlation_id,
        );
    }
    let (code, category, message, retryable) = match error {
        ModelDiscoveryServiceError::InvalidConfiguration => (
            "configuration.endpoint.invalid",
            ErrorCategory::Validation,
            "保存的连接配置不能用来获取模型列表，请检查 Base URL 与模型目录路径。",
            false,
        ),
        ModelDiscoveryServiceError::ConfigurationRevisionChanged => (
            "models.configuration.revision_conflict",
            ErrorCategory::Conflict,
            "模型设置已在别处更新，请刷新后再获取模型列表。",
            true,
        ),
        ModelDiscoveryServiceError::CredentialUnavailable => (
            "credential.unavailable",
            ErrorCategory::Unavailable,
            "这个连接的凭据暂不可用，无法获取模型列表；请重新保存 API Key 后再试。",
            false,
        ),
        ModelDiscoveryServiceError::ModelConfigMutationInProgress => (
            "models.configuration.mutation_in_progress",
            ErrorCategory::Conflict,
            "另一项模型设置的修改正在进行，请等它完成后再试。",
            true,
        ),
        ModelDiscoveryServiceError::Persistence(_) => (
            "persistence.operation_failed",
            ErrorCategory::Unavailable,
            "获取模型列表的操作没能记录下来，没有开始；请重试。",
            true,
        ),
        ModelDiscoveryServiceError::Transport(_) => (
            "transport.operation_failed",
            ErrorCategory::Unavailable,
            "获取模型列表的网络请求没能开始；请检查网络与代理设置后重试。",
            true,
        ),
        ModelDiscoveryServiceError::RevisionOverflow => (
            "persistence.revision_overflow",
            ErrorCategory::Internal,
            "获取模型列表的状态没能更新，请刷新后重试。",
            false,
        ),
        ModelDiscoveryServiceError::MissingProjection => (
            "models.projection.unavailable",
            ErrorCategory::Unavailable,
            "暂时读不到获取模型列表的状态，请刷新后再看。",
            true,
        ),
        ModelDiscoveryServiceError::OperationNotFound => (
            "operation.not_found",
            ErrorCategory::NotFound,
            "这次获取模型列表的操作已不存在，请刷新。",
            false,
        ),
        ModelDiscoveryServiceError::WorkerTerminated => {
            ("internal.unexpected", ErrorCategory::Internal, "获取模型列表意外中断，请重试。", true)
        }
        ModelDiscoveryServiceError::OperationRevisionConflict { .. } => {
            unreachable!("discovery revision conflicts return before generic mapping")
        }
        ModelDiscoveryServiceError::ConfigJournal(_) => unreachable!("handled above"),
    };
    IpcErrorEnvelope {
        code: ErrorCode::new(code).expect("static ErrorCode is valid"),
        category,
        message: message.to_owned(),
        retryable,
        correlation_id,
        terminal: IpcRequestTerminal::Failed,
        details: BTreeMap::new(),
    }
}

fn operation_revision_error(
    code: &str,
    message: &str,
    expected: u64,
    actual: u64,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    let details = BTreeMap::from([
        ("expected_revision".to_owned(), serde_json::json!(expected.to_string())),
        ("actual_revision".to_owned(), serde_json::json!(actual.to_string())),
    ]);
    IpcErrorEnvelope {
        code: ErrorCode::new(code).expect("static ErrorCode is valid"),
        category: ErrorCategory::Conflict,
        message: message.to_owned(),
        retryable: true,
        correlation_id,
        terminal: IpcRequestTerminal::Failed,
        details,
    }
}

fn projection_error(_error: P05ProjectionError, correlation_id: CorrelationId) -> IpcErrorEnvelope {
    IpcErrorEnvelope {
        code: ErrorCode::new("models.projection.unavailable").expect("static ErrorCode is valid"),
        category: ErrorCategory::Unavailable,
        message: "暂时读不到模型设置，请稍后刷新。".to_owned(),
        retryable: true,
        correlation_id,
        terminal: IpcRequestTerminal::Failed,
        details: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc, time::Duration};

    use aura_config::{AuraPaths, ConfigModule, ModuleConfigValidator, SettingsFacade};
    use aura_domain_common::{
        CommandInvocationId, CorrelationId, EndpointId, ErrorCategory, ExpectedRevision,
        IpcChannelId, Revision, RuntimeInstanceId, SessionId, WorkspaceId,
    };
    use aura_model_config::{ModelConfigError, ModelConfigValidationError, ModelConfigValidator};
    use aura_model_domain::{
        AddManualModelInput, ChatInstructionRole, ChatOutputTokenField, CreateModelProviderInput,
        EndpointAuthScheme, ModelApiFormat, ModelAvailabilityTestCancelRequest,
        ModelAvailabilityTestCancelResult, ModelAvailabilityTestStartAccepted,
        ModelAvailabilityTestStartRequest, ModelCatalogGetRequest, ModelDeletionMutationResult,
        ModelEndpointCreateRequest, ModelEndpointMutationResult, ModelEndpointTimeouts,
        ModelEndpointUpdateRequest, ModelProfileCreateManualRequest, ModelProfileDeleteRequest,
        ModelProfileMutationResult, ModelProfileUpdateRequest, ModelProviderCreateRequest,
        ModelProviderMutationResult, ModelProviderSource, ModelRequestDefaults,
        ProviderCatalogProjection, SessionModelBindingMutationResult,
        SessionModelBindingSetRequest, TransportAttemptState, model_ipc_method,
    };
    use aura_network_control::AuthenticatedConfigurationChannelIdentity;
    use aura_persistence::{
        ModelCredentialRecordState, ModelProjectionDraft, PersistenceCoordinator,
        PersistenceOptions, PersistenceStartup,
    };
    use aura_projection_contract::{ProjectionChange, ProjectionKind, ProjectionScope};
    use aura_runtime_contract::{BundleNamespace, IpcErrorEnvelope};
    use aura_workspace_domain::{
        CatalogFreshness, SessionSummary, WorkspaceState, WorkspaceSummary,
    };

    use super::super::{P05Services, config_journal::ModelConfigJournalError};

    struct Fixture {
        _directory: tempfile::TempDir,
        services: P05Services,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().expect("temporary P05 command root");
            let paths = AuraPaths::from_roots(
                directory.path().join("roaming/Aura"),
                directory.path().join("local/Aura"),
            );
            let validators = BTreeMap::<ConfigModule, Arc<dyn ModuleConfigValidator>>::from([(
                ConfigModule::Models,
                Arc::new(ModelConfigValidator) as Arc<dyn ModuleConfigValidator>,
            )]);
            let settings =
                SettingsFacade::open_with_validators(paths, validators).expect("settings facade");
            let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
                directory.path().join("data/aura.db"),
                directory.path().join("data/artifacts"),
                RuntimeInstanceId::new(),
            ))
            .expect("start persistence")
            {
                PersistenceStartup::Ready(coordinator) => Arc::new(coordinator),
                PersistenceStartup::RecoveryRequired(recovery) => {
                    panic!("unexpected persistence recovery: {}", recovery.reason)
                }
            };
            let namespace: BundleNamespace =
                "aura.desktop.test.p05-commands".parse().expect("bundle namespace");
            let services = P05Services::open(
                persistence,
                settings,
                namespace,
                aura_core_base::sync_boundary::SyncBoundary::open(),
            )
            .expect("open P05 services");
            Self { _directory: directory, services }
        }

        fn command<T: serde::Serialize, R: serde::de::DeserializeOwned>(
            &self,
            method: &str,
            request: T,
        ) -> R {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("P05 command test runtime");
            runtime.block_on(self.command_async(method, request))
        }

        async fn command_async<T: serde::Serialize, R: serde::de::DeserializeOwned>(
            &self,
            method: &str,
            request: T,
        ) -> R {
            let result = self
                .services
                .dispatch_main_command(
                    method,
                    &serde_json::to_value(request).expect("encode command"),
                    Some(CommandInvocationId::new()),
                    CorrelationId::new(),
                    AuthenticatedConfigurationChannelIdentity {
                        core_runtime_instance_id: RuntimeInstanceId::new(),
                        authenticated_main_runtime_instance_id: RuntimeInstanceId::new(),
                        authenticated_channel_id: IpcChannelId::new(),
                    },
                )
                .await
                .expect("recognized P05 command")
                .unwrap_or_else(|error| panic!("{method} failed: {error:?}"));
            serde_json::from_value(result.value).expect("decode P05 result")
        }

        fn command_error<T: serde::Serialize>(&self, method: &str, request: T) -> IpcErrorEnvelope {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("P05 command test runtime");
            let result = runtime
                .block_on(self.services.dispatch_main_command(
                    method,
                    &serde_json::to_value(request).expect("encode command"),
                    Some(CommandInvocationId::new()),
                    CorrelationId::new(),
                    AuthenticatedConfigurationChannelIdentity {
                        core_runtime_instance_id: RuntimeInstanceId::new(),
                        authenticated_main_runtime_instance_id: RuntimeInstanceId::new(),
                        authenticated_channel_id: IpcChannelId::new(),
                    },
                ))
                .expect("recognized P05 command");
            match result {
                Ok(_) => panic!("{method} unexpectedly succeeded"),
                Err(error) => error,
            }
        }

        fn seed_session(&self) -> SessionId {
            let workspace_id = WorkspaceId::new();
            let session_id = SessionId::new();
            self.services
                .persistence
                .upsert_workspace_session(
                    WorkspaceSummary {
                        workspace_id,
                        display_name: "P05 Workspace".to_owned(),
                        selected_root: r"C:\Aura Tests\P05 Commands".to_owned(),
                        canonical_root: r"C:\Aura Tests\P05 Commands".to_owned(),
                        root_identity: None,
                        state: WorkspaceState::Online,
                        revision: Revision::INITIAL,
                        catalog_freshness: CatalogFreshness::Unavailable,
                        authorization_epoch: Revision::INITIAL,
                    },
                    SessionSummary {
                        session_id,
                        workspace_id,
                        title: "P05 Session".to_owned(),
                        state: "idle".to_owned(),
                    },
                    CorrelationId::new(),
                )
                .expect("seed P05 Session");
            session_id
        }
    }

    #[test]
    fn endpoint_validation_errors_have_stable_codes_and_field_details() {
        let endpoint_id = EndpointId::new();
        let cases = [
            (
                ModelConfigValidationError::MissingChatOutputTokenField { endpoint_id },
                "models.endpoint.chat_output_token_field_required",
                "chat_output_token_field",
            ),
            (
                ModelConfigValidationError::MissingChatInstructionRole { endpoint_id },
                "models.endpoint.chat_instruction_role_required",
                "chat_instruction_role",
            ),
            (
                ModelConfigValidationError::InvalidModelsPath {
                    endpoint_id,
                    message: "invalid path".to_owned(),
                },
                "models.endpoint.invalid_models_path",
                "models_path",
            ),
            (
                ModelConfigValidationError::InvalidTimeout { endpoint_id, field: "stream_idle_ms" },
                "models.endpoint.invalid_timeout",
                "stream_idle_ms",
            ),
            (
                ModelConfigValidationError::InvalidParallelLimit { endpoint_id },
                "models.endpoint.invalid_parallel_limit",
                "max_parallel_model_requests",
            ),
            (
                ModelConfigValidationError::InvalidAuthScheme {
                    endpoint_id,
                    api_format: ModelApiFormat::OpenAiResponses,
                    auth_scheme: EndpointAuthScheme::AnthropicApiKey,
                },
                "models.endpoint.invalid_auth_scheme",
                "auth_scheme",
            ),
            (
                ModelConfigValidationError::InvalidBaseUrl {
                    endpoint_id,
                    message: "invalid URL".to_owned(),
                },
                "models.endpoint.invalid_base_url",
                "base_url",
            ),
            (
                ModelConfigValidationError::InvalidRoutingField {
                    endpoint_id,
                    field: "chat_instruction_role",
                },
                "models.endpoint.field_not_allowed",
                "chat_instruction_role",
            ),
            (
                ModelConfigValidationError::BlankValue { path: "endpoint.display_name".to_owned() },
                "models.endpoint.invalid_display_name",
                "display_name",
            ),
        ];

        for (validation_error, expected_code, expected_field) in cases {
            let envelope = super::journal_error(
                ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
                    validation_error,
                )),
                CorrelationId::new(),
            );
            assert_eq!(envelope.code.as_str(), expected_code);
            assert_eq!(envelope.category, ErrorCategory::Validation);
            assert!(!envelope.retryable);
            assert_eq!(
                envelope.details.get("field").and_then(serde_json::Value::as_str),
                Some(expected_field)
            );
        }
    }

    #[test]
    fn catalog_get_returns_the_committed_projection_revision() {
        let fixture = Fixture::new();
        let latest = fixture
            .services
            .persistence
            .latest_model_projection(ProjectionKind::ProviderCatalog, ProjectionScope::Global)
            .expect("read initial catalog projection")
            .expect("initial catalog projection");
        let ProjectionChange::ProviderCatalog { payload, .. } = latest else {
            panic!("expected Provider Catalog projection");
        };
        let changes = fixture
            .services
            .persistence
            .commit_model_projection_snapshots(vec![ModelProjectionDraft::ProviderCatalog(
                *payload,
            )])
            .expect("commit next catalog projection");
        let ProjectionChange::ProviderCatalog { revision, payload, .. } = &changes[0] else {
            panic!("expected committed Provider Catalog projection");
        };
        assert_eq!(*revision, Revision::new(2));
        assert_eq!(payload.projection_revision, *revision);

        let returned: ProviderCatalogProjection =
            fixture.command(model_ipc_method::CATALOG_GET, ModelCatalogGetRequest {});
        assert_eq!(returned.projection_revision, *revision);
        assert_eq!(returned, **payload);
    }

    #[test]
    fn endpoint_and_profile_commands_commit_atomic_revisions_and_credential_slot() {
        let fixture = Fixture::new();
        let provider: ModelProviderMutationResult = fixture.command(
            model_ipc_method::PROVIDER_CREATE,
            ModelProviderCreateRequest {
                expected_revision: ExpectedRevision::new(1),
                input: CreateModelProviderInput {
                    display_name: "Example".to_owned(),
                    source: ModelProviderSource::Custom { catalog_provider_id: None },
                },
            },
        );
        assert_eq!(provider.revision, Revision::new(2));

        let endpoint: ModelEndpointMutationResult = fixture.command(
            model_ipc_method::ENDPOINT_CREATE,
            ModelEndpointCreateRequest {
                expected_revision: ExpectedRevision::new(2),
                provider_id: provider.value.id,
                display_name: "Primary".to_owned(),
                api_format: ModelApiFormat::OpenAiChatCompletions,
                base_url: "https://api.example.test/v1".to_owned(),
                discovery_mode: aura_model_domain::ModelDiscoveryMode::Automatic,
                chat_output_token_field: Some(ChatOutputTokenField::MaxCompletionTokens),
                chat_instruction_role: Some(ChatInstructionRole::System),
                auth_scheme: EndpointAuthScheme::OpenAiBearerApiKey,
                openai_organization_id: None,
                openai_project_id: None,
                anthropic_workspace_id: None,
                timeouts: ModelEndpointTimeouts {
                    connect_ms: 5_000,
                    first_event_ms: 30_000,
                    stream_idle_ms: 30_000,
                    total_ms: Some(120_000),
                },
                max_parallel_model_requests: 2,
            },
        );
        assert_eq!(endpoint.revision, Revision::new(3));
        assert_eq!(endpoint.value.revision, Revision::INITIAL);
        let credential = fixture
            .services
            .persistence
            .model_credential_record_for_endpoint(
                endpoint.value.id,
                aura_model_domain::ModelCredentialSlot::ApiKey,
            )
            .expect("query credential slot")
            .expect("secret-auth Endpoint receives a stable slot");
        assert_eq!(credential.endpoint_id, endpoint.value.id);
        assert_eq!(credential.state, ModelCredentialRecordState::Unconfigured);
        assert_eq!(credential.row_revision, Revision::INITIAL);

        let profile: ModelProfileMutationResult = fixture.command(
            model_ipc_method::PROFILE_CREATE_MANUAL,
            ModelProfileCreateManualRequest {
                expected_revision: ExpectedRevision::new(3),
                input: AddManualModelInput {
                    endpoint_id: endpoint.value.id,
                    provider_model_name: "example-model".to_owned(),
                    display_name: Some("Example Model".to_owned()),
                    notes: None,
                    context_window_tokens: Some(128_000),
                },
            },
        );
        assert_eq!(profile.revision, Revision::new(4));
        assert_eq!(profile.value.revision, Revision::INITIAL);
        let after_profile = fixture.services.model_config.snapshot().expect("model snapshot");
        assert_eq!(
            after_profile.registry.endpoints[&endpoint.value.id].revision,
            Revision::new(2),
            "adding a Profile changes Endpoint model_order exactly once"
        );

        let updated: ModelProfileMutationResult = fixture.command(
            model_ipc_method::PROFILE_UPDATE,
            ModelProfileUpdateRequest {
                expected_revision: ExpectedRevision::new(4),
                model_profile_id: profile.value.id,
                display_name: "Example Model".to_owned(),
                notes: None,
                enabled: true,
                request_defaults: ModelRequestDefaults {
                    max_output_tokens: Some(4_096),
                    ..ModelRequestDefaults::default()
                },
                historical_snapshot_max_tokens: 2_048,
            },
        );
        assert_eq!(updated.revision, Revision::new(5));
        assert_eq!(updated.value.revision, Revision::new(2));
        assert_eq!(updated.value.provider_model_name, "example-model");

        let no_op: ModelProfileMutationResult = fixture.command(
            model_ipc_method::PROFILE_UPDATE,
            ModelProfileUpdateRequest {
                expected_revision: ExpectedRevision::new(5),
                model_profile_id: updated.value.id,
                display_name: updated.value.display_name.clone(),
                notes: updated.value.notes.clone(),
                enabled: updated.value.enabled,
                request_defaults: updated.value.request_defaults.clone(),
                historical_snapshot_max_tokens: updated
                    .value
                    .context
                    .historical_snapshot_max_tokens,
            },
        );
        assert!(!no_op.changed);
        assert_eq!(no_op.revision, Revision::new(5));
        assert_eq!(no_op.value.revision, Revision::new(2));
    }

    #[test]
    fn endpoint_update_validates_global_and_object_revisions_independently() {
        fn update_request(
            endpoint: &aura_model_domain::ModelEndpointConfig,
            expected_models_revision: u64,
            expected_endpoint_revision: u64,
        ) -> ModelEndpointUpdateRequest {
            ModelEndpointUpdateRequest {
                expected_models_revision: ExpectedRevision::new(expected_models_revision),
                expected_endpoint_revision: ExpectedRevision::new(expected_endpoint_revision),
                endpoint_id: endpoint.id,
                display_name: endpoint.display_name.clone(),
                api_format: endpoint.api_format,
                base_url: endpoint.base_url.clone(),
                discovery_mode: endpoint.discovery_mode.clone(),
                chat_output_token_field: endpoint.chat_output_token_field,
                chat_instruction_role: endpoint.chat_instruction_role,
                auth_scheme: endpoint.auth_scheme,
                openai_organization_id: endpoint.openai_organization_id.clone(),
                openai_project_id: endpoint.openai_project_id.clone(),
                anthropic_workspace_id: endpoint.anthropic_workspace_id.clone(),
                timeouts: endpoint.timeouts.clone(),
                max_parallel_model_requests: endpoint.max_parallel_model_requests,
            }
        }

        let fixture = Fixture::new();
        let provider: ModelProviderMutationResult = fixture.command(
            model_ipc_method::PROVIDER_CREATE,
            ModelProviderCreateRequest {
                expected_revision: ExpectedRevision::new(1),
                input: CreateModelProviderInput {
                    display_name: "Revision Test".to_owned(),
                    source: ModelProviderSource::Custom { catalog_provider_id: None },
                },
            },
        );
        let endpoint: ModelEndpointMutationResult = fixture.command(
            model_ipc_method::ENDPOINT_CREATE,
            ModelEndpointCreateRequest {
                expected_revision: ExpectedRevision::new(2),
                provider_id: provider.value.id,
                display_name: "Primary".to_owned(),
                api_format: ModelApiFormat::OpenAiChatCompletions,
                base_url: "https://api.example.test/v1".to_owned(),
                discovery_mode: aura_model_domain::ModelDiscoveryMode::Automatic,
                chat_output_token_field: Some(ChatOutputTokenField::MaxCompletionTokens),
                chat_instruction_role: Some(ChatInstructionRole::System),
                auth_scheme: EndpointAuthScheme::None,
                openai_organization_id: None,
                openai_project_id: None,
                anthropic_workspace_id: None,
                timeouts: ModelEndpointTimeouts {
                    connect_ms: 5_000,
                    first_event_ms: 30_000,
                    stream_idle_ms: 30_000,
                    total_ms: Some(120_000),
                },
                max_parallel_model_requests: 2,
            },
        );
        assert_eq!(endpoint.revision, Revision::new(3));
        assert_eq!(endpoint.value.revision, Revision::INITIAL);

        let mut update = update_request(&endpoint.value, 3, 1);
        update.display_name = "Primary Renamed".to_owned();
        let updated: ModelEndpointMutationResult =
            fixture.command(model_ipc_method::ENDPOINT_UPDATE, update);
        assert!(updated.changed);
        assert_eq!(updated.revision, Revision::new(4));
        assert_eq!(updated.value.revision, Revision::new(2));

        let no_op: ModelEndpointMutationResult = fixture
            .command(model_ipc_method::ENDPOINT_UPDATE, update_request(&updated.value, 4, 2));
        assert!(!no_op.changed);
        assert_eq!(no_op.revision, Revision::new(4));
        assert_eq!(no_op.value.revision, Revision::new(2));

        let global_conflict = fixture
            .command_error(model_ipc_method::ENDPOINT_UPDATE, update_request(&updated.value, 3, 2));
        assert_eq!(global_conflict.code.as_str(), "models.configuration.revision_conflict");

        let object_conflict = fixture
            .command_error(model_ipc_method::ENDPOINT_UPDATE, update_request(&updated.value, 4, 1));
        assert_eq!(object_conflict.code.as_str(), "models.configuration.revision_conflict");

        let snapshot = fixture.services.model_config.snapshot().expect("model snapshot");
        let persisted = &snapshot.registry.endpoints[&updated.value.id];
        assert_eq!(snapshot.revision, 4);
        assert_eq!(persisted.revision, Revision::new(2));
        assert_eq!(persisted.display_name, "Primary Renamed");
    }

    #[test]
    fn queued_and_running_availability_cancel_preserve_transport_terminal_order() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

        let fixture = Fixture::new();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test Provider");
        listener.set_nonblocking(true).expect("configure test Provider listener");
        let provider_address = listener.local_addr().expect("test Provider address");

        let provider: ModelProviderMutationResult = fixture.command(
            model_ipc_method::PROVIDER_CREATE,
            ModelProviderCreateRequest {
                expected_revision: ExpectedRevision::new(1),
                input: CreateModelProviderInput {
                    display_name: "Cancellation Provider".to_owned(),
                    source: ModelProviderSource::Custom { catalog_provider_id: None },
                },
            },
        );
        let endpoint: ModelEndpointMutationResult = fixture.command(
            model_ipc_method::ENDPOINT_CREATE,
            ModelEndpointCreateRequest {
                expected_revision: ExpectedRevision::new(2),
                provider_id: provider.value.id,
                display_name: "Cancellation Endpoint".to_owned(),
                api_format: ModelApiFormat::OpenAiResponses,
                base_url: format!("http://{provider_address}/v1"),
                discovery_mode: aura_model_domain::ModelDiscoveryMode::Automatic,
                chat_output_token_field: None,
                chat_instruction_role: None,
                auth_scheme: EndpointAuthScheme::None,
                openai_organization_id: None,
                openai_project_id: None,
                anthropic_workspace_id: None,
                timeouts: ModelEndpointTimeouts {
                    connect_ms: 5_000,
                    first_event_ms: 30_000,
                    stream_idle_ms: 30_000,
                    total_ms: Some(30_000),
                },
                max_parallel_model_requests: 1,
            },
        );
        let profile: ModelProfileMutationResult = fixture.command(
            model_ipc_method::PROFILE_CREATE_MANUAL,
            ModelProfileCreateManualRequest {
                expected_revision: ExpectedRevision::new(3),
                input: AddManualModelInput {
                    endpoint_id: endpoint.value.id,
                    provider_model_name: "cancel-test-model".to_owned(),
                    display_name: Some("Cancel Test Model".to_owned()),
                    notes: None,
                    context_window_tokens: Some(8_192),
                },
            },
        );
        let profile: ModelProfileMutationResult = fixture.command(
            model_ipc_method::PROFILE_UPDATE,
            ModelProfileUpdateRequest {
                expected_revision: ExpectedRevision::new(4),
                model_profile_id: profile.value.id,
                display_name: profile.value.display_name,
                notes: profile.value.notes,
                enabled: true,
                request_defaults: ModelRequestDefaults {
                    max_output_tokens: Some(512),
                    ..ModelRequestDefaults::default()
                },
                historical_snapshot_max_tokens: profile
                    .value
                    .context
                    .historical_snapshot_max_tokens,
            },
        );
        assert_eq!(profile.value.revision, Revision::new(2));
        let current_endpoint =
            fixture.services.model_config.snapshot().expect("model snapshot").registry.endpoints
                [&endpoint.value.id]
                .clone();
        assert_eq!(current_endpoint.revision, Revision::new(2));

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("availability cancellation runtime");

        let queued_correlation_id = CorrelationId::new();
        let queued_command_id = CommandInvocationId::new();
        let queued_channel = AuthenticatedConfigurationChannelIdentity {
            core_runtime_instance_id: RuntimeInstanceId::new(),
            authenticated_main_runtime_instance_id: RuntimeInstanceId::new(),
            authenticated_channel_id: IpcChannelId::new(),
        };
        let prepared_queued = fixture
            .services
            .prepare_model_availability_test(ModelAvailabilityTestStartRequest {
                model_profile_id: profile.value.id,
                expected_model_profile_revision: ExpectedRevision::new(2),
                expected_endpoint_revision: ExpectedRevision::new(2),
            })
            .expect("prepare queued availability test");
        let queued_identity = super::model_availability_identity(
            queued_command_id,
            queued_correlation_id,
            queued_channel,
            &prepared_queued,
        );
        let queued = fixture
            .services
            .queue_model_availability_test(
                &prepared_queued,
                &queued_identity,
                queued_correlation_id,
            )
            .expect("queue availability test without starting its Worker");
        assert!(queued.newly_queued);
        fixture
            .services
            .persistence
            .publish_model_projection_changes(&queued.projection_changes)
            .expect("publish queued availability Projection");
        fixture
            .services
            .operation_cancellations
            .register_availability(queued.accepted.model_availability_test_id);
        let queued_cancelled = runtime
            .block_on(fixture.services.cancel_model_availability_test(
                ModelAvailabilityTestCancelRequest {
                    model_availability_test_id: queued.accepted.model_availability_test_id,
                    expected_operational_revision: ExpectedRevision::new(
                        queued.accepted.operational_revision.value(),
                    ),
                },
            ))
            .expect("cancel queued availability test")
            .0;
        fixture
            .services
            .operation_cancellations
            .remove_availability(queued.accepted.model_availability_test_id);
        assert_eq!(
            queued_cancelled.state,
            aura_model_domain::ModelAvailabilityTestState::Cancelled
        );
        let connection = rusqlite::Connection::open(fixture._directory.path().join("data/aura.db"))
            .expect("open event database after queued cancellation");
        let transport_attempt_count = connection
            .query_row("SELECT COUNT(*) FROM transport_attempts", [], |row| row.get::<_, i64>(0))
            .expect("count queued-cancel transport Attempts");
        assert_eq!(transport_attempt_count, 0, "Queued cancel must not create an Attempt");
        drop(connection);

        let (accepted_sender, accepted_receiver) = tokio::sync::oneshot::channel();
        let (closed_sender, closed_receiver) = tokio::sync::oneshot::channel();
        let server = runtime.spawn(async move {
            let listener =
                tokio::net::TcpListener::from_std(listener).expect("adopt test Provider listener");
            let (mut stream, _) = listener.accept().await.expect("accept Provider request");
            let _ = accepted_sender.send(());
            let mut buffer = [0_u8; 4_096];
            loop {
                match stream.read(&mut buffer).await {
                    Ok(0) | Err(_) => {
                        let _ = closed_sender.send(());
                        break;
                    }
                    Ok(_) => {}
                }
            }

            let (mut success_stream, _) = listener
                .accept()
                .await
                .expect("accept successful Provider request");
            let mut request = Vec::new();
            let mut request_buffer = [0_u8; 4_096];
            loop {
                let read = success_stream
                    .read(&mut request_buffer)
                    .await
                    .expect("read successful Provider request");
                assert!(read > 0, "Provider request ended before its body was complete");
                request.extend_from_slice(&request_buffer[..read]);
                let Some(header_end) = request
                    .windows(4)
                    .position(|window| window == b"\r\n\r\n")
                    .map(|offset| offset + 4)
                else {
                    continue;
                };
                let headers = std::str::from_utf8(&request[..header_end])
                    .expect("Provider request headers are UTF-8");
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().expect("Content-Length"))
                    })
                    .unwrap_or(0);
                if request.len() >= header_end + content_length {
                    break;
                }
            }
            let body = br#"{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"Acknowledged."}]}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            success_stream
                .write_all(response.as_bytes())
                .await
                .expect("write successful Provider headers");
            success_stream
                .write_all(body)
                .await
                .expect("write successful Provider body");
            success_stream.shutdown().await.expect("close successful Provider response");
        });

        let accepted: ModelAvailabilityTestStartAccepted = runtime.block_on(async {
            fixture
                .command_async(
                    model_ipc_method::PROFILE_TEST_AVAILABILITY,
                    ModelAvailabilityTestStartRequest {
                        model_profile_id: profile.value.id,
                        expected_model_profile_revision: ExpectedRevision::new(2),
                        expected_endpoint_revision: ExpectedRevision::new(2),
                    },
                )
                .await
        });
        runtime
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(10), accepted_receiver).await
            })
            .expect("Provider request should start")
            .expect("Provider request accepted signal");

        let running = fixture
            .services
            .persistence
            .model_availability_test(accepted.model_availability_test_id)
            .expect("load running availability test")
            .expect("running availability test exists");
        assert_eq!(running.state, aura_model_domain::ModelAvailabilityTestState::Running);
        assert_eq!(running.direct_transport_attempt_ids.len(), 1);
        let attempt_id = running.direct_transport_attempt_ids[0];

        let cancelled: ModelAvailabilityTestCancelResult = runtime
            .block_on(async {
                tokio::time::timeout(
                    Duration::from_secs(10),
                    fixture.command_async(
                        model_ipc_method::AVAILABILITY_TEST_CANCEL,
                        ModelAvailabilityTestCancelRequest {
                            model_availability_test_id: running.id,
                            expected_operational_revision: ExpectedRevision::new(
                                running.revision.value(),
                            ),
                        },
                    ),
                )
                .await
            })
            .expect("availability cancellation should reach a durable terminal Projection");
        assert_eq!(cancelled.state, aura_model_domain::ModelAvailabilityTestState::Cancelled);
        runtime
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(10), closed_receiver).await
            })
            .expect("cancel should close the Provider connection")
            .expect("Provider connection closed signal");

        let attempt = fixture
            .services
            .persistence
            .transport_attempt(attempt_id)
            .expect("load cancelled Attempt")
            .expect("cancelled Attempt exists");
        assert_eq!(attempt.state, TransportAttemptState::Cancelled);
        let operation = fixture
            .services
            .persistence
            .model_availability_test(running.id)
            .expect("load cancelled availability test")
            .expect("cancelled availability test exists");
        assert_eq!(operation.state, aura_model_domain::ModelAvailabilityTestState::Cancelled);
        assert!(
            fixture
                .services
                .persistence
                .nonterminal_configuration_transport_attempts()
                .expect("load non-terminal configuration Attempts")
                .is_empty()
        );

        let completed_accepted: ModelAvailabilityTestStartAccepted =
            runtime.block_on(fixture.command_async(
                model_ipc_method::PROFILE_TEST_AVAILABILITY,
                ModelAvailabilityTestStartRequest {
                    model_profile_id: profile.value.id,
                    expected_model_profile_revision: ExpectedRevision::new(2),
                    expected_endpoint_revision: ExpectedRevision::new(2),
                },
            ));
        let completed = runtime
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(10), async {
                    loop {
                        let current = fixture
                            .services
                            .persistence
                            .model_availability_test(completed_accepted.model_availability_test_id)
                            .expect("poll completed availability test")
                            .expect("completed availability test exists");
                        if current.state.is_terminal() {
                            break current;
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
            })
            .expect("successful Provider response should complete availability test");
        runtime.block_on(server).expect("test Provider task");
        assert_eq!(completed.state, aura_model_domain::ModelAvailabilityTestState::Completed);
        assert_eq!(
            completed.result.as_ref().map(|result| result.outcome),
            Some(aura_model_domain::ModelAvailabilityTestOutcome::Available),
            "unexpected availability result: {:#?}",
            completed.result
        );
        let completed_cancel: ModelAvailabilityTestCancelResult =
            runtime.block_on(fixture.command_async(
                model_ipc_method::AVAILABILITY_TEST_CANCEL,
                ModelAvailabilityTestCancelRequest {
                    model_availability_test_id: completed.id,
                    expected_operational_revision: ExpectedRevision::new(
                        completed.revision.value(),
                    ),
                },
            ));
        assert_eq!(
            completed_cancel.state,
            aura_model_domain::ModelAvailabilityTestState::Completed,
            "a response that persisted first must win over a later cancel"
        );
        let completed_after_cancel = fixture
            .services
            .persistence
            .model_availability_test(completed.id)
            .expect("reload completed availability test")
            .expect("completed availability test exists after cancel");
        assert_eq!(completed_after_cancel, completed);

        let connection = rusqlite::Connection::open(fixture._directory.path().join("data/aura.db"))
            .expect("open event database");
        let events = connection
            .prepare("SELECT event_type, payload_json FROM events ORDER BY global_sequence")
            .expect("prepare event query")
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .expect("query events")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect events");
        let attempt_cancelled = events
            .iter()
            .position(|(event_type, payload)| {
                event_type == "ProviderTransportAttemptCancelled"
                    && payload.contains(&attempt_id.to_string())
            })
            .expect("Attempt cancellation event");
        let operation_cancelled = events
            .iter()
            .position(|(event_type, payload)| {
                event_type == "ModelAvailabilityTestCancelled"
                    && payload.contains(&running.id.to_string())
            })
            .expect("availability cancellation event");
        assert!(attempt_cancelled < operation_cancelled);
    }

    #[test]
    fn session_binding_and_profile_delete_commit_binding_clear_atomically() {
        let fixture = Fixture::new();
        let provider: ModelProviderMutationResult = fixture.command(
            model_ipc_method::PROVIDER_CREATE,
            ModelProviderCreateRequest {
                expected_revision: ExpectedRevision::new(1),
                input: CreateModelProviderInput {
                    display_name: "Local".to_owned(),
                    source: ModelProviderSource::Custom { catalog_provider_id: None },
                },
            },
        );
        let endpoint: ModelEndpointMutationResult = fixture.command(
            model_ipc_method::ENDPOINT_CREATE,
            ModelEndpointCreateRequest {
                expected_revision: ExpectedRevision::new(2),
                provider_id: provider.value.id,
                display_name: "No Auth".to_owned(),
                api_format: ModelApiFormat::OpenAiChatCompletions,
                base_url: "https://api.example.test/v1".to_owned(),
                discovery_mode: aura_model_domain::ModelDiscoveryMode::Automatic,
                chat_output_token_field: Some(ChatOutputTokenField::MaxCompletionTokens),
                chat_instruction_role: Some(ChatInstructionRole::System),
                auth_scheme: EndpointAuthScheme::None,
                openai_organization_id: None,
                openai_project_id: None,
                anthropic_workspace_id: None,
                timeouts: ModelEndpointTimeouts {
                    connect_ms: 5_000,
                    first_event_ms: 30_000,
                    stream_idle_ms: 30_000,
                    total_ms: Some(120_000),
                },
                max_parallel_model_requests: 2,
            },
        );
        let profile: ModelProfileMutationResult = fixture.command(
            model_ipc_method::PROFILE_CREATE_MANUAL,
            ModelProfileCreateManualRequest {
                expected_revision: ExpectedRevision::new(3),
                input: AddManualModelInput {
                    endpoint_id: endpoint.value.id,
                    provider_model_name: "local-model".to_owned(),
                    display_name: None,
                    notes: None,
                    context_window_tokens: Some(32_000),
                },
            },
        );
        let profile: ModelProfileMutationResult = fixture.command(
            model_ipc_method::PROFILE_UPDATE,
            ModelProfileUpdateRequest {
                expected_revision: ExpectedRevision::new(4),
                model_profile_id: profile.value.id,
                display_name: profile.value.display_name,
                notes: profile.value.notes,
                enabled: true,
                request_defaults: ModelRequestDefaults {
                    max_output_tokens: Some(1_024),
                    ..ModelRequestDefaults::default()
                },
                historical_snapshot_max_tokens: profile
                    .value
                    .context
                    .historical_snapshot_max_tokens,
            },
        );
        let session_id = fixture.seed_session();
        let selector = fixture
            .services
            .projections
            .session_model_selector(
                &fixture.services.model_config.snapshot().expect("snapshot"),
                session_id,
                None,
            )
            .expect("unbound selector");
        assert_eq!(selector.binding_revision, Revision::new(0));
        let binding: SessionModelBindingMutationResult = fixture.command(
            model_ipc_method::SESSION_SET_BINDING,
            SessionModelBindingSetRequest {
                expected_revision: ExpectedRevision::new(0),
                session_id,
                model_profile_id: Some(profile.value.id),
            },
        );
        assert!(binding.changed);
        assert_eq!(binding.revision, Revision::INITIAL);

        let deleted: ModelDeletionMutationResult = fixture.command(
            model_ipc_method::PROFILE_DELETE,
            ModelProfileDeleteRequest {
                expected_revision: ExpectedRevision::new(5),
                model_profile_id: profile.value.id,
            },
        );
        assert!(deleted.changed);
        assert_eq!(deleted.revision, Revision::new(6));
        let binding_state = fixture
            .services
            .persistence
            .session_model_binding_state(session_id)
            .expect("binding state after deletion");
        assert_eq!(binding_state.revision, Revision::new(2));
        assert_eq!(binding_state.binding, None);
        let selector = fixture
            .services
            .projections
            .session_model_selector(
                &fixture.services.model_config.snapshot().expect("snapshot"),
                session_id,
                None,
            )
            .expect("cleared selector");
        assert_eq!(selector.binding_revision, Revision::new(2));
    }

    /// Binding refuses an Endpoint whose credential is not usable. The refusal
    /// must name that reason instead of falling into the generic commit failure,
    /// which is what a real "cleared" Endpoint produced in the field.
    #[test]
    fn binding_a_model_on_an_endpoint_without_a_credential_names_the_reason() {
        let fixture = Fixture::new();
        let provider: ModelProviderMutationResult = fixture.command(
            model_ipc_method::PROVIDER_CREATE,
            ModelProviderCreateRequest {
                expected_revision: ExpectedRevision::new(1),
                input: CreateModelProviderInput {
                    display_name: "Needs a key".to_owned(),
                    source: ModelProviderSource::Custom { catalog_provider_id: None },
                },
            },
        );
        let endpoint: ModelEndpointMutationResult = fixture.command(
            model_ipc_method::ENDPOINT_CREATE,
            ModelEndpointCreateRequest {
                expected_revision: ExpectedRevision::new(2),
                provider_id: provider.value.id,
                display_name: "Bearer endpoint".to_owned(),
                api_format: ModelApiFormat::OpenAiChatCompletions,
                base_url: "https://api.example.test/v1".to_owned(),
                discovery_mode: aura_model_domain::ModelDiscoveryMode::Disabled,
                chat_output_token_field: Some(ChatOutputTokenField::MaxTokens),
                chat_instruction_role: Some(ChatInstructionRole::System),
                auth_scheme: EndpointAuthScheme::OpenAiBearerApiKey,
                openai_organization_id: None,
                openai_project_id: None,
                anthropic_workspace_id: None,
                timeouts: ModelEndpointTimeouts {
                    connect_ms: 5_000,
                    first_event_ms: 30_000,
                    stream_idle_ms: 30_000,
                    total_ms: Some(120_000),
                },
                max_parallel_model_requests: 2,
            },
        );
        let profile: ModelProfileMutationResult = fixture.command(
            model_ipc_method::PROFILE_CREATE_MANUAL,
            ModelProfileCreateManualRequest {
                expected_revision: ExpectedRevision::new(3),
                input: AddManualModelInput {
                    endpoint_id: endpoint.value.id,
                    provider_model_name: "vendor-model".to_owned(),
                    display_name: None,
                    notes: None,
                    context_window_tokens: Some(32_000),
                },
            },
        );
        let profile: ModelProfileMutationResult = fixture.command(
            model_ipc_method::PROFILE_UPDATE,
            ModelProfileUpdateRequest {
                expected_revision: ExpectedRevision::new(4),
                model_profile_id: profile.value.id,
                display_name: profile.value.display_name,
                notes: profile.value.notes,
                enabled: true,
                request_defaults: ModelRequestDefaults {
                    max_output_tokens: Some(1_024),
                    ..ModelRequestDefaults::default()
                },
                historical_snapshot_max_tokens: profile
                    .value
                    .context
                    .historical_snapshot_max_tokens,
            },
        );
        let session_id = fixture.seed_session();
        let refusal = fixture.command_error(
            model_ipc_method::SESSION_SET_BINDING,
            SessionModelBindingSetRequest {
                expected_revision: ExpectedRevision::new(0),
                session_id,
                model_profile_id: Some(profile.value.id),
            },
        );
        assert_eq!(refusal.code.as_str(), "models.binding.credential_required");
        assert_eq!(refusal.category, ErrorCategory::Permission);
        assert!(!refusal.retryable);
        assert_eq!(
            fixture
                .services
                .persistence
                .session_model_binding_state(session_id)
                .expect("binding state after refusal")
                .binding,
            None
        );
    }
}
