use std::{collections::BTreeMap, sync::Arc};

use aura_config::{AuraPaths, ConfigModule, ModuleConfigValidator, SettingsFacade};
use aura_domain_common::{
    CommandInvocationId, ContentHash, CorrelationId, CredentialId, EndpointId, MutationJournalId,
    Revision, RuntimeInstanceId, UtcTimestamp,
};
use aura_model_config::{ModelConfigService, ModelConfigValidator, create_provider};
use aura_model_domain::{
    CreateModelProviderInput, CredentialBindingFingerprint, EndpointAuthScheme,
    ModelCredentialSlot, ModelProviderSource, ModelRegistryConfig, ModelSettingsProjection,
};
use aura_permission_domain::NormalizedOrigin;
use aura_persistence::{
    CreateModelCredentialSlot, ModelCredentialDeletionReason, ModelCredentialStorageBinding,
    ModelProjectionDraft, PersistenceCoordinator, PersistenceOptions, PersistenceStartup,
    PersistentEventDraft, PrepareMutationJournal,
};
use serde_json::json;

use super::super::{
    ModelConfigCommandCompletion, ModelConfigCommandPlan, ModelConfigCredentialCommandPlan,
    ModelConfigCredentialDeleteCommandPlan, ModelConfigJournalCoordinator, ModelConfigJournalError,
};

const LEASE_HOLDER_KIND: &str = "test_model_config_lease_holder";

pub(super) struct Fixture {
    _directory: tempfile::TempDir,
    pub(super) settings: Arc<SettingsFacade>,
    pub(super) persistence: Arc<PersistenceCoordinator>,
    pub(super) model_config: ModelConfigService,
    pub(super) coordinator: ModelConfigJournalCoordinator,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let directory = tempfile::tempdir().expect("temporary model config root");
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
        Self {
            _directory: directory,
            model_config: ModelConfigService::new(Arc::clone(&settings)),
            coordinator: ModelConfigJournalCoordinator::new(
                Arc::clone(&persistence),
                Arc::clone(&settings),
            ),
            settings,
            persistence,
        }
    }

    pub(super) fn model_settings_projection(
        &self,
        models_revision: Revision,
        registry: ModelRegistryConfig,
    ) -> ModelProjectionDraft {
        let aura = self.settings.snapshot(ConfigModule::Aura).expect("Aura config snapshot");
        ModelProjectionDraft::ModelSettings(ModelSettingsProjection {
            models_revision,
            aura_revision: Revision::new(aura.document.revision),
            registry,
            default_model_profile_id: None,
            active_credentials: Vec::new(),
            dormant_credentials: Vec::new(),
            reasoning_efforts: Default::default(),
            speed_modes: Default::default(),
        })
    }

    pub(super) fn models_revision(&self) -> u64 {
        self.model_config.snapshot().expect("models snapshot").revision
    }

    pub(super) fn provider_count(&self) -> usize {
        self.model_config.snapshot().expect("models snapshot").registry.providers.len()
    }

    /// Reserves a models bundle that creates one Provider, named by the journal id.
    pub(super) fn provider_plan(
        &self,
        journal_id: MutationJournalId,
        correlation_id: CorrelationId,
        display_name: &str,
    ) -> Result<ModelConfigCommandPlan, ModelConfigJournalError> {
        let revision = self.models_revision();
        let prepared =
            self.model_config.prepare_update(journal_id.to_string(), revision, |registry| {
                create_provider(
                    registry,
                    CreateModelProviderInput {
                        display_name: display_name.to_owned(),
                        source: ModelProviderSource::Custom { catalog_provider_id: None },
                    },
                    UtcTimestamp::now(),
                )
            })?;
        let provider_id = prepared.value;
        let prepared_bundle = prepared
            .prepared_bundle
            .ok_or(ModelConfigJournalError::CredentialChildRequiresDedicatedCoordinator)?;
        Ok(ModelConfigCommandPlan::Publish {
            prepared_bundle: Box::new(prepared_bundle),
            expected_models_revision: Revision::new(revision),
            reserved_models_revision: Some(prepared.candidate_revision),
            expected_aura_revision: None,
            reserved_aura_revision: None,
            credential_slot_creations: Vec::new(),
            session_binding_clears: Vec::new(),
            metadata_events: vec![PersistentEventDraft::system(
                "ModelProviderCreated",
                correlation_id,
                json!({"provider_id": provider_id}),
            )],
            projection_drafts: vec![self.model_settings_projection(
                prepared.candidate_revision,
                (*prepared.candidate_registry).clone(),
            )],
            success_result: json!({"changed": true, "provider_id": provider_id}),
        })
    }

    /// One ordinary model-config write: a new Provider through the coordinator.
    pub(super) fn create_provider(
        &self,
        display_name: &str,
    ) -> Result<ModelConfigCommandCompletion, ModelConfigJournalError> {
        let correlation_id = CorrelationId::new();
        self.coordinator.execute(CommandInvocationId::new(), correlation_id, |journal_id| {
            self.provider_plan(journal_id, correlation_id, display_name)
        })
    }

    /// Holds the model-config lease the way a journal that has not finished yet holds it.
    pub(super) fn hold_model_config_lease(&self) -> MutationJournalId {
        let journal_id = MutationJournalId::new();
        self.persistence
            .prepare_mutation_journal(PrepareMutationJournal {
                journal_id,
                journal_kind: LEASE_HOLDER_KIND.to_owned(),
                idempotency_key: None,
                initial_state: "prepared".to_owned(),
                payload: json!({}),
                resource_kind: "model_config".to_owned(),
                resource_id: "global".to_owned(),
            })
            .expect("hold the model-config lease");
        journal_id
    }

    pub(super) fn finish(&self, holder: MutationJournalId) {
        self.persistence
            .transition_mutation_journal(holder, LEASE_HOLDER_KIND, "prepared", "completed", None)
            .expect("finish the lease holder");
    }
}

/// The same Provider write, shaped as a write that also configures one credential.
pub(super) fn credential_plan(
    plan: ModelConfigCommandPlan,
    credential_id: CredentialId,
) -> ModelConfigCredentialCommandPlan {
    let ModelConfigCommandPlan::Publish {
        prepared_bundle,
        expected_models_revision,
        reserved_models_revision,
        metadata_events,
        projection_drafts,
        success_result,
        ..
    } = plan
    else {
        panic!("a Provider write publishes its bundle");
    };
    ModelConfigCredentialCommandPlan {
        prepared_bundle,
        expected_models_revision,
        reserved_models_revision: reserved_models_revision.expect("reserved models revision"),
        credential_slot_creation: unwritten_credential_slot(credential_id),
        metadata_events,
        projection_drafts,
        success_result,
    }
}

/// The same Provider write, shaped as a write that deletes these credentials after publishing.
pub(super) fn credential_deletion_plan(
    plan: ModelConfigCommandPlan,
    credential_ids: Vec<CredentialId>,
) -> ModelConfigCredentialDeleteCommandPlan {
    let ModelConfigCommandPlan::Publish {
        prepared_bundle,
        expected_models_revision,
        reserved_models_revision,
        metadata_events,
        projection_drafts,
        success_result,
        ..
    } = plan
    else {
        panic!("a Provider write publishes its bundle");
    };
    ModelConfigCredentialDeleteCommandPlan {
        prepared_bundle,
        expected_models_revision,
        reserved_models_revision: reserved_models_revision.expect("reserved models revision"),
        expected_aura_revision: None,
        reserved_aura_revision: None,
        credential_ids,
        deletion_reason: ModelCredentialDeletionReason::ProviderDeleted,
        session_binding_clears: Vec::new(),
        metadata_events,
        projection_drafts,
        success_result,
    }
}

/// A well-formed slot for a key nobody writes: these plans never reach the credential store.
pub(super) fn unwritten_credential_slot(credential_id: CredentialId) -> CreateModelCredentialSlot {
    let bundle_namespace_key = "0123456789abcdef".to_owned();
    CreateModelCredentialSlot {
        credential_id,
        endpoint_id: EndpointId::new(),
        binding: ModelCredentialStorageBinding {
            credential_ref: format!(
                "wincred://aura/{bundle_namespace_key}/credential/{credential_id}"
            )
            .parse()
            .expect("credential ref"),
            credential_binding_fingerprint: CredentialBindingFingerprint(ContentHash::sha256(
                b"unwritten credential slot",
            )),
            bundle_namespace_key: bundle_namespace_key.clone(),
            backend_kind: "wincred".to_owned(),
            wincred_type: "generic".to_owned(),
            final_target: format!("Aura/{bundle_namespace_key}/credential/{credential_id}"),
            normalized_origin: NormalizedOrigin {
                scheme: "https".to_owned(),
                host: "api.example.test".to_owned(),
                port: 443,
            },
            auth_scheme: EndpointAuthScheme::OpenAiBearerApiKey,
            credential_slot: ModelCredentialSlot::ApiKey,
        },
    }
}
