use std::{str::FromStr, sync::Arc};

use aura_config::{
    ConfigModule, ConfigModuleBundleEntry, ConfigSource, PreparedConfigBundle, SettingsFacade,
    SettingsFacadeError,
};
use aura_domain_common::{
    ContentHash, ContextProjectionSnapshotId, CredentialId, CredentialRevision, EndpointId,
    ModelExecutionSnapshotId, ModelProfileId, ModelStepId, PromptBundleSnapshotId, Revision,
};
use aura_model_domain::{
    CredentialBindingFingerprint, EndpointAuthScheme, ModelExecutionSnapshot, ModelRegistryConfig,
    ProviderToolCallWireBinding,
};
use thiserror::Error;

use crate::validation::{registry_from_document, registry_values};
use crate::{endpoint_fingerprint, normalize_base_url, resolve_api_url, validate_model_registry};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelExecutionCredentialBinding {
    pub credential_id: CredentialId,
    pub credential_revision: CredentialRevision,
    pub credential_binding_fingerprint: CredentialBindingFingerprint,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateModelExecutionSnapshot {
    pub model_step_id: ModelStepId,
    pub model_profile_id: ModelProfileId,
    pub context_snapshot_id: ContextProjectionSnapshotId,
    pub prompt_bundle_snapshot_id: PromptBundleSnapshotId,
    pub historical_tool_call_wire_bindings: Vec<ProviderToolCallWireBinding>,
    pub credential_binding: Option<ModelExecutionCredentialBinding>,
    /// The connection the step is sent through when it is not the profile's own — the
    /// provider's default, or a backup after a failure. Same provider and API format only.
    pub endpoint_id: Option<aura_domain_common::EndpointId>,
}

#[derive(Clone, Debug)]
pub struct ModelConfigSnapshot {
    pub revision: u64,
    pub content_hash: ContentHash,
    pub source: ConfigSource,
    pub registry: Arc<ModelRegistryConfig>,
}

/// A validated, revision-reconciled models.json mutation that has not crossed
/// the config publication forward point.
///
/// Core owns the surrounding ModelConfig Parent Journal. This value only
/// freezes the before/candidate domain state and the exact prepared file
/// bundle; it never publishes or activates the bundle by itself.
pub struct PreparedModelConfigMutation<T> {
    pub changed: bool,
    pub previous: ModelConfigSnapshot,
    pub candidate_revision: Revision,
    pub candidate_registry: Arc<ModelRegistryConfig>,
    pub prepared_bundle: Option<PreparedConfigBundle>,
    pub value: T,
}

/// A validated default-model mutation frozen into an Aura config bundle.
///
/// The models registry is read only to validate the selected Profile. The
/// resulting mutation owns only the Aura config Revision and never increments
/// the global models Revision.
pub struct PreparedDefaultModelMutation {
    pub changed: bool,
    pub models_snapshot: ModelConfigSnapshot,
    pub previous: aura_model_domain::DefaultModelSelection,
    pub candidate: aura_model_domain::DefaultModelSelection,
    pub prepared_bundle: Option<PreparedConfigBundle>,
}

/// A validated cascade deletion frozen across models.json and, when needed,
/// Aura's default-model field.
pub struct PreparedModelConfigDeletion {
    pub previous: ModelConfigSnapshot,
    pub candidate_revision: Revision,
    pub candidate_registry: Arc<ModelRegistryConfig>,
    pub expected_aura_revision: Revision,
    pub candidate_aura_revision: Revision,
    pub prepared_bundle: PreparedConfigBundle,
    pub value: aura_model_domain::DeleteModelConfigCommitResult,
}

#[derive(Debug, Error)]
pub enum ModelConfigError {
    #[error(transparent)]
    Settings(#[from] SettingsFacadeError),
    #[error("failed to decode models.json: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("model configuration is invalid: {0}")]
    Validation(#[from] crate::ModelConfigValidationError),
    #[error("model profile was not found: {0}")]
    ProfileNotFound(ModelProfileId),
    #[error("model profile is disabled: {0}")]
    ProfileDisabled(ModelProfileId),
    #[error("model endpoint was not found for profile: {0}")]
    EndpointNotFound(ModelProfileId),
    #[error("model provider was not found for profile: {0}")]
    ProviderNotFound(ModelProfileId),
    #[error("model provider was not found: {0}")]
    ProviderNotFoundById(aura_domain_common::ProviderId),
    /// A default or backup connection that is not this provider's, or named twice.
    #[error("provider connections must be the provider's own, each named once: {0}")]
    InvalidProviderConnections(aura_domain_common::ProviderId),
    #[error("model endpoint URL is invalid: {0}")]
    Url(#[from] crate::ModelUrlError),
    #[error("model endpoint was not found: {0}")]
    EndpointIdNotFound(aura_domain_common::EndpointId),
    #[error("model provider was not found for endpoint: {0}")]
    ProviderForEndpointNotFound(aura_domain_common::EndpointId),
    #[error("provider model name must not be blank")]
    BlankProviderModelName,
    #[error("endpoint {endpoint_id} already has provider model name {provider_model_name:?}")]
    DuplicateProviderModelName {
        endpoint_id: aura_domain_common::EndpointId,
        provider_model_name: String,
    },
    #[error(
        "discovery response for endpoint {endpoint_id} duplicated provider model name {provider_model_name:?}"
    )]
    DuplicateDiscoveredModelName {
        endpoint_id: aura_domain_common::EndpointId,
        provider_model_name: String,
    },
    #[error("manual model context window must be greater than zero")]
    InvalidManualContextWindow,
    #[error("aura.toml default_model_profile_id must be a ModelProfileId string")]
    InvalidDefaultModelProfileId,
    #[error("default model profile was not found: {0}")]
    DefaultProfileNotFound(ModelProfileId),
    #[error("model configuration invariant failed after a committed transaction: {0}")]
    InternalInvariant(&'static str),
    #[error("context window must be greater than zero")]
    InvalidContextWindow,
    #[error(
        "model execution credential binding must be absent for auth=none and complete after secret-bearing preflight"
    )]
    InvalidExecutionCredentialBinding,
    #[error("historical snapshot maximum must be greater than zero")]
    InvalidHistoricalSnapshotLimit,
    #[error("model profile requires an explicit or catalog-provided max output token limit: {0}")]
    OutputLimitConfigurationRequired(ModelProfileId),
    #[error("user context-window override revision is exhausted")]
    UserOverrideRevisionExhausted,
    #[error("no smaller automatic context-window candidate remains")]
    NoSmallerContextWindowCandidate,
    #[error("runtime context-window candidate must be {expected}, found {actual}")]
    InvalidRuntimeContextWindowCandidate { expected: u64, actual: u64 },
    #[error("model endpoint revision is exhausted: {0}")]
    EndpointRevisionExhausted(EndpointId),
    #[error("model endpoint revision conflict: expected {expected}, actual {actual}")]
    EndpointObjectRevisionConflict { expected: u64, actual: u64 },
    #[error("model profile revision is exhausted: {0}")]
    ProfileRevisionExhausted(ModelProfileId),
    #[error("model profile revision conflict: expected {expected}, actual {actual}")]
    ProfileObjectRevisionConflict { expected: u64, actual: u64 },
    #[error("model notes exceed 4096 UTF-8 bytes")]
    ModelNotesTooLarge,
    #[error("model notes contain a prohibited control character")]
    ModelNotesInvalidCharacter,
    #[error("provider catalog entry was not found: {0}")]
    CatalogProviderNotFound(String),
    #[error("provider catalog model was not found: {catalog_provider_id}/{catalog_model_id}")]
    CatalogModelNotFound { catalog_provider_id: String, catalog_model_id: String },
    #[error("endpoint provider is not bound to catalog provider {catalog_provider_id}")]
    CatalogProviderBindingMismatch { catalog_provider_id: String },
    #[error("catalog model object changed before the mutation was committed")]
    CatalogObjectHashMismatch,
    #[error("model profile has no catalog snapshot: {0}")]
    CatalogSnapshotMissing(ModelProfileId),
    #[error("output-token field alias must switch to the other supported field")]
    InvalidOutputTokenFieldAlias,
    #[error(
        "output-token alias model {provider_model_name:?} was not found on endpoint {endpoint_id}"
    )]
    OutputTokenAliasProfileNotFound { endpoint_id: EndpointId, provider_model_name: String },
    #[error("output-token aliases require an OpenAI Chat endpoint: {0}")]
    OutputTokenAliasFormatMismatch(EndpointId),
    #[error("output-token alias endpoint has no configured field: {0}")]
    OutputTokenAliasConfiguredFieldMissing(EndpointId),
    #[error("output-token alias configured field changed before persistence: {0}")]
    OutputTokenAliasConfiguredFieldChanged(EndpointId),
    #[error(
        "compatibility.output_token_field_conflict: endpoint {endpoint_id}, model {provider_model_name:?}"
    )]
    OutputTokenFieldAliasConflict { endpoint_id: EndpointId, provider_model_name: String },
    #[error("output-token alias endpoint fingerprint changed before persistence: {0}")]
    OutputTokenAliasEndpointFingerprintChanged(EndpointId),
}

pub struct ModelConfigService {
    settings: Arc<SettingsFacade>,
}

impl ModelConfigService {
    #[must_use]
    pub fn new(settings: Arc<SettingsFacade>) -> Self {
        Self { settings }
    }

    pub fn snapshot(&self) -> Result<ModelConfigSnapshot, ModelConfigError> {
        let snapshot = self.settings.snapshot(ConfigModule::Models)?;
        let registry = registry_from_document(&snapshot.document)?;
        validate_model_registry(&registry)?;
        Ok(ModelConfigSnapshot {
            revision: snapshot.document.revision,
            content_hash: snapshot.content_hash.clone(),
            source: snapshot.source,
            registry: Arc::new(registry),
        })
    }

    pub fn update<F>(
        &self,
        expected_revision: u64,
        mutate: F,
    ) -> Result<ModelConfigSnapshot, ModelConfigError>
    where
        F: FnOnce(&mut ModelRegistryConfig) -> Result<(), ModelConfigError>,
    {
        let current = self.snapshot()?;
        if current.revision != expected_revision {
            return Err(ModelConfigError::Settings(SettingsFacadeError::RevisionConflict {
                module: ConfigModule::Models,
                expected: expected_revision,
                actual: current.revision,
            }));
        }
        let mut candidate = (*current.registry).clone();
        mutate(&mut candidate)?;
        reconcile_registry_revisions(&current.registry, &mut candidate)?;
        validate_model_registry(&candidate)?;
        if candidate == *current.registry {
            return Ok(current);
        }
        let values = registry_values(&candidate)?;
        let updated = self.settings.update(
            ConfigModule::Models,
            expected_revision,
            move |document_values| *document_values = values,
        )?;
        let registry = registry_from_document(&updated.document)?;
        Ok(ModelConfigSnapshot {
            revision: updated.document.revision,
            content_hash: updated.content_hash.clone(),
            source: updated.source,
            registry: Arc::new(registry),
        })
    }

    pub fn prepare_update<T, F>(
        &self,
        transaction_id: String,
        expected_revision: u64,
        mutate: F,
    ) -> Result<PreparedModelConfigMutation<T>, ModelConfigError>
    where
        F: FnOnce(&mut ModelRegistryConfig) -> Result<T, ModelConfigError>,
    {
        let current = self.snapshot()?;
        if current.revision != expected_revision {
            return Err(ModelConfigError::Settings(SettingsFacadeError::RevisionConflict {
                module: ConfigModule::Models,
                expected: expected_revision,
                actual: current.revision,
            }));
        }
        let mut candidate = (*current.registry).clone();
        let value = mutate(&mut candidate)?;
        reconcile_registry_revisions(&current.registry, &mut candidate)?;
        validate_model_registry(&candidate)?;
        let changed = candidate != *current.registry;
        let candidate_revision = if changed {
            Revision::new(expected_revision.checked_add(1).ok_or(ModelConfigError::Settings(
                SettingsFacadeError::RevisionExhausted { module: ConfigModule::Models },
            ))?)
        } else {
            Revision::new(expected_revision)
        };
        let prepared_bundle = if changed {
            Some(self.settings.prepare_module_bundle(
                transaction_id,
                vec![ConfigModuleBundleEntry {
                    module: ConfigModule::Models,
                    expected_revision,
                    replacement_values: Some(registry_values(&candidate)?),
                }],
            )?)
        } else {
            None
        };
        Ok(PreparedModelConfigMutation {
            changed,
            previous: current,
            candidate_revision,
            candidate_registry: Arc::new(candidate),
            prepared_bundle,
            value,
        })
    }

    pub fn prepare_set_default_model(
        &self,
        transaction_id: String,
        expected_aura_revision: u64,
        model_profile_id: Option<ModelProfileId>,
    ) -> Result<PreparedDefaultModelMutation, ModelConfigError> {
        let models_snapshot = self.snapshot()?;
        if let Some(model_profile_id) = model_profile_id {
            let profile = models_snapshot
                .registry
                .models
                .get(&model_profile_id)
                .ok_or(ModelConfigError::DefaultProfileNotFound(model_profile_id))?;
            if !profile.enabled {
                return Err(ModelConfigError::ProfileDisabled(model_profile_id));
            }
            if profile.effective_max_output_tokens().is_none_or(|value| value == 0) {
                return Err(ModelConfigError::OutputLimitConfigurationRequired(model_profile_id));
            }
        }

        let aura = self.settings.snapshot(ConfigModule::Aura)?;
        if aura.document.revision != expected_aura_revision {
            return Err(ModelConfigError::Settings(SettingsFacadeError::RevisionConflict {
                module: ConfigModule::Aura,
                expected: expected_aura_revision,
                actual: aura.document.revision,
            }));
        }
        let previous_model_profile_id = parse_default_model_profile_id(&aura.document.values)?;
        let previous = aura_model_domain::DefaultModelSelection {
            model_profile_id: previous_model_profile_id,
            aura_config_revision: aura.document.revision,
        };
        if previous_model_profile_id == model_profile_id {
            return Ok(PreparedDefaultModelMutation {
                changed: false,
                models_snapshot,
                candidate: previous.clone(),
                previous,
                prepared_bundle: None,
            });
        }

        let mut aura_values = aura.document.values.clone();
        if let Some(model_profile_id) = model_profile_id {
            aura_values.insert(
                "default_model_profile_id".to_owned(),
                serde_json::Value::String(model_profile_id.to_string()),
            );
        } else {
            aura_values.remove("default_model_profile_id");
        }
        let candidate_revision =
            expected_aura_revision.checked_add(1).ok_or(ModelConfigError::Settings(
                SettingsFacadeError::RevisionExhausted { module: ConfigModule::Aura },
            ))?;
        let prepared_bundle = self.settings.prepare_module_bundle(
            transaction_id,
            vec![ConfigModuleBundleEntry {
                module: ConfigModule::Aura,
                expected_revision: expected_aura_revision,
                replacement_values: Some(aura_values),
            }],
        )?;
        Ok(PreparedDefaultModelMutation {
            changed: true,
            models_snapshot,
            previous,
            candidate: aura_model_domain::DefaultModelSelection {
                model_profile_id,
                aura_config_revision: candidate_revision,
            },
            prepared_bundle: Some(prepared_bundle),
        })
    }

    pub fn prepare_delete_model_profile(
        &self,
        transaction_id: String,
        expected_models_revision: u64,
        model_profile_id: ModelProfileId,
    ) -> Result<PreparedModelConfigDeletion, ModelConfigError> {
        self.prepare_deletion(transaction_id, expected_models_revision, |registry| {
            crate::delete_model_profile(registry, model_profile_id)
        })
    }

    pub fn prepare_delete_endpoint(
        &self,
        transaction_id: String,
        expected_models_revision: u64,
        endpoint_id: aura_domain_common::EndpointId,
    ) -> Result<PreparedModelConfigDeletion, ModelConfigError> {
        self.prepare_deletion(transaction_id, expected_models_revision, |registry| {
            crate::delete_endpoint(registry, endpoint_id)
        })
    }

    pub fn prepare_delete_provider(
        &self,
        transaction_id: String,
        expected_models_revision: u64,
        provider_id: aura_domain_common::ProviderId,
    ) -> Result<PreparedModelConfigDeletion, ModelConfigError> {
        self.prepare_deletion(transaction_id, expected_models_revision, |registry| {
            crate::delete_provider(registry, provider_id)
        })
    }

    fn prepare_deletion<F>(
        &self,
        transaction_id: String,
        expected_models_revision: u64,
        delete: F,
    ) -> Result<PreparedModelConfigDeletion, ModelConfigError>
    where
        F: FnOnce(
            &mut ModelRegistryConfig,
        ) -> Result<aura_model_domain::DeleteModelConfigResult, ModelConfigError>,
    {
        let previous = self.snapshot()?;
        if previous.revision != expected_models_revision {
            return Err(ModelConfigError::Settings(SettingsFacadeError::RevisionConflict {
                module: ConfigModule::Models,
                expected: expected_models_revision,
                actual: previous.revision,
            }));
        }
        let mut candidate = (*previous.registry).clone();
        let deleted = delete(&mut candidate)?;
        reconcile_registry_revisions(&previous.registry, &mut candidate)?;
        validate_model_registry(&candidate)?;

        let aura = self.settings.snapshot(ConfigModule::Aura)?;
        let expected_aura_revision = Revision::new(aura.document.revision);
        let default = parse_default_model_profile_id(&aura.document.values)?;
        let default_model_cleared =
            default.is_some_and(|default| deleted.deleted_model_profile_ids.contains(&default));
        let mut entries = vec![ConfigModuleBundleEntry {
            module: ConfigModule::Models,
            expected_revision: expected_models_revision,
            replacement_values: Some(registry_values(&candidate)?),
        }];
        let candidate_aura_revision = if default_model_cleared {
            let mut aura_values = aura.document.values.clone();
            aura_values.remove("default_model_profile_id");
            entries.push(ConfigModuleBundleEntry {
                module: ConfigModule::Aura,
                expected_revision: aura.document.revision,
                replacement_values: Some(aura_values),
            });
            Revision::new(aura.document.revision.checked_add(1).ok_or(
                ModelConfigError::Settings(SettingsFacadeError::RevisionExhausted {
                    module: ConfigModule::Aura,
                }),
            )?)
        } else {
            expected_aura_revision
        };
        let candidate_revision = Revision::new(expected_models_revision.checked_add(1).ok_or(
            ModelConfigError::Settings(SettingsFacadeError::RevisionExhausted {
                module: ConfigModule::Models,
            }),
        )?);
        let prepared_bundle = self.settings.prepare_module_bundle(transaction_id, entries)?;
        Ok(PreparedModelConfigDeletion {
            previous,
            candidate_revision,
            candidate_registry: Arc::new(candidate),
            expected_aura_revision,
            candidate_aura_revision,
            prepared_bundle,
            value: aura_model_domain::DeleteModelConfigCommitResult {
                deleted,
                default_model_cleared,
                credential_cleanup_pending: false,
            },
        })
    }

    pub fn publish_prepared_update(
        &self,
        prepared: &PreparedConfigBundle,
    ) -> Result<(), ModelConfigError> {
        self.settings.publish_prepared_module_bundle(prepared)?;
        Ok(())
    }

    pub fn activate_prepared_update(
        &self,
        prepared: &PreparedConfigBundle,
    ) -> Result<ModelConfigSnapshot, ModelConfigError> {
        let updated = self.settings.activate_published_module_bundle(prepared)?;
        let models =
            updated.get(&ConfigModule::Models).ok_or(ModelConfigError::InternalInvariant(
                "prepared models mutation did not activate models.json",
            ))?;
        let registry = registry_from_document(&models.document)?;
        validate_model_registry(&registry)?;
        Ok(ModelConfigSnapshot {
            revision: models.document.revision,
            content_hash: models.content_hash.clone(),
            source: models.source,
            registry: Arc::new(registry),
        })
    }

    pub fn abort_prepared_update(
        &self,
        prepared: &PreparedConfigBundle,
    ) -> Result<(), ModelConfigError> {
        self.settings.abort_prepared_module_bundle(prepared)?;
        Ok(())
    }

    pub fn create_execution_snapshot(
        &self,
        input: CreateModelExecutionSnapshot,
    ) -> Result<ModelExecutionSnapshot, ModelConfigError> {
        Self::freeze_execution_snapshot(&self.snapshot()?, input)
    }

    /// Resolve all request fields from one validated configuration revision.
    /// Callers which already resolved endpoint credentials must use that same
    /// revision so a concurrent settings edit cannot mix two endpoints.
    pub fn freeze_execution_snapshot(
        config: &ModelConfigSnapshot,
        input: CreateModelExecutionSnapshot,
    ) -> Result<ModelExecutionSnapshot, ModelConfigError> {
        let CreateModelExecutionSnapshot {
            model_step_id,
            model_profile_id,
            context_snapshot_id,
            prompt_bundle_snapshot_id,
            historical_tool_call_wire_bindings,
            credential_binding,
            endpoint_id: routed_endpoint,
        } = input;
        let model = config
            .registry
            .models
            .get(&model_profile_id)
            .ok_or(ModelConfigError::ProfileNotFound(model_profile_id))?;
        if !model.enabled {
            return Err(ModelConfigError::ProfileDisabled(model_profile_id));
        }
        let (endpoint, provider) =
            crate::routing::step_connection(&config.registry, model, routed_endpoint)?;
        let effective_max_output_tokens = model
            .effective_max_output_tokens()
            .filter(|value| *value > 0)
            .ok_or(ModelConfigError::OutputLimitConfigurationRequired(model_profile_id))?;
        let (chat_output_token_field, chat_instruction_role) = match endpoint.api_format {
            aura_model_domain::ModelApiFormat::OpenAiChatCompletions => (
                model.request_defaults.chat_output_token_field.or(endpoint.chat_output_token_field),
                model.request_defaults.chat_instruction_role.or(endpoint.chat_instruction_role),
            ),
            aura_model_domain::ModelApiFormat::OpenAiResponses
            | aura_model_domain::ModelApiFormat::AnthropicMessages => (None, None),
        };
        if endpoint.api_format == aura_model_domain::ModelApiFormat::OpenAiChatCompletions
            && (chat_output_token_field.is_none() || chat_instruction_role.is_none())
        {
            return Err(ModelConfigError::InternalInvariant(
                "validated Chat endpoint did not resolve output-token and instruction fields",
            ));
        }
        let credential_binding_is_valid = match endpoint.auth_scheme {
            EndpointAuthScheme::None => credential_binding.is_none(),
            EndpointAuthScheme::OpenAiBearerApiKey | EndpointAuthScheme::AnthropicApiKey => {
                credential_binding.is_some()
            }
        };
        if !credential_binding_is_valid {
            return Err(ModelConfigError::InvalidExecutionCredentialBinding);
        }
        let (credential_id, credential_revision, credential_binding_fingerprint) =
            credential_binding.map_or((None, None, None), |binding| {
                (
                    Some(binding.credential_id),
                    Some(binding.credential_revision),
                    Some(binding.credential_binding_fingerprint),
                )
            });
        let normalized_base = normalize_base_url(&endpoint.base_url)?;
        let request_url = resolve_api_url(&endpoint.base_url, endpoint.api_format)?;
        Ok(ModelExecutionSnapshot {
            id: ModelExecutionSnapshotId::new(),
            model_step_id,
            provider_id: provider.id,
            endpoint_id: endpoint.id,
            model_profile_id: model.id,
            provider_display_name: provider.display_name.clone(),
            endpoint_display_name: endpoint.display_name.clone(),
            model_name: model.provider_model_name.clone(),
            readable_ref: format!(
                "{}/{}/{}",
                provider.slug, endpoint.slug, model.provider_model_name
            ),
            api_format: endpoint.api_format,
            auth_scheme: endpoint.auth_scheme,
            base_url: normalized_base.to_string(),
            resolved_request_url: request_url.to_string(),
            openai_organization_id: endpoint.openai_organization_id.clone(),
            openai_project_id: endpoint.openai_project_id.clone(),
            anthropic_workspace_id: endpoint.anthropic_workspace_id.clone(),
            chat_output_token_field,
            chat_instruction_role,
            endpoint_config_revision: endpoint.revision.value(),
            model_config_revision: config.revision,
            runtime_effective_window_tokens: model.context.runtime_effective_window_tokens,
            historical_snapshot_max_tokens: model.context.historical_snapshot_max_tokens,
            request_options: crate::request_options::resolve(
                model,
                endpoint.api_format,
                effective_max_output_tokens,
            ),
            historical_tool_call_wire_bindings,
            credential_id,
            credential_revision,
            credential_binding_fingerprint,
            context_snapshot_id,
            prompt_bundle_snapshot_id,
            endpoint_fingerprint: endpoint_fingerprint(endpoint)?,
            created_at: aura_domain_common::UtcTimestamp::now(),
        })
    }

    pub fn apply_discovery(
        &self,
        expected_revision: u64,
        endpoint_id: aura_domain_common::EndpointId,
        provider_model_names: &[String],
        catalog: Option<&aura_model_domain::ProviderCatalogSnapshot>,
        discovered_at: aura_domain_common::UtcTimestamp,
    ) -> Result<(ModelConfigSnapshot, aura_model_domain::ModelDiscoveryMergeResult), ModelConfigError>
    {
        let mut result = None;
        let snapshot = self.update(expected_revision, |candidate| {
            result = Some(crate::merge_discovery_results(
                candidate,
                endpoint_id,
                provider_model_names,
                catalog,
                discovered_at,
            )?);
            Ok(())
        })?;
        let result = result
            .ok_or(ModelConfigError::InternalInvariant("discovery merge produced no result"))?;
        Ok((snapshot, result))
    }

    pub fn create_manual_model(
        &self,
        expected_revision: u64,
        input: aura_model_domain::AddManualModelInput,
        created_at: aura_domain_common::UtcTimestamp,
    ) -> Result<(ModelConfigSnapshot, aura_model_domain::ManualModelCreateResult), ModelConfigError>
    {
        let mut result = None;
        let snapshot = self.update(expected_revision, |candidate| {
            result = Some(crate::add_manual_model(candidate, input, created_at)?);
            Ok(())
        })?;
        let result = result.ok_or(ModelConfigError::InternalInvariant(
            "manual model creation produced no result",
        ))?;
        Ok((snapshot, result))
    }

    pub fn create_provider(
        &self,
        expected_revision: u64,
        input: aura_model_domain::CreateModelProviderInput,
        created_at: aura_domain_common::UtcTimestamp,
    ) -> Result<(ModelConfigSnapshot, aura_domain_common::ProviderId), ModelConfigError> {
        let mut provider_id = None;
        let snapshot = self.update(expected_revision, |candidate| {
            provider_id = Some(crate::create_provider(candidate, input, created_at)?);
            Ok(())
        })?;
        let provider_id = provider_id
            .ok_or(ModelConfigError::InternalInvariant("provider creation produced no id"))?;
        Ok((snapshot, provider_id))
    }

    pub fn rename_provider(
        &self,
        expected_revision: u64,
        provider_id: aura_domain_common::ProviderId,
        display_name: String,
        updated_at: aura_domain_common::UtcTimestamp,
    ) -> Result<ModelConfigSnapshot, ModelConfigError> {
        self.update(expected_revision, |candidate| {
            crate::rename_provider(candidate, provider_id, display_name, updated_at)
        })
    }

    pub fn create_endpoint(
        &self,
        expected_revision: u64,
        input: aura_model_domain::CreateModelEndpointInput,
        created_at: aura_domain_common::UtcTimestamp,
    ) -> Result<(ModelConfigSnapshot, aura_domain_common::EndpointId), ModelConfigError> {
        let mut endpoint_id = None;
        let snapshot = self.update(expected_revision, |candidate| {
            endpoint_id = Some(crate::create_endpoint(candidate, input, created_at)?);
            Ok(())
        })?;
        let endpoint_id = endpoint_id
            .ok_or(ModelConfigError::InternalInvariant("endpoint creation produced no id"))?;
        Ok((snapshot, endpoint_id))
    }

    pub fn update_endpoint(
        &self,
        expected_revision: u64,
        input: aura_model_domain::UpdateModelEndpointInput,
        updated_at: aura_domain_common::UtcTimestamp,
    ) -> Result<ModelConfigSnapshot, ModelConfigError> {
        self.update(expected_revision, |candidate| {
            crate::update_endpoint(candidate, input, updated_at)
        })
    }

    pub fn rename_model_profile(
        &self,
        expected_revision: u64,
        model_profile_id: ModelProfileId,
        display_name: String,
        updated_at: aura_domain_common::UtcTimestamp,
    ) -> Result<ModelConfigSnapshot, ModelConfigError> {
        self.update(expected_revision, |candidate| {
            crate::rename_model_profile(candidate, model_profile_id, display_name, updated_at)
        })
    }

    pub fn set_model_subagent_availability(
        &self,
        expected_revision: u64,
        model_profile_id: ModelProfileId,
        available: bool,
        updated_at: aura_domain_common::UtcTimestamp,
    ) -> Result<ModelConfigSnapshot, ModelConfigError> {
        self.update(expected_revision, |candidate| {
            crate::set_model_subagent_availability(
                candidate,
                model_profile_id,
                available,
                updated_at,
            )
        })
    }

    pub fn set_user_context_window(
        &self,
        expected_revision: u64,
        model_profile_id: ModelProfileId,
        window_tokens: Option<u64>,
        updated_at: aura_domain_common::UtcTimestamp,
    ) -> Result<(ModelConfigSnapshot, aura_model_domain::ContextWindowUpdateResult), ModelConfigError>
    {
        let mut result = None;
        let snapshot = self.update(expected_revision, |candidate| {
            result = Some(crate::set_user_context_window(
                candidate,
                model_profile_id,
                window_tokens,
                updated_at,
            )?);
            Ok(())
        })?;
        let result = result.ok_or(ModelConfigError::InternalInvariant(
            "user context-window update produced no result",
        ))?;
        Ok((snapshot, result))
    }

    pub fn apply_runtime_context_window_correction(
        &self,
        expected_revision: u64,
        model_profile_id: ModelProfileId,
        corrected_window_tokens: u64,
        provider_error_ref: aura_domain_common::ArtifactId,
        corrected_at: aura_domain_common::UtcTimestamp,
    ) -> Result<(ModelConfigSnapshot, aura_model_domain::ContextWindowUpdateResult), ModelConfigError>
    {
        let mut result = None;
        let snapshot = self.update(expected_revision, |candidate| {
            result = Some(crate::apply_runtime_context_window_correction(
                candidate,
                model_profile_id,
                corrected_window_tokens,
                provider_error_ref,
                corrected_at,
            )?);
            Ok(())
        })?;
        let result = result.ok_or(ModelConfigError::InternalInvariant(
            "runtime context-window correction produced no result",
        ))?;
        Ok((snapshot, result))
    }

    pub fn reset_runtime_context_window_correction(
        &self,
        expected_revision: u64,
        model_profile_id: ModelProfileId,
        updated_at: aura_domain_common::UtcTimestamp,
    ) -> Result<(ModelConfigSnapshot, aura_model_domain::ContextWindowUpdateResult), ModelConfigError>
    {
        let mut result = None;
        let snapshot = self.update(expected_revision, |candidate| {
            result = Some(crate::reset_runtime_context_window_correction(
                candidate,
                model_profile_id,
                updated_at,
            )?);
            Ok(())
        })?;
        let result = result.ok_or(ModelConfigError::InternalInvariant(
            "runtime context-window reset produced no result",
        ))?;
        Ok((snapshot, result))
    }

    pub fn set_provider_current_models_subagent_availability(
        &self,
        expected_revision: u64,
        provider_id: aura_domain_common::ProviderId,
        available: bool,
        updated_at: aura_domain_common::UtcTimestamp,
    ) -> Result<(ModelConfigSnapshot, Vec<ModelProfileId>), ModelConfigError> {
        let mut changed = None;
        let snapshot = self.update(expected_revision, |candidate| {
            changed = Some(crate::set_provider_current_models_subagent_availability(
                candidate,
                provider_id,
                available,
                updated_at,
            )?);
            Ok(())
        })?;
        let changed = changed.ok_or(ModelConfigError::InternalInvariant(
            "provider model batch update produced no result",
        ))?;
        Ok((snapshot, changed))
    }

    pub fn default_model_selection(
        &self,
    ) -> Result<aura_model_domain::DefaultModelSelection, ModelConfigError> {
        let aura = self.settings.snapshot(ConfigModule::Aura)?;
        Ok(aura_model_domain::DefaultModelSelection {
            model_profile_id: parse_default_model_profile_id(&aura.document.values)?,
            aura_config_revision: aura.document.revision,
        })
    }

    pub fn set_default_model(
        &self,
        expected_models_revision: u64,
        expected_aura_revision: u64,
        model_profile_id: Option<ModelProfileId>,
    ) -> Result<aura_model_domain::DefaultModelSelection, ModelConfigError> {
        let models = self.snapshot()?;
        if let Some(model_profile_id) = model_profile_id {
            let model = models
                .registry
                .models
                .get(&model_profile_id)
                .ok_or(ModelConfigError::DefaultProfileNotFound(model_profile_id))?;
            if model.effective_max_output_tokens().is_none_or(|value| value == 0) {
                return Err(ModelConfigError::OutputLimitConfigurationRequired(model_profile_id));
            }
        }
        let aura = self.settings.snapshot(ConfigModule::Aura)?;
        let mut aura_values = aura.document.values.clone();
        if let Some(model_profile_id) = model_profile_id {
            aura_values.insert(
                "default_model_profile_id".to_owned(),
                serde_json::Value::String(model_profile_id.to_string()),
            );
        } else {
            aura_values.remove("default_model_profile_id");
        }
        let updated = self.settings.commit_module_bundle(vec![
            ConfigModuleBundleEntry {
                module: ConfigModule::Aura,
                expected_revision: expected_aura_revision,
                replacement_values: Some(aura_values),
            },
            ConfigModuleBundleEntry {
                module: ConfigModule::Models,
                expected_revision: expected_models_revision,
                replacement_values: None,
            },
        ])?;
        let aura = updated.get(&ConfigModule::Aura).ok_or(ModelConfigError::InternalInvariant(
            "default-model transaction did not return aura.toml",
        ))?;
        Ok(aura_model_domain::DefaultModelSelection {
            model_profile_id,
            aura_config_revision: aura.document.revision,
        })
    }

    pub fn delete_model_profile(
        &self,
        expected_models_revision: u64,
        expected_aura_revision: u64,
        model_profile_id: ModelProfileId,
    ) -> Result<
        (ModelConfigSnapshot, aura_model_domain::DeleteModelConfigCommitResult),
        ModelConfigError,
    > {
        self.commit_deletion(expected_models_revision, expected_aura_revision, |candidate| {
            crate::delete_model_profile(candidate, model_profile_id)
        })
    }

    pub fn delete_endpoint(
        &self,
        expected_models_revision: u64,
        expected_aura_revision: u64,
        endpoint_id: aura_domain_common::EndpointId,
    ) -> Result<
        (ModelConfigSnapshot, aura_model_domain::DeleteModelConfigCommitResult),
        ModelConfigError,
    > {
        self.commit_deletion(expected_models_revision, expected_aura_revision, |candidate| {
            crate::delete_endpoint(candidate, endpoint_id)
        })
    }

    pub fn delete_provider(
        &self,
        expected_models_revision: u64,
        expected_aura_revision: u64,
        provider_id: aura_domain_common::ProviderId,
    ) -> Result<
        (ModelConfigSnapshot, aura_model_domain::DeleteModelConfigCommitResult),
        ModelConfigError,
    > {
        self.commit_deletion(expected_models_revision, expected_aura_revision, |candidate| {
            crate::delete_provider(candidate, provider_id)
        })
    }

    fn commit_deletion<F>(
        &self,
        expected_models_revision: u64,
        expected_aura_revision: u64,
        delete: F,
    ) -> Result<
        (ModelConfigSnapshot, aura_model_domain::DeleteModelConfigCommitResult),
        ModelConfigError,
    >
    where
        F: FnOnce(
            &mut ModelRegistryConfig,
        ) -> Result<aura_model_domain::DeleteModelConfigResult, ModelConfigError>,
    {
        let current_models = self.snapshot()?;
        let mut registry = (*current_models.registry).clone();
        let deleted = delete(&mut registry)?;
        reconcile_registry_revisions(&current_models.registry, &mut registry)?;
        validate_model_registry(&registry)?;
        let model_values = registry_values(&registry)?;
        let aura = self.settings.snapshot(ConfigModule::Aura)?;
        let default = parse_default_model_profile_id(&aura.document.values)?;
        let default_model_cleared =
            default.is_some_and(|default| deleted.deleted_model_profile_ids.contains(&default));
        let mut aura_values = aura.document.values.clone();
        if default_model_cleared {
            aura_values.remove("default_model_profile_id");
        }
        let updated = self.settings.commit_module_bundle(vec![
            ConfigModuleBundleEntry {
                module: ConfigModule::Aura,
                expected_revision: expected_aura_revision,
                replacement_values: default_model_cleared.then_some(aura_values),
            },
            ConfigModuleBundleEntry {
                module: ConfigModule::Models,
                expected_revision: expected_models_revision,
                replacement_values: Some(model_values),
            },
        ])?;
        let models =
            updated.get(&ConfigModule::Models).ok_or(ModelConfigError::InternalInvariant(
                "model deletion transaction did not return models.json",
            ))?;
        let registry = registry_from_document(&models.document)?;
        Ok((
            ModelConfigSnapshot {
                revision: models.document.revision,
                content_hash: models.content_hash.clone(),
                source: models.source,
                registry: Arc::new(registry),
            },
            aura_model_domain::DeleteModelConfigCommitResult {
                deleted,
                default_model_cleared,
                credential_cleanup_pending: false,
            },
        ))
    }
}

fn reconcile_registry_revisions(
    current: &ModelRegistryConfig,
    candidate: &mut ModelRegistryConfig,
) -> Result<(), ModelConfigError> {
    for (provider_id, provider) in &mut candidate.providers {
        let Some(previous) = current.providers.get(provider_id) else {
            continue;
        };
        if provider_semantically_equal(previous, provider) {
            *provider = previous.clone();
        }
    }

    for (endpoint_id, endpoint) in &mut candidate.endpoints {
        let Some(previous) = current.endpoints.get(endpoint_id) else {
            endpoint.revision = Revision::INITIAL;
            continue;
        };
        if endpoint_semantically_equal(previous, endpoint) {
            *endpoint = previous.clone();
            continue;
        }
        endpoint.revision = Revision::new(
            previous
                .revision
                .value()
                .checked_add(1)
                .ok_or(ModelConfigError::EndpointRevisionExhausted(*endpoint_id))?,
        );
    }

    for (model_profile_id, model) in &mut candidate.models {
        let Some(previous) = current.models.get(model_profile_id) else {
            model.revision = Revision::INITIAL;
            continue;
        };
        if model_profile_semantically_equal(previous, model) {
            *model = previous.clone();
            continue;
        }
        model.revision = Revision::new(
            previous
                .revision
                .value()
                .checked_add(1)
                .ok_or(ModelConfigError::ProfileRevisionExhausted(*model_profile_id))?,
        );
    }
    Ok(())
}

fn provider_semantically_equal(
    previous: &aura_model_domain::ModelProviderConfig,
    candidate: &aura_model_domain::ModelProviderConfig,
) -> bool {
    let mut normalized = candidate.clone();
    normalized.created_at = previous.created_at;
    normalized.updated_at = previous.updated_at;
    normalized == *previous
}

fn endpoint_semantically_equal(
    previous: &aura_model_domain::ModelEndpointConfig,
    candidate: &aura_model_domain::ModelEndpointConfig,
) -> bool {
    let mut normalized = candidate.clone();
    normalized.revision = previous.revision;
    normalized.created_at = previous.created_at;
    normalized.updated_at = previous.updated_at;
    normalized == *previous
}

fn model_profile_semantically_equal(
    previous: &aura_model_domain::ModelProfileConfig,
    candidate: &aura_model_domain::ModelProfileConfig,
) -> bool {
    let mut normalized = candidate.clone();
    normalized.revision = previous.revision;
    normalized.created_at = previous.created_at;
    normalized.updated_at = previous.updated_at;
    if let (Some(previous_catalog), Some(candidate_catalog)) =
        (previous.catalog.as_ref(), normalized.catalog.as_mut())
    {
        candidate_catalog.matched_at = previous_catalog.matched_at;
    }
    normalized == *previous
}

fn parse_default_model_profile_id(
    values: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Result<Option<ModelProfileId>, ModelConfigError> {
    let Some(value) = values.get("default_model_profile_id") else {
        return Ok(None);
    };
    let value = value.as_str().ok_or(ModelConfigError::InvalidDefaultModelProfileId)?;
    ModelProfileId::from_str(value)
        .map(Some)
        .map_err(|_| ModelConfigError::InvalidDefaultModelProfileId)
}
