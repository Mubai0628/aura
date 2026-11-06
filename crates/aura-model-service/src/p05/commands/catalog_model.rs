//! Model profiles and the catalog: a profile created from a catalog entry, a profile's link
//! updated to the entry as the catalog now lists it, and the entries a model name matches, to
//! link it to the one the user chooses. Each at the catalog revision and the model object the
//! settings page showed.
use std::collections::BTreeMap;

use aura_domain_common::{CommandInvocationId, CorrelationId, Revision, UtcTimestamp};
use aura_model_config::{
    ModelConfigError, apply_catalog_update, catalog_entries, choose_catalog_entry,
    create_model_from_catalog,
};
use aura_model_domain::{
    ModelProfileApplyCatalogUpdateRequest, ModelProfileCatalogEntriesRequest,
    ModelProfileCreateFromCatalogRequest, ModelProfileCreatedPayload, ModelProfileMutationResult,
    ModelProfileUpdatedPayload,
};
use aura_persistence::ModelProjectionDraft;
use aura_runtime_contract::IpcErrorEnvelope;
use serde_json::Value;

use crate::p05::{
    P05Services,
    config_journal::{ModelConfigCommandPlan, ModelConfigJournalError},
};

use super::{
    P05CommandDispatch, catalog_error, decode, encode, journal_error, prepared_snapshot,
    profile_changed_fields, require_command_id, user_event,
};

impl P05Services {
    pub(super) fn dispatch_create_profile_from_catalog(
        &self,
        params: &Value,
        command_invocation_id: Option<CommandInvocationId>,
        correlation_id: CorrelationId,
    ) -> Result<P05CommandDispatch, IpcErrorEnvelope> {
        let request: ModelProfileCreateFromCatalogRequest = decode(params, correlation_id)?;
        let command_invocation_id = require_command_id(command_invocation_id, correlation_id)?;
        let catalog = self.provider_catalog.current_snapshot().ok_or_else(|| {
            catalog_error(aura_model_catalog::CatalogError::Unavailable.into(), correlation_id)
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
                            .ok_or(ModelConfigError::EndpointIdNotFound(request.endpoint_id))?;
                        if endpoint.revision.value() != request.expected_endpoint_revision.value() {
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
                let profile =
                    prepared.candidate_registry.models.get(&prepared.value).cloned().ok_or(
                        ModelConfigError::InternalInvariant(
                            "catalog-created Model Profile is absent from the candidate registry",
                        ),
                    )?;
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
                let prepared_bundle =
                    prepared.prepared_bundle.ok_or(ModelConfigError::InternalInvariant(
                        "catalog-created Model Profile mutation lacks a prepared bundle",
                    ))?;
                Ok(ModelConfigCommandPlan::Publish {
                    prepared_bundle: Box::new(prepared_bundle),
                    expected_models_revision: Revision::new(prepared.previous.revision),
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

    /// Updates the catalog entry a profile is linked to, or links it to the one the user chose
    /// among those its model name matches.
    pub(super) fn dispatch_apply_catalog_update(
        &self,
        params: &Value,
        command_invocation_id: Option<CommandInvocationId>,
        correlation_id: CorrelationId,
    ) -> Result<P05CommandDispatch, IpcErrorEnvelope> {
        let request: ModelProfileApplyCatalogUpdateRequest = decode(params, correlation_id)?;
        let command_invocation_id = require_command_id(command_invocation_id, correlation_id)?;
        let catalog = self.provider_catalog.current_snapshot().ok_or_else(|| {
            catalog_error(aura_model_catalog::CatalogError::Unavailable.into(), correlation_id)
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
        let completion =
            self.config_journal
                .execute(command_invocation_id, correlation_id, |journal_id| {
                    let prepared = self.model_config.prepare_update(
                        journal_id.to_string(),
                        request.expected_models_revision.value(),
                        |registry| {
                            let profile = registry.models.get(&request.model_profile_id).ok_or(
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
                            match &request.catalog_entry {
                                Some(entry) => choose_catalog_entry(
                                    registry,
                                    request.model_profile_id,
                                    &catalog,
                                    entry,
                                    &request.expected_catalog_object_hash,
                                    UtcTimestamp::now(),
                                ),
                                None => apply_catalog_update(
                                    registry,
                                    request.model_profile_id,
                                    &catalog,
                                    &request.expected_catalog_object_hash,
                                    UtcTimestamp::now(),
                                ),
                            }
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
                        expected_models_revision: Revision::new(prepared.previous.revision),
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

    /// The catalog entries a profile's model name matches, to choose from.
    pub(super) fn dispatch_catalog_entries(
        &self,
        params: &Value,
        correlation_id: CorrelationId,
    ) -> Result<P05CommandDispatch, IpcErrorEnvelope> {
        let request: ModelProfileCatalogEntriesRequest = decode(params, correlation_id)?;
        let catalog = self.provider_catalog.current_snapshot().ok_or_else(|| {
            catalog_error(aura_model_catalog::CatalogError::Unavailable.into(), correlation_id)
        })?;
        let snapshot = self
            .model_config
            .snapshot()
            .map_err(|error| journal_error(error.into(), correlation_id))?;
        let view = catalog_entries(&snapshot.registry, request.model_profile_id, &catalog)
            .map_err(|error| journal_error(error.into(), correlation_id))?;
        Ok(P05CommandDispatch {
            value: encode(view, correlation_id)?,
            projection_changes: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use aura_domain_common::CorrelationId;
    use aura_model_config::ModelConfigError;
    use serde_json::Value;

    use super::super::journal_error;
    use crate::p05::config_journal::ModelConfigJournalError;

    fn refusal(error: ModelConfigError) -> (String, String, Option<Value>) {
        let envelope =
            journal_error(ModelConfigJournalError::ModelConfig(error), CorrelationId::new());
        (
            envelope.code.as_str().to_owned(),
            envelope.message,
            envelope.details.get("route").cloned(),
        )
    }

    /// An entry the model's name does not match, or one the catalog changed after the settings
    /// listed it, is refused with what to do next.
    #[test]
    fn a_catalog_entry_not_for_the_model_or_changed_since_listed_says_so() {
        assert_eq!(
            refusal(ModelConfigError::CatalogEntryNotForModel {
                catalog_provider_id: "openai".to_owned(),
                catalog_model_id: "gpt-other".to_owned(),
            }),
            (
                "models.catalog.entry_not_for_model".to_owned(),
                "这个目录条目和模型名称对不上，不能选用。".to_owned(),
                None
            )
        );
        let (code, message, _) = refusal(ModelConfigError::CatalogObjectHashMismatch);
        assert_eq!(code, "models.catalog.entry_changed");
        assert!(message.contains("重新打开设置"), "{message}");
    }

    /// A catalog model refused for what it is or for the route it goes by says so, and names the
    /// route; the settings page shows the message as it is.
    #[test]
    fn a_catalog_model_refused_for_its_type_or_its_route_says_why() {
        let ids = || ("opencode-go".to_owned(), "minimax-m3".to_owned());
        let (catalog_provider_id, catalog_model_id) = ids();
        assert_eq!(
            refusal(ModelConfigError::CatalogModelNotChat {
                catalog_provider_id,
                catalog_model_id
            }),
            (
                "models.catalog.model_not_chat".to_owned(),
                "这个目录条目不是对话模型，不能添加。".to_owned(),
                None
            )
        );
        let (catalog_provider_id, catalog_model_id) = ids();
        assert_eq!(
            refusal(ModelConfigError::CatalogModelOnAnotherRoute {
                catalog_provider_id,
                catalog_model_id,
                route_id: "anthropic-messages".to_owned(),
            }),
            (
                "models.catalog.model_on_another_route".to_owned(),
                "这个模型要走这家服务商的另一条连接，请添加到那条连接下。".to_owned(),
                Some(Value::String("anthropic-messages".to_owned()))
            )
        );
        let (catalog_provider_id, catalog_model_id) = ids();
        assert_eq!(
            refusal(ModelConfigError::CatalogModelRouteUnsupported {
                catalog_provider_id,
                catalog_model_id,
            }),
            (
                "models.catalog.model_route_unsupported".to_owned(),
                "这个模型走的协议 Aura 还不支持，不能添加到这家服务商的连接。".to_owned(),
                None
            )
        );
    }
}
