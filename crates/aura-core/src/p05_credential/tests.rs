use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use aura_domain_common::{
    CommandInvocationId, ContentHash, CorrelationId, CredentialId, EndpointId, ExpectedRevision,
    ProviderId, Revision, RuntimeInstanceId, UtcTimestamp,
};
use aura_model_domain::{
    CredentialBindingFingerprint, EndpointAuthScheme, EndpointAuthenticationKind,
    EndpointAuthenticationProjection, EndpointCredentialState, EndpointCredentialStatus,
    EndpointDetailProjection, ModelApiFormat, ModelCredentialSlot, ModelDiscoveryMode,
    ModelEndpointConfig, ModelEndpointTimeouts, ModelProviderConfig, ModelProviderSource,
    ModelRegistryConfig, ModelSettingsProjection,
};
use aura_permission_domain::NormalizedOrigin;
use aura_persistence::{
    CreateModelCredentialSlot, CredentialMutationJournalState, ModelCredentialRecordState,
    ModelCredentialStorageBinding, ModelCredentialWriteIntent, ModelProjectionDraft,
    PersistenceCoordinator, PersistenceOptions, PersistenceStartup, PrepareModelCredentialClear,
    PrepareModelCredentialWrite, PrepareModelCredentialWriteResult,
};
use aura_windows_platform::{CredentialApplicationAttribute, CredentialStoreError, SecretBytes};

use super::{
    BackendCredentialRecord, CredentialBackend, CredentialChildPrePublishRecovery,
    CredentialClearCompletion, CredentialJournalCoordinator, CredentialProjectionFactory,
    CredentialRecoveryOutcome, P05CredentialJournalError, WinCredentialRecordState,
    journal_attributes,
};

mod s05_dual_real;

#[derive(Clone, Debug, Eq, PartialEq)]
struct FakeCredentialRecord {
    secret: Vec<u8>,
    attributes: Vec<CredentialApplicationAttribute>,
}

#[derive(Clone, Default)]
struct FakeCredentialBackend {
    records: Arc<Mutex<BTreeMap<String, FakeCredentialRecord>>>,
}

impl FakeCredentialBackend {
    fn contains(&self, reference: &aura_domain_common::CredentialRef) -> bool {
        self.records
            .lock()
            .expect("fake credential backend lock")
            .contains_key(&reference.to_string())
    }

    fn record(
        &self,
        reference: &aura_domain_common::CredentialRef,
    ) -> Option<FakeCredentialRecord> {
        self.records
            .lock()
            .expect("fake credential backend lock")
            .get(&reference.to_string())
            .cloned()
    }
}

impl CredentialBackend for FakeCredentialBackend {
    fn write(
        &self,
        reference: &aura_domain_common::CredentialRef,
        secret: &SecretBytes,
        attributes: &[CredentialApplicationAttribute],
    ) -> Result<(), CredentialStoreError> {
        self.records.lock().expect("fake credential backend lock").insert(
            reference.to_string(),
            FakeCredentialRecord {
                secret: secret.expose().to_vec(),
                attributes: attributes.to_vec(),
            },
        );
        Ok(())
    }

    fn read(
        &self,
        reference: &aura_domain_common::CredentialRef,
    ) -> Result<Option<BackendCredentialRecord>, CredentialStoreError> {
        Ok(self.record(reference).map(|record| BackendCredentialRecord {
            secret: SecretBytes::new(record.secret),
            attributes: record.attributes,
        }))
    }

    fn delete(
        &self,
        reference: &aura_domain_common::CredentialRef,
    ) -> Result<bool, CredentialStoreError> {
        Ok(self
            .records
            .lock()
            .expect("fake credential backend lock")
            .remove(&reference.to_string())
            .is_some())
    }
}

struct TestProjectionFactory {
    endpoint_id: EndpointId,
    models_revision: Revision,
}

impl CredentialProjectionFactory for TestProjectionFactory {
    fn projections(
        &mut self,
        journal: &aura_persistence::CredentialMutationJournalRecord,
        status: EndpointCredentialStatus,
    ) -> Result<Vec<ModelProjectionDraft>, P05CredentialJournalError> {
        assert_eq!(journal.binding.credential_slot, ModelCredentialSlot::ApiKey);
        let provider_id = ProviderId::new();
        let now = UtcTimestamp::now();
        Ok(vec![
            ModelProjectionDraft::ModelSettings(ModelSettingsProjection {
                models_revision: self.models_revision,
                aura_revision: Revision::INITIAL,
                registry: ModelRegistryConfig::default(),
                default_model_profile_id: None,
                active_credentials: vec![status.clone()],
                dormant_credentials: Vec::new(),
                reasoning_efforts: Default::default(),
            }),
            ModelProjectionDraft::EndpointDetail(Box::new(EndpointDetailProjection {
                endpoint_id: self.endpoint_id,
                models_revision: self.models_revision,
                provider: ModelProviderConfig {
                    id: provider_id,
                    display_name: "Provider".to_owned(),
                    slug: "provider".parse().expect("provider slug"),
                    source: ModelProviderSource::Custom { catalog_provider_id: None },
                    endpoint_order: vec![self.endpoint_id],
                    created_at: now,
                    updated_at: now,
                    default_endpoint_id: None,
                    backup_endpoint_ids: vec![],
                },
                endpoint: ModelEndpointConfig {
                    id: self.endpoint_id,
                    revision: Revision::INITIAL,
                    provider_id,
                    display_name: "Endpoint".to_owned(),
                    slug: "endpoint".parse().expect("endpoint slug"),
                    api_format: ModelApiFormat::OpenAiResponses,
                    base_url: "https://api.example.test/v1".to_owned(),
                    discovery_mode: ModelDiscoveryMode::Disabled,
                    chat_output_token_field: None,
                    chat_instruction_role: None,
                    auth_scheme: journal.binding.auth_scheme,
                    openai_organization_id: None,
                    openai_project_id: None,
                    anthropic_workspace_id: None,
                    timeouts: ModelEndpointTimeouts {
                        connect_ms: 15_000,
                        first_event_ms: 120_000,
                        stream_idle_ms: 120_000,
                        total_ms: None,
                    },
                    max_parallel_model_requests: 2,
                    model_order: Vec::new(),
                    compatibility: None,
                    created_at: now,
                    updated_at: now,
                },
                authentication: EndpointAuthenticationProjection {
                    auth_kind: EndpointAuthenticationKind::Secret,
                    active_credential: Some(status),
                    dormant_credential: None,
                },
                models: Vec::new(),
                latest_discovery_run: None,
            })),
        ])
    }
}

struct CredentialFixture {
    _directory: tempfile::TempDir,
    persistence: Arc<PersistenceCoordinator>,
    backend: FakeCredentialBackend,
    endpoint_id: EndpointId,
    credential_id: CredentialId,
    binding: ModelCredentialStorageBinding,
}

impl CredentialFixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("temporary directory");
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
        let endpoint_id = EndpointId::new();
        let credential_id = CredentialId::new();
        let bundle_namespace_key = "0123456789abcdef".to_owned();
        let final_target = format!("Aura/{bundle_namespace_key}/credential/{credential_id}");
        let binding = ModelCredentialStorageBinding {
            credential_ref: format!(
                "wincred://aura/{bundle_namespace_key}/credential/{credential_id}"
            )
            .parse()
            .expect("credential ref"),
            credential_binding_fingerprint: CredentialBindingFingerprint(ContentHash::sha256(
                b"credential-journal-test-binding",
            )),
            bundle_namespace_key,
            backend_kind: "wincred".to_owned(),
            wincred_type: "generic".to_owned(),
            final_target,
            normalized_origin: NormalizedOrigin {
                scheme: "https".to_owned(),
                host: "api.example.test".to_owned(),
                port: 443,
            },
            auth_scheme: EndpointAuthScheme::OpenAiBearerApiKey,
            credential_slot: ModelCredentialSlot::ApiKey,
        };
        persistence
            .create_model_credential_slot(CreateModelCredentialSlot {
                credential_id,
                endpoint_id,
                binding: binding.clone(),
            })
            .expect("create stable credential slot");
        Self {
            _directory: directory,
            persistence,
            backend: FakeCredentialBackend::default(),
            endpoint_id,
            credential_id,
            binding,
        }
    }

    fn coordinator(&self) -> CredentialJournalCoordinator<FakeCredentialBackend> {
        CredentialJournalCoordinator {
            persistence: Arc::clone(&self.persistence),
            backend: self.backend.clone(),
        }
    }

    fn projections(&self) -> TestProjectionFactory {
        TestProjectionFactory { endpoint_id: self.endpoint_id, models_revision: Revision::new(7) }
    }

    fn write_request(&self, expected_row_revision: u64) -> PrepareModelCredentialWrite {
        PrepareModelCredentialWrite {
            command_invocation_id: CommandInvocationId::new(),
            endpoint_id: self.endpoint_id,
            credential_id: self.credential_id,
            credential_slot: ModelCredentialSlot::ApiKey,
            expected_row_revision: ExpectedRevision::new(expected_row_revision),
            intent: ModelCredentialWriteIntent::Configure,
            binding: self.binding.clone(),
            models_revision: Revision::new(7),
            correlation_id: CorrelationId::new(),
        }
    }
}

#[test]
fn configure_and_clear_keep_the_slot_identity_and_invalidate_consumption() {
    let fixture = CredentialFixture::new();
    let coordinator = fixture.coordinator();
    let mut projections = fixture.projections();
    let secret = SecretBytes::new(b"test-only-model-api-key".to_vec());

    let configured = coordinator
        .configure_secret(fixture.write_request(1), &secret, &mut projections)
        .expect("configure credential");
    assert!(configured.cleanup_completed);
    assert_eq!(
        configured.metadata_commit.credential_record.state,
        ModelCredentialRecordState::Configured
    );
    assert_eq!(
        configured
            .metadata_commit
            .credential_record
            .renderer_status()
            .expect("configured Renderer status")
            .active_credential_revision,
        configured.metadata_commit.credential_record.last_committed_credential_revision
    );
    assert!(fixture.backend.contains(&fixture.binding.credential_ref));
    assert!(
        !fixture.backend.contains(
            &configured
                .metadata_commit
                .journal
                .staging_credential_ref()
                .expect("staging credential ref")
        )
    );

    let credential_revision = configured
        .metadata_commit
        .credential_record
        .last_committed_credential_revision
        .expect("committed credential revision");
    fixture
        .persistence
        .credential_for_model_transport(
            fixture.credential_id,
            credential_revision,
            &fixture.binding.credential_binding_fingerprint,
        )
        .expect("configured credential resolves for transport");

    let cleared = coordinator
        .clear(
            PrepareModelCredentialClear {
                command_invocation_id: CommandInvocationId::new(),
                endpoint_id: fixture.endpoint_id,
                credential_id: fixture.credential_id,
                credential_slot: ModelCredentialSlot::ApiKey,
                expected_row_revision: ExpectedRevision::new(2),
                models_revision: Revision::new(7),
                correlation_id: CorrelationId::new(),
            },
            &mut projections,
        )
        .expect("clear credential");
    let CredentialClearCompletion::Committed(cleared) = cleared else {
        panic!("configured credential must create a dedicated clear Journal");
    };
    let status = cleared
        .metadata_commit
        .credential_record
        .renderer_status()
        .expect("cleared Renderer status");
    assert_eq!(status.credential_id, fixture.credential_id);
    assert_eq!(status.state, EndpointCredentialState::Cleared);
    assert_eq!(status.active_credential_revision, None);
    assert_eq!(status.last_committed_credential_revision, Some(credential_revision));
    assert!(!fixture.backend.contains(&fixture.binding.credential_ref));
    assert!(
        fixture
            .persistence
            .credential_for_model_transport(
                fixture.credential_id,
                credential_revision,
                &fixture.binding.credential_binding_fingerprint,
            )
            .is_err(),
        "clear must immediately invalidate an existing model channel authorization"
    );
}

#[test]
fn recovery_forwards_a_staged_write_and_reuses_the_reserved_revision() {
    let fixture = CredentialFixture::new();
    let coordinator = fixture.coordinator();
    let mut projections = fixture.projections();
    let journal = fixture
        .persistence
        .prepare_model_credential_write(fixture.write_request(1))
        .expect("prepare configure Journal");
    let PrepareModelCredentialWriteResult::Prepared(journal) = journal else {
        panic!("new configure must prepare a Journal");
    };
    let staging_ref = journal.staging_credential_ref().expect("staging ref");
    let staged_attributes =
        journal_attributes(&journal, WinCredentialRecordState::Staged).expect("attributes");
    let secret = SecretBytes::new(b"recoverable-test-secret".to_vec());
    fixture.backend.write(&staging_ref, &secret, &staged_attributes).expect("write staging");
    fixture
        .persistence
        .transition_credential_mutation_journal(
            journal.journal_id,
            CredentialMutationJournalState::Prepared,
            CredentialMutationJournalState::Staged,
        )
        .expect("persist staged state");

    let outcomes = coordinator.recover(&mut projections).expect("recover J-A");
    let [CredentialRecoveryOutcome::Committed(completion)] = outcomes.as_slice() else {
        panic!("staged configure must forward-complete exactly one Journal");
    };
    assert!(completion.cleanup_completed);
    assert_eq!(
        completion.metadata_commit.credential_record.last_committed_credential_revision,
        journal.reserved_credential_revision
    );
    assert_eq!(
        fixture.backend.record(&fixture.binding.credential_ref).expect("final credential").secret,
        b"recoverable-test-secret"
    );
    assert!(!fixture.backend.contains(&staging_ref));
}

#[test]
fn pre_publish_child_recovery_adopts_verified_staging_without_secret_replay() {
    let fixture = CredentialFixture::new();
    let coordinator = fixture.coordinator();
    let journal = fixture
        .persistence
        .prepare_model_credential_write(fixture.write_request(1))
        .expect("prepare configure Journal");
    let PrepareModelCredentialWriteResult::Prepared(journal) = journal else {
        panic!("new configure must prepare a Journal");
    };
    let staging_ref = journal.staging_credential_ref().expect("staging ref");
    let staged_attributes =
        journal_attributes(&journal, WinCredentialRecordState::Staged).expect("attributes");
    fixture
        .backend
        .write(
            &staging_ref,
            &SecretBytes::new(b"staged-before-parent-publish".to_vec()),
            &staged_attributes,
        )
        .expect("write staging before simulated crash");

    let recovered = coordinator
        .recover_child_before_config_publish(*journal.clone())
        .expect("adopt verified staging");
    let CredentialChildPrePublishRecovery::Staged(recovered) = recovered else {
        panic!("verified staging must be adopted");
    };
    assert_eq!(recovered.state, CredentialMutationJournalState::Staged);
    assert_eq!(recovered.reserved_credential_revision, journal.reserved_credential_revision);
    assert!(fixture.backend.contains(&staging_ref));
    assert!(!fixture.backend.contains(&fixture.binding.credential_ref));
}

#[test]
fn pre_publish_child_recovery_aborts_when_secret_never_reached_staging() {
    let fixture = CredentialFixture::new();
    let coordinator = fixture.coordinator();
    let journal = fixture
        .persistence
        .prepare_model_credential_write(fixture.write_request(1))
        .expect("prepare configure Journal");
    let PrepareModelCredentialWriteResult::Prepared(journal) = journal else {
        panic!("new configure must prepare a Journal");
    };

    let recovered = coordinator
        .recover_child_before_config_publish(*journal.clone())
        .expect("abort unrecoverable prepared child");
    assert_eq!(recovered, CredentialChildPrePublishRecovery::Aborted);
    assert_eq!(
        fixture
            .persistence
            .credential_mutation_journal(journal.journal_id)
            .expect("read child Journal")
            .expect("child Journal")
            .state,
        CredentialMutationJournalState::Aborted
    );
}

#[test]
fn contradictory_staging_attributes_require_reconciliation_without_cleanup() {
    let fixture = CredentialFixture::new();
    let coordinator = fixture.coordinator();
    let mut projections = fixture.projections();
    let journal = fixture
        .persistence
        .prepare_model_credential_write(fixture.write_request(1))
        .expect("prepare configure Journal");
    let PrepareModelCredentialWriteResult::Prepared(journal) = journal else {
        panic!("new configure must prepare a Journal");
    };
    let staging_ref = journal.staging_credential_ref().expect("staging ref");
    let mut contradictory_attributes =
        journal_attributes(&journal, WinCredentialRecordState::Staged).expect("attributes");
    contradictory_attributes.push(
        CredentialApplicationAttribute::new("Aura_Unexpected", "contradiction")
            .expect("valid test attribute"),
    );
    fixture
        .backend
        .write(
            &staging_ref,
            &SecretBytes::new(b"unrecoverable-test-secret".to_vec()),
            &contradictory_attributes,
        )
        .expect("write contradictory staging record");

    let outcomes = coordinator.recover(&mut projections).expect("recover J-A");
    assert!(matches!(
        outcomes.as_slice(),
        [CredentialRecoveryOutcome::Reconciled { journal_id }]
            if *journal_id == journal.journal_id
    ));
    assert_eq!(
        fixture
            .persistence
            .credential_mutation_journal(journal.journal_id)
            .expect("read Journal")
            .expect("Journal")
            .state,
        CredentialMutationJournalState::ReconciliationRequired
    );
    assert_eq!(
        fixture
            .persistence
            .model_credential_record(fixture.credential_id)
            .expect("read record")
            .expect("credential record")
            .state,
        ModelCredentialRecordState::ReconciliationRequired
    );
    assert!(
        fixture.backend.contains(&staging_ref),
        "reconciliation must preserve contradictory WinCred evidence"
    );
    assert!(
        !fixture.backend.contains(&fixture.binding.credential_ref),
        "reconciliation must not invent a final credential"
    );
}
