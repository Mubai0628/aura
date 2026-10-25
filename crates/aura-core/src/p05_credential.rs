use std::sync::Arc;

use aura_domain_common::{CredentialRef, MutationJournalId};
use aura_model_domain::{EndpointAuthScheme, EndpointCredentialState, EndpointCredentialStatus};
use aura_network_control::{
    ConfigurationRequestCredentialBindingV1, NetworkControlError, ProviderCredentialBinding,
    ProviderCredentialResolver, ResolvedProviderCredential,
};
use aura_persistence::{
    CredentialMutationJournalKind, CredentialMutationJournalRecord, CredentialMutationJournalState,
    ModelCredentialMetadataCommit, ModelCredentialRecord, ModelCredentialRecordState,
    ModelProjectionDraft, PersistenceCoordinator, PersistenceError, PrepareModelCredentialClear,
    PrepareModelCredentialClearResult, PrepareModelCredentialWrite,
    PrepareModelCredentialWriteResult,
};
use aura_windows_platform::{
    CredentialApplicationAttribute, CredentialStoreError, SecretBytes, StoredCredential,
    WindowsCredentialStore,
};
use thiserror::Error;

const ATTRIBUTE_CREDENTIAL_ID: &str = "Aura_CredentialId";
const ATTRIBUTE_CREDENTIAL_REVISION: &str = "Aura_CredentialRevision";
const ATTRIBUTE_TRANSACTION_ID: &str = "Aura_TransactionId";
const ATTRIBUTE_RECORD_STATE: &str = "Aura_RecordState";
const ATTRIBUTE_BUNDLE_NAMESPACE_HASH: &str = "Aura_BundleNamespaceHash";

#[derive(Debug, Error)]
pub enum P05CredentialJournalError {
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(transparent)]
    CredentialStore(#[from] CredentialStoreError),
    #[error("credential projection construction failed: {0}")]
    Projection(String),
    #[error("credential staging or final read-back did not match the frozen J-A attributes")]
    VerificationMismatch,
    #[error("credential Journal entered reconciliation_required: {journal_id}")]
    ReconciliationRequired { journal_id: MutationJournalId },
    #[error("credential Journal was aborted: {journal_id}")]
    Aborted { journal_id: MutationJournalId },
}

#[derive(Clone, Debug, PartialEq)]
pub struct CredentialJournalCompletion {
    pub metadata_commit: ModelCredentialMetadataCommit,
    pub cleanup_completed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CredentialClearCompletion {
    NoChange(EndpointCredentialStatus),
    Committed(Box<CredentialJournalCompletion>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum CredentialRecoveryOutcome {
    Aborted { journal_id: MutationJournalId },
    Committed(Box<CredentialJournalCompletion>),
    Reconciled { journal_id: MutationJournalId },
    CleanupCompleted { journal_id: MutationJournalId },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CredentialChildPrePublishRecovery {
    Staged(Box<CredentialMutationJournalRecord>),
    Aborted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CredentialDeleteChildOutcome {
    Completed,
    CleanupPending,
    ReconciliationRequired,
}

pub trait CredentialProjectionFactory {
    fn projections(
        &mut self,
        journal: &CredentialMutationJournalRecord,
        status: EndpointCredentialStatus,
    ) -> Result<Vec<ModelProjectionDraft>, P05CredentialJournalError>;
}

impl<F> CredentialProjectionFactory for F
where
    F: FnMut(
        &CredentialMutationJournalRecord,
        EndpointCredentialStatus,
    ) -> Result<Vec<ModelProjectionDraft>, P05CredentialJournalError>,
{
    fn projections(
        &mut self,
        journal: &CredentialMutationJournalRecord,
        status: EndpointCredentialStatus,
    ) -> Result<Vec<ModelProjectionDraft>, P05CredentialJournalError> {
        self(journal, status)
    }
}

#[derive(Clone)]
pub struct P05CredentialJournal {
    coordinator: CredentialJournalCoordinator<WindowsCredentialBackend>,
}

/// Resolves a frozen provider-transport credential binding only after the
/// Gateway has consumed its one-shot grant.
///
/// The SQLite registry remains the authority for state, revision and binding
/// fingerprint. WinCred is read only after those checks pass, and the final
/// record's non-secret J-A attributes are checked again before any header value
/// is returned to the Gateway.
pub struct PersistentProviderCredentialResolver {
    persistence: Arc<PersistenceCoordinator>,
}

impl PersistentProviderCredentialResolver {
    #[must_use]
    pub fn new(persistence: Arc<PersistenceCoordinator>) -> Self {
        Self { persistence }
    }
}

/// Checks only the non-secret J-A record identity needed to derive Renderer
/// availability. The Secret bytes are consumed by the platform record and are
/// never copied or returned from this function.
pub(crate) fn credential_record_is_available(record: &ModelCredentialRecord) -> bool {
    if record.state != ModelCredentialRecordState::Configured {
        return false;
    }
    let Some(credential_revision) = record.last_committed_credential_revision else {
        return false;
    };
    let Ok(stored) = crate::credential_io::read(&record.binding.credential_ref) else {
        return false;
    };
    let attributes = stored
        .attributes()
        .iter()
        .map(|attribute| (attribute.keyword(), attribute.value()))
        .collect::<std::collections::BTreeMap<_, _>>();
    attributes.len() == 5
        && attributes.get(ATTRIBUTE_CREDENTIAL_ID).copied()
            == Some(record.credential_id.to_string().as_str())
        && attributes.get(ATTRIBUTE_CREDENTIAL_REVISION).copied()
            == Some(credential_revision.to_string().as_str())
        && attributes.get(ATTRIBUTE_RECORD_STATE).copied() == Some("committed")
        && attributes.get(ATTRIBUTE_BUNDLE_NAMESPACE_HASH).copied()
            == Some(record.binding.bundle_namespace_key.as_str())
        && attributes
            .get(ATTRIBUTE_TRANSACTION_ID)
            .is_some_and(|value| value.parse::<MutationJournalId>().is_ok())
}

impl ProviderCredentialResolver for PersistentProviderCredentialResolver {
    fn resolve(
        &self,
        requested: &ConfigurationRequestCredentialBindingV1,
    ) -> Result<ResolvedProviderCredential, NetworkControlError> {
        let resolution = self
            .persistence
            .credential_for_model_transport(
                requested.credential_id,
                requested.credential_revision,
                &requested.credential_binding_fingerprint,
            )
            .map_err(|error| match error {
                PersistenceError::CredentialBindingMismatch => {
                    NetworkControlError::CredentialBindingMismatch
                }
                _ => NetworkControlError::CredentialUnavailable,
            })?;
        let stored = crate::credential_io::read(&resolution.credential_ref)
            .map_err(|_| NetworkControlError::CredentialUnavailable)?;
        validate_committed_transport_attributes(
            &stored,
            requested,
            resolution.credential_ref.windows_target_name(),
        )?;
        let secret = stored.into_secret();
        let (header_name, secret_header_value) = match resolution.auth_scheme {
            EndpointAuthScheme::OpenAiBearerApiKey => {
                let mut value = Vec::with_capacity(7_usize.saturating_add(secret.expose().len()));
                value.extend_from_slice(b"Bearer ");
                value.extend_from_slice(secret.expose());
                ("authorization", value)
            }
            EndpointAuthScheme::AnthropicApiKey => ("x-api-key", secret.expose().to_vec()),
            EndpointAuthScheme::None => {
                return Err(NetworkControlError::CredentialBindingMismatch);
            }
        };
        Ok(ResolvedProviderCredential {
            binding: ProviderCredentialBinding {
                credential_id: resolution.credential_id,
                credential_revision: resolution.credential_revision,
                credential_binding_fingerprint: requested.credential_binding_fingerprint.clone(),
                origin: resolution.normalized_origin,
                header_name: header_name.to_owned(),
            },
            secret_header_value,
        })
    }
}

fn validate_committed_transport_attributes(
    stored: &StoredCredential,
    requested: &ConfigurationRequestCredentialBindingV1,
    final_target: String,
) -> Result<(), NetworkControlError> {
    let expected_namespace = final_target
        .strip_prefix("Aura/")
        .and_then(|target| target.strip_suffix(&format!("/credential/{}", requested.credential_id)))
        .filter(|namespace| !namespace.is_empty())
        .ok_or(NetworkControlError::CredentialBindingMismatch)?;
    let attributes = stored
        .attributes()
        .iter()
        .map(|attribute| (attribute.keyword(), attribute.value()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let transaction_id = attributes
        .get(ATTRIBUTE_TRANSACTION_ID)
        .ok_or(NetworkControlError::CredentialBindingMismatch)?;
    if attributes.len() != 5
        || attributes.get(ATTRIBUTE_CREDENTIAL_ID).copied()
            != Some(requested.credential_id.to_string().as_str())
        || attributes.get(ATTRIBUTE_CREDENTIAL_REVISION).copied()
            != Some(requested.credential_revision.to_string().as_str())
        || attributes.get(ATTRIBUTE_RECORD_STATE).copied() != Some("committed")
        || attributes.get(ATTRIBUTE_BUNDLE_NAMESPACE_HASH).copied() != Some(expected_namespace)
        || transaction_id.parse::<MutationJournalId>().is_err()
    {
        return Err(NetworkControlError::CredentialBindingMismatch);
    }
    Ok(())
}

impl P05CredentialJournal {
    #[must_use]
    pub fn new(persistence: Arc<PersistenceCoordinator>) -> Self {
        Self {
            coordinator: CredentialJournalCoordinator {
                persistence,
                backend: WindowsCredentialBackend,
            },
        }
    }

    pub fn configure_secret<F>(
        &self,
        input: PrepareModelCredentialWrite,
        secret: &SecretBytes,
        projection_factory: &mut F,
    ) -> Result<CredentialJournalCompletion, P05CredentialJournalError>
    where
        F: CredentialProjectionFactory,
    {
        self.coordinator.configure_secret(input, secret, projection_factory)
    }

    pub fn clear<F>(
        &self,
        input: PrepareModelCredentialClear,
        projection_factory: &mut F,
    ) -> Result<CredentialClearCompletion, P05CredentialJournalError>
    where
        F: CredentialProjectionFactory,
    {
        self.coordinator.clear(input, projection_factory)
    }

    pub fn recover<F>(
        &self,
        projection_factory: &mut F,
    ) -> Result<Vec<CredentialRecoveryOutcome>, P05CredentialJournalError>
    where
        F: CredentialProjectionFactory,
    {
        self.coordinator.recover(projection_factory)
    }

    pub(crate) fn stage_prepared_child(
        &self,
        journal: CredentialMutationJournalRecord,
        secret: &SecretBytes,
    ) -> Result<CredentialMutationJournalRecord, P05CredentialJournalError> {
        self.coordinator.stage_prepared_child(journal, secret)
    }

    pub(crate) fn recover_child_before_config_publish(
        &self,
        journal: CredentialMutationJournalRecord,
    ) -> Result<CredentialChildPrePublishRecovery, P05CredentialJournalError> {
        self.coordinator.recover_child_before_config_publish(journal)
    }

    pub(crate) fn write_staged_child_to_final(
        &self,
        journal: CredentialMutationJournalRecord,
    ) -> Result<CredentialMutationJournalRecord, P05CredentialJournalError> {
        self.coordinator.write_staged_child_to_final(journal)
    }

    pub(crate) fn complete_child_after_external_metadata_commit(
        &self,
        journal: CredentialMutationJournalRecord,
    ) -> Result<bool, P05CredentialJournalError> {
        self.coordinator.complete_child_after_external_metadata_commit(journal)
    }

    pub(crate) fn abort_child_before_final_commit(
        &self,
        journal: CredentialMutationJournalRecord,
    ) -> Result<(), P05CredentialJournalError> {
        self.coordinator.abort_child_before_final_commit(journal)
    }

    pub(crate) fn delete_published_child(
        &self,
        journal: CredentialMutationJournalRecord,
    ) -> Result<CredentialDeleteChildOutcome, P05CredentialJournalError> {
        self.coordinator.delete_published_child(journal)
    }
}

struct BackendCredentialRecord {
    secret: SecretBytes,
    attributes: Vec<CredentialApplicationAttribute>,
}

impl BackendCredentialRecord {
    fn from_stored(record: StoredCredential) -> Self {
        let attributes = record.attributes().to_vec();
        Self { secret: record.into_secret(), attributes }
    }
}

trait CredentialBackend {
    fn write(
        &self,
        reference: &CredentialRef,
        secret: &SecretBytes,
        attributes: &[CredentialApplicationAttribute],
    ) -> Result<(), CredentialStoreError>;

    fn read(
        &self,
        reference: &CredentialRef,
    ) -> Result<Option<BackendCredentialRecord>, CredentialStoreError>;

    fn delete(&self, reference: &CredentialRef) -> Result<bool, CredentialStoreError>;
}

#[derive(Clone, Copy)]
struct WindowsCredentialBackend;

impl CredentialBackend for WindowsCredentialBackend {
    fn write(
        &self,
        reference: &CredentialRef,
        secret: &SecretBytes,
        attributes: &[CredentialApplicationAttribute],
    ) -> Result<(), CredentialStoreError> {
        WindowsCredentialStore.write_with_attributes(reference, secret, attributes)
    }

    fn read(
        &self,
        reference: &CredentialRef,
    ) -> Result<Option<BackendCredentialRecord>, CredentialStoreError> {
        match crate::credential_io::read(reference) {
            Ok(record) => Ok(Some(BackendCredentialRecord::from_stored(record))),
            Err(CredentialStoreError::NotFound { .. }) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn delete(&self, reference: &CredentialRef) -> Result<bool, CredentialStoreError> {
        WindowsCredentialStore.delete(reference)
    }
}

#[derive(Clone)]
struct CredentialJournalCoordinator<B> {
    persistence: Arc<PersistenceCoordinator>,
    backend: B,
}

impl<B> CredentialJournalCoordinator<B>
where
    B: CredentialBackend,
{
    fn recover_child_before_config_publish(
        &self,
        journal: CredentialMutationJournalRecord,
    ) -> Result<CredentialChildPrePublishRecovery, P05CredentialJournalError> {
        if journal.journal_kind != CredentialMutationJournalKind::Configure
            || !matches!(
                journal.state,
                CredentialMutationJournalState::Prepared | CredentialMutationJournalState::Staged
            )
        {
            return Err(PersistenceError::CredentialJournalStateConflict.into());
        }

        let staging_ref = journal.staging_credential_ref()?;
        let staged = self.backend.read(&staging_ref)?;
        let final_record = self.backend.read(&journal.binding.credential_ref)?;
        let staged_attributes = journal_attributes(&journal, WinCredentialRecordState::Staged)?;
        let staged_current =
            staged.as_ref().is_some_and(|record| record_matches(record, &staged_attributes, None));

        // Before models.json is published there must never be a committed final
        // credential. Seeing one means the external stores and the Parent state
        // disagree, so preserve both records for explicit reconciliation.
        if final_record.is_some() || staged.is_some_and(|_| !staged_current) {
            self.persistence.transition_credential_mutation_journal(
                journal.journal_id,
                journal.state,
                CredentialMutationJournalState::ReconciliationRequired,
            )?;
            return Err(P05CredentialJournalError::ReconciliationRequired {
                journal_id: journal.journal_id,
            });
        }

        if staged_current {
            let journal = if journal.state == CredentialMutationJournalState::Prepared {
                self.persistence.transition_credential_mutation_journal(
                    journal.journal_id,
                    CredentialMutationJournalState::Prepared,
                    CredentialMutationJournalState::Staged,
                )?
            } else {
                journal
            };
            return Ok(CredentialChildPrePublishRecovery::Staged(Box::new(journal)));
        }

        if journal.state == CredentialMutationJournalState::Staged {
            self.persistence.transition_credential_mutation_journal(
                journal.journal_id,
                CredentialMutationJournalState::Staged,
                CredentialMutationJournalState::ReconciliationRequired,
            )?;
            return Err(P05CredentialJournalError::ReconciliationRequired {
                journal_id: journal.journal_id,
            });
        }

        self.persistence.transition_credential_mutation_journal(
            journal.journal_id,
            CredentialMutationJournalState::Prepared,
            CredentialMutationJournalState::Aborted,
        )?;
        Ok(CredentialChildPrePublishRecovery::Aborted)
    }

    fn stage_prepared_child(
        &self,
        journal: CredentialMutationJournalRecord,
        secret: &SecretBytes,
    ) -> Result<CredentialMutationJournalRecord, P05CredentialJournalError> {
        if journal.journal_kind != CredentialMutationJournalKind::Configure
            || journal.state != CredentialMutationJournalState::Prepared
        {
            return Err(PersistenceError::CredentialJournalStateConflict.into());
        }
        let staging_ref = journal.staging_credential_ref()?;
        let staged_attributes = journal_attributes(&journal, WinCredentialRecordState::Staged)?;
        self.backend.write(&staging_ref, secret, &staged_attributes)?;
        let staged = self
            .backend
            .read(&staging_ref)?
            .ok_or(P05CredentialJournalError::VerificationMismatch)?;
        if !record_matches(&staged, &staged_attributes, Some(secret)) {
            return Err(P05CredentialJournalError::VerificationMismatch);
        }
        self.persistence
            .transition_credential_mutation_journal(
                journal.journal_id,
                CredentialMutationJournalState::Prepared,
                CredentialMutationJournalState::Staged,
            )
            .map_err(P05CredentialJournalError::from)
    }

    fn write_staged_child_to_final(
        &self,
        journal: CredentialMutationJournalRecord,
    ) -> Result<CredentialMutationJournalRecord, P05CredentialJournalError> {
        if journal.journal_kind != CredentialMutationJournalKind::Configure
            || journal.state != CredentialMutationJournalState::Staged
        {
            return Err(PersistenceError::CredentialJournalStateConflict.into());
        }
        let staging_ref = journal.staging_credential_ref()?;
        let staged_attributes = journal_attributes(&journal, WinCredentialRecordState::Staged)?;
        let staged = self
            .backend
            .read(&staging_ref)?
            .filter(|record| record_matches(record, &staged_attributes, None))
            .ok_or(P05CredentialJournalError::VerificationMismatch)?;
        let committed_attributes =
            journal_attributes(&journal, WinCredentialRecordState::Committed)?;
        self.backend.write(
            &journal.binding.credential_ref,
            &staged.secret,
            &committed_attributes,
        )?;
        let final_record = self
            .backend
            .read(&journal.binding.credential_ref)?
            .ok_or(P05CredentialJournalError::VerificationMismatch)?;
        if !record_matches(&final_record, &committed_attributes, Some(&staged.secret)) {
            return Err(P05CredentialJournalError::VerificationMismatch);
        }
        self.persistence
            .transition_credential_mutation_journal(
                journal.journal_id,
                CredentialMutationJournalState::Staged,
                CredentialMutationJournalState::FinalCredentialWritten,
            )
            .map_err(P05CredentialJournalError::from)
    }

    fn complete_child_after_external_metadata_commit(
        &self,
        mut journal: CredentialMutationJournalRecord,
    ) -> Result<bool, P05CredentialJournalError> {
        if journal.state == CredentialMutationJournalState::Completed {
            return Ok(true);
        }
        if journal.state == CredentialMutationJournalState::MetadataCommitted {
            journal = self.persistence.transition_credential_mutation_journal(
                journal.journal_id,
                CredentialMutationJournalState::MetadataCommitted,
                CredentialMutationJournalState::CleanupPending,
            )?;
        }
        if journal.state != CredentialMutationJournalState::CleanupPending {
            return Err(PersistenceError::CredentialJournalStateConflict.into());
        }
        match self.backend.delete(&journal.staging_credential_ref()?) {
            Ok(_) => {
                self.persistence.transition_credential_mutation_journal(
                    journal.journal_id,
                    CredentialMutationJournalState::CleanupPending,
                    CredentialMutationJournalState::Completed,
                )?;
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }

    fn abort_child_before_final_commit(
        &self,
        journal: CredentialMutationJournalRecord,
    ) -> Result<(), P05CredentialJournalError> {
        if journal.journal_kind != CredentialMutationJournalKind::Configure
            || !matches!(
                journal.state,
                CredentialMutationJournalState::Prepared | CredentialMutationJournalState::Staged
            )
        {
            return Err(PersistenceError::CredentialJournalStateConflict.into());
        }
        let committed_attributes =
            journal_attributes(&journal, WinCredentialRecordState::Committed)?;
        if self
            .backend
            .read(&journal.binding.credential_ref)?
            .is_some_and(|record| record_matches(&record, &committed_attributes, None))
        {
            return Err(P05CredentialJournalError::ReconciliationRequired {
                journal_id: journal.journal_id,
            });
        }
        self.backend.delete(&journal.staging_credential_ref()?)?;
        self.persistence.transition_credential_mutation_journal(
            journal.journal_id,
            journal.state,
            CredentialMutationJournalState::Aborted,
        )?;
        Ok(())
    }

    fn delete_published_child(
        &self,
        mut journal: CredentialMutationJournalRecord,
    ) -> Result<CredentialDeleteChildOutcome, P05CredentialJournalError> {
        if journal.journal_kind != CredentialMutationJournalKind::Delete {
            return Err(PersistenceError::CredentialJournalStateConflict.into());
        }
        loop {
            match journal.state {
                CredentialMutationJournalState::Staged => {
                    let record = self
                        .persistence
                        .model_credential_record(journal.credential_id)?
                        .ok_or(PersistenceError::CredentialRecordNotFound)?;
                    if record.state != ModelCredentialRecordState::DeletionPending
                        || record.row_revision != journal.target_row_revision
                        || record.binding != journal.binding
                    {
                        self.persistence.transition_credential_mutation_journal(
                            journal.journal_id,
                            journal.state,
                            CredentialMutationJournalState::ReconciliationRequired,
                        )?;
                        return Ok(CredentialDeleteChildOutcome::ReconciliationRequired);
                    }
                    if let Some(final_record) =
                        self.backend.read(&journal.binding.credential_ref)?
                        && !delete_record_matches(&record, &final_record)
                    {
                        self.persistence.transition_credential_mutation_journal(
                            journal.journal_id,
                            journal.state,
                            CredentialMutationJournalState::ReconciliationRequired,
                        )?;
                        return Ok(CredentialDeleteChildOutcome::ReconciliationRequired);
                    }
                    if self.backend.delete(&journal.binding.credential_ref).is_err()
                        || self.backend.read(&journal.binding.credential_ref)?.is_some()
                    {
                        return Ok(CredentialDeleteChildOutcome::CleanupPending);
                    }
                    journal = self.persistence.transition_credential_mutation_journal(
                        journal.journal_id,
                        CredentialMutationJournalState::Staged,
                        CredentialMutationJournalState::FinalCredentialWritten,
                    )?;
                }
                CredentialMutationJournalState::FinalCredentialWritten => {
                    if self.backend.read(&journal.binding.credential_ref)?.is_some() {
                        self.persistence.transition_credential_mutation_journal(
                            journal.journal_id,
                            journal.state,
                            CredentialMutationJournalState::ReconciliationRequired,
                        )?;
                        return Ok(CredentialDeleteChildOutcome::ReconciliationRequired);
                    }
                    journal = self
                        .persistence
                        .commit_model_credential_delete_metadata(journal.journal_id)?
                        .journal;
                }
                CredentialMutationJournalState::MetadataCommitted => {
                    journal = self.persistence.transition_credential_mutation_journal(
                        journal.journal_id,
                        CredentialMutationJournalState::MetadataCommitted,
                        CredentialMutationJournalState::CleanupPending,
                    )?;
                }
                CredentialMutationJournalState::CleanupPending => {
                    self.persistence.transition_credential_mutation_journal(
                        journal.journal_id,
                        CredentialMutationJournalState::CleanupPending,
                        CredentialMutationJournalState::Completed,
                    )?;
                    return Ok(CredentialDeleteChildOutcome::Completed);
                }
                CredentialMutationJournalState::Completed => {
                    return Ok(CredentialDeleteChildOutcome::Completed);
                }
                CredentialMutationJournalState::ReconciliationRequired => {
                    return Ok(CredentialDeleteChildOutcome::ReconciliationRequired);
                }
                CredentialMutationJournalState::Prepared
                | CredentialMutationJournalState::Aborted => {
                    return Err(PersistenceError::CredentialJournalStateConflict.into());
                }
            }
        }
    }

    fn configure_secret<F>(
        &self,
        input: PrepareModelCredentialWrite,
        secret: &SecretBytes,
        projection_factory: &mut F,
    ) -> Result<CredentialJournalCompletion, P05CredentialJournalError>
    where
        F: CredentialProjectionFactory,
    {
        let mut journal = match self.persistence.prepare_model_credential_write(input)? {
            PrepareModelCredentialWriteResult::Prepared(journal) => *journal,
            PrepareModelCredentialWriteResult::Existing(journal) => {
                return self.resume_existing_write(*journal, projection_factory);
            }
        };
        let staging_ref = journal.staging_credential_ref()?;
        let staged_attributes = journal_attributes(&journal, WinCredentialRecordState::Staged)?;
        self.backend.write(&staging_ref, secret, &staged_attributes)?;
        let staged = self
            .backend
            .read(&staging_ref)?
            .ok_or(P05CredentialJournalError::VerificationMismatch)?;
        if !record_matches(&staged, &staged_attributes, Some(secret)) {
            return self.reconcile_error(&journal, projection_factory);
        }
        journal = self.persistence.transition_credential_mutation_journal(
            journal.journal_id,
            CredentialMutationJournalState::Prepared,
            CredentialMutationJournalState::Staged,
        )?;

        let committed_attributes =
            journal_attributes(&journal, WinCredentialRecordState::Committed)?;
        self.backend.write(&journal.binding.credential_ref, secret, &committed_attributes)?;
        let final_record = self
            .backend
            .read(&journal.binding.credential_ref)?
            .ok_or(P05CredentialJournalError::VerificationMismatch)?;
        if !record_matches(&final_record, &committed_attributes, Some(secret)) {
            return self.reconcile_error(&journal, projection_factory);
        }
        journal = self.persistence.transition_credential_mutation_journal(
            journal.journal_id,
            CredentialMutationJournalState::Staged,
            CredentialMutationJournalState::FinalCredentialWritten,
        )?;
        self.commit_and_cleanup(journal, projection_factory)
    }

    fn clear<F>(
        &self,
        input: PrepareModelCredentialClear,
        projection_factory: &mut F,
    ) -> Result<CredentialClearCompletion, P05CredentialJournalError>
    where
        F: CredentialProjectionFactory,
    {
        let mut journal = match self.persistence.prepare_model_credential_clear(input)? {
            PrepareModelCredentialClearResult::NoChange(status) => {
                return Ok(CredentialClearCompletion::NoChange(status));
            }
            PrepareModelCredentialClearResult::Prepared(journal) => *journal,
            PrepareModelCredentialClearResult::Existing(journal) => {
                return self
                    .resume_existing_write(*journal, projection_factory)
                    .map(Box::new)
                    .map(CredentialClearCompletion::Committed);
            }
        };
        journal = self.persistence.transition_credential_mutation_journal(
            journal.journal_id,
            CredentialMutationJournalState::Prepared,
            CredentialMutationJournalState::Staged,
        )?;
        self.backend.delete(&journal.binding.credential_ref)?;
        if self.backend.read(&journal.binding.credential_ref)?.is_some() {
            return self.reconcile_error(&journal, projection_factory);
        }
        journal = self.persistence.transition_credential_mutation_journal(
            journal.journal_id,
            CredentialMutationJournalState::Staged,
            CredentialMutationJournalState::FinalCredentialWritten,
        )?;
        self.commit_and_cleanup(journal, projection_factory)
            .map(Box::new)
            .map(CredentialClearCompletion::Committed)
    }

    fn resume_existing_write<F>(
        &self,
        journal: CredentialMutationJournalRecord,
        projection_factory: &mut F,
    ) -> Result<CredentialJournalCompletion, P05CredentialJournalError>
    where
        F: CredentialProjectionFactory,
    {
        match journal.state {
            CredentialMutationJournalState::Completed => self.completed_journal(journal),
            CredentialMutationJournalState::Aborted => {
                Err(P05CredentialJournalError::Aborted { journal_id: journal.journal_id })
            }
            CredentialMutationJournalState::ReconciliationRequired => {
                Err(P05CredentialJournalError::ReconciliationRequired {
                    journal_id: journal.journal_id,
                })
            }
            CredentialMutationJournalState::Prepared
            | CredentialMutationJournalState::Staged
            | CredentialMutationJournalState::FinalCredentialWritten
            | CredentialMutationJournalState::MetadataCommitted
            | CredentialMutationJournalState::CleanupPending => {
                match self.recover_one(journal, projection_factory)? {
                    CredentialRecoveryOutcome::Committed(completion) => Ok(*completion),
                    CredentialRecoveryOutcome::CleanupCompleted { journal_id } => {
                        let completed = self
                            .persistence
                            .credential_mutation_journal(journal_id)?
                            .ok_or(PersistenceError::CredentialJournalNotFound)?;
                        self.completed_journal(completed)
                    }
                    CredentialRecoveryOutcome::Aborted { journal_id } => {
                        Err(P05CredentialJournalError::Aborted { journal_id })
                    }
                    CredentialRecoveryOutcome::Reconciled { journal_id } => {
                        Err(P05CredentialJournalError::ReconciliationRequired { journal_id })
                    }
                }
            }
        }
    }

    fn completed_journal(
        &self,
        journal: CredentialMutationJournalRecord,
    ) -> Result<CredentialJournalCompletion, P05CredentialJournalError> {
        let credential_record = self
            .persistence
            .model_credential_record(journal.credential_id)?
            .ok_or(PersistenceError::CredentialRecordNotFound)?;
        if credential_record.row_revision != journal.target_row_revision {
            return Err(PersistenceError::CredentialRowRevisionConflict.into());
        }
        Ok(CredentialJournalCompletion {
            metadata_commit: ModelCredentialMetadataCommit {
                credential_record,
                journal,
                projection_changes: Vec::new(),
                projection_outbox_ids: Vec::new(),
            },
            cleanup_completed: true,
        })
    }

    fn recover<F>(
        &self,
        projection_factory: &mut F,
    ) -> Result<Vec<CredentialRecoveryOutcome>, P05CredentialJournalError>
    where
        F: CredentialProjectionFactory,
    {
        let journals = self.persistence.incomplete_credential_mutation_journals()?;
        journals
            .into_iter()
            .map(|journal| {
                let adopted =
                    self.persistence.adopt_credential_mutation_journal(journal.journal_id)?;
                self.recover_one(adopted, projection_factory)
            })
            .collect()
    }

    fn recover_one<F>(
        &self,
        mut journal: CredentialMutationJournalRecord,
        projection_factory: &mut F,
    ) -> Result<CredentialRecoveryOutcome, P05CredentialJournalError>
    where
        F: CredentialProjectionFactory,
    {
        loop {
            match journal.journal_kind {
                CredentialMutationJournalKind::Configure
                | CredentialMutationJournalKind::Replace => match journal.state {
                    CredentialMutationJournalState::Prepared => {
                        let staged = self.backend.read(&journal.staging_credential_ref()?)?;
                        let final_record = self.backend.read(&journal.binding.credential_ref)?;
                        let staged_attributes =
                            journal_attributes(&journal, WinCredentialRecordState::Staged)?;
                        let committed_attributes =
                            journal_attributes(&journal, WinCredentialRecordState::Committed)?;
                        let staged_current = staged
                            .as_ref()
                            .is_some_and(|record| record_matches(record, &staged_attributes, None));
                        let final_current = final_record.as_ref().is_some_and(|record| {
                            record_matches(record, &committed_attributes, None)
                        });
                        if staged.is_some() && !staged_current {
                            return self.reconciled_outcome(&journal, projection_factory);
                        }
                        if staged_current
                            && final_current
                            && !constant_time_bytes_equal(
                                staged.as_ref().expect("staged was checked").secret.expose(),
                                final_record.as_ref().expect("final was checked").secret.expose(),
                            )
                        {
                            return self.reconciled_outcome(&journal, projection_factory);
                        }
                        if staged_current || final_current {
                            journal = self.persistence.transition_credential_mutation_journal(
                                journal.journal_id,
                                CredentialMutationJournalState::Prepared,
                                CredentialMutationJournalState::Staged,
                            )?;
                        } else {
                            self.persistence.transition_credential_mutation_journal(
                                journal.journal_id,
                                CredentialMutationJournalState::Prepared,
                                CredentialMutationJournalState::Aborted,
                            )?;
                            return Ok(CredentialRecoveryOutcome::Aborted {
                                journal_id: journal.journal_id,
                            });
                        }
                    }
                    CredentialMutationJournalState::Staged => {
                        let staging_ref = journal.staging_credential_ref()?;
                        let staged = self.backend.read(&staging_ref)?;
                        let final_record = self.backend.read(&journal.binding.credential_ref)?;
                        let staged_attributes =
                            journal_attributes(&journal, WinCredentialRecordState::Staged)?;
                        let committed_attributes =
                            journal_attributes(&journal, WinCredentialRecordState::Committed)?;
                        let staged_current = staged
                            .as_ref()
                            .is_some_and(|record| record_matches(record, &staged_attributes, None));
                        let final_current = final_record.as_ref().is_some_and(|record| {
                            record_matches(record, &committed_attributes, None)
                        });
                        if staged.is_some() && !staged_current {
                            return self.reconciled_outcome(&journal, projection_factory);
                        }
                        if staged_current
                            && final_current
                            && !constant_time_bytes_equal(
                                staged.as_ref().expect("staged was checked").secret.expose(),
                                final_record.as_ref().expect("final was checked").secret.expose(),
                            )
                        {
                            return self.reconciled_outcome(&journal, projection_factory);
                        }
                        if !final_current {
                            let Some(staged) = staged.filter(|_| staged_current) else {
                                return self.reconciled_outcome(&journal, projection_factory);
                            };
                            self.backend.write(
                                &journal.binding.credential_ref,
                                &staged.secret,
                                &committed_attributes,
                            )?;
                            let verified = self
                                .backend
                                .read(&journal.binding.credential_ref)?
                                .ok_or(P05CredentialJournalError::VerificationMismatch)?;
                            if !record_matches(
                                &verified,
                                &committed_attributes,
                                Some(&staged.secret),
                            ) {
                                return self.reconciled_outcome(&journal, projection_factory);
                            }
                        }
                        journal = self.persistence.transition_credential_mutation_journal(
                            journal.journal_id,
                            CredentialMutationJournalState::Staged,
                            CredentialMutationJournalState::FinalCredentialWritten,
                        )?;
                    }
                    CredentialMutationJournalState::FinalCredentialWritten => {
                        if !self.write_final_is_valid(&journal)? {
                            return self.reconciled_outcome(&journal, projection_factory);
                        }
                        return self
                            .commit_and_cleanup(journal, projection_factory)
                            .map(Box::new)
                            .map(CredentialRecoveryOutcome::Committed);
                    }
                    CredentialMutationJournalState::MetadataCommitted => {
                        if !self.write_metadata_is_valid(&journal)? {
                            return self.reconciled_outcome(&journal, projection_factory);
                        }
                        journal = self.persistence.transition_credential_mutation_journal(
                            journal.journal_id,
                            CredentialMutationJournalState::MetadataCommitted,
                            CredentialMutationJournalState::CleanupPending,
                        )?;
                    }
                    CredentialMutationJournalState::CleanupPending => {
                        if !self.write_metadata_is_valid(&journal)? {
                            return self.reconciled_outcome(&journal, projection_factory);
                        }
                        self.complete_recovered_cleanup(&journal)?;
                        return Ok(CredentialRecoveryOutcome::CleanupCompleted {
                            journal_id: journal.journal_id,
                        });
                    }
                    CredentialMutationJournalState::Completed
                    | CredentialMutationJournalState::Aborted
                    | CredentialMutationJournalState::ReconciliationRequired => {
                        return Err(PersistenceError::CredentialJournalStateConflict.into());
                    }
                },
                CredentialMutationJournalKind::Clear => match journal.state {
                    CredentialMutationJournalState::Prepared => {
                        if self.backend.read(&journal.staging_credential_ref()?)?.is_some() {
                            return self.reconciled_outcome(&journal, projection_factory);
                        }
                        journal = self.persistence.transition_credential_mutation_journal(
                            journal.journal_id,
                            CredentialMutationJournalState::Prepared,
                            CredentialMutationJournalState::Staged,
                        )?;
                    }
                    CredentialMutationJournalState::Staged => {
                        if self.backend.read(&journal.staging_credential_ref()?)?.is_some() {
                            return self.reconciled_outcome(&journal, projection_factory);
                        }
                        self.backend.delete(&journal.binding.credential_ref)?;
                        if self.backend.read(&journal.binding.credential_ref)?.is_some() {
                            return self.reconciled_outcome(&journal, projection_factory);
                        }
                        journal = self.persistence.transition_credential_mutation_journal(
                            journal.journal_id,
                            CredentialMutationJournalState::Staged,
                            CredentialMutationJournalState::FinalCredentialWritten,
                        )?;
                    }
                    CredentialMutationJournalState::FinalCredentialWritten => {
                        if self.backend.read(&journal.binding.credential_ref)?.is_some() {
                            return self.reconciled_outcome(&journal, projection_factory);
                        }
                        return self
                            .commit_and_cleanup(journal, projection_factory)
                            .map(Box::new)
                            .map(CredentialRecoveryOutcome::Committed);
                    }
                    CredentialMutationJournalState::MetadataCommitted => {
                        if !self.clear_metadata_is_valid(&journal)? {
                            return self.reconciled_outcome(&journal, projection_factory);
                        }
                        journal = self.persistence.transition_credential_mutation_journal(
                            journal.journal_id,
                            CredentialMutationJournalState::MetadataCommitted,
                            CredentialMutationJournalState::CleanupPending,
                        )?;
                    }
                    CredentialMutationJournalState::CleanupPending => {
                        if !self.clear_metadata_is_valid(&journal)? {
                            return self.reconciled_outcome(&journal, projection_factory);
                        }
                        self.complete_recovered_cleanup(&journal)?;
                        return Ok(CredentialRecoveryOutcome::CleanupCompleted {
                            journal_id: journal.journal_id,
                        });
                    }
                    CredentialMutationJournalState::Completed
                    | CredentialMutationJournalState::Aborted
                    | CredentialMutationJournalState::ReconciliationRequired => {
                        return Err(PersistenceError::CredentialJournalStateConflict.into());
                    }
                },
                CredentialMutationJournalKind::Delete => {
                    return Err(PersistenceError::CredentialJournalStateConflict.into());
                }
            }
        }
    }

    fn write_final_is_valid(
        &self,
        journal: &CredentialMutationJournalRecord,
    ) -> Result<bool, P05CredentialJournalError> {
        let committed_attributes =
            journal_attributes(journal, WinCredentialRecordState::Committed)?;
        let final_record = self.backend.read(&journal.binding.credential_ref)?;
        let Some(final_record) = final_record else {
            return Ok(false);
        };
        if !record_matches(&final_record, &committed_attributes, None) {
            return Ok(false);
        }
        let staged = self.backend.read(&journal.staging_credential_ref()?)?;
        let Some(staged) = staged else {
            return Ok(true);
        };
        let staged_attributes = journal_attributes(journal, WinCredentialRecordState::Staged)?;
        Ok(record_matches(&staged, &staged_attributes, None)
            && constant_time_bytes_equal(staged.secret.expose(), final_record.secret.expose()))
    }

    fn write_metadata_is_valid(
        &self,
        journal: &CredentialMutationJournalRecord,
    ) -> Result<bool, P05CredentialJournalError> {
        if !self.write_final_is_valid(journal)? {
            return Ok(false);
        }
        let record = self
            .persistence
            .model_credential_record(journal.credential_id)?
            .ok_or(PersistenceError::CredentialRecordNotFound)?;
        Ok(record.state == ModelCredentialRecordState::Configured
            && record.row_revision == journal.target_row_revision
            && record.last_committed_credential_revision == journal.reserved_credential_revision
            && record.binding.credential_binding_fingerprint
                == journal.binding.credential_binding_fingerprint)
    }

    fn clear_metadata_is_valid(
        &self,
        journal: &CredentialMutationJournalRecord,
    ) -> Result<bool, P05CredentialJournalError> {
        if self.backend.read(&journal.binding.credential_ref)?.is_some()
            || self.backend.read(&journal.staging_credential_ref()?)?.is_some()
        {
            return Ok(false);
        }
        let record = self
            .persistence
            .model_credential_record(journal.credential_id)?
            .ok_or(PersistenceError::CredentialRecordNotFound)?;
        Ok(record.state == ModelCredentialRecordState::Cleared
            && record.row_revision == journal.target_row_revision
            && record.last_committed_credential_revision.is_some())
    }

    fn complete_recovered_cleanup(
        &self,
        journal: &CredentialMutationJournalRecord,
    ) -> Result<(), P05CredentialJournalError> {
        self.backend.delete(&journal.staging_credential_ref()?)?;
        self.persistence.transition_credential_mutation_journal(
            journal.journal_id,
            CredentialMutationJournalState::CleanupPending,
            CredentialMutationJournalState::Completed,
        )?;
        Ok(())
    }

    fn reconciled_outcome<F>(
        &self,
        journal: &CredentialMutationJournalRecord,
        projection_factory: &mut F,
    ) -> Result<CredentialRecoveryOutcome, P05CredentialJournalError>
    where
        F: CredentialProjectionFactory,
    {
        self.reconcile(journal, projection_factory)?;
        Ok(CredentialRecoveryOutcome::Reconciled { journal_id: journal.journal_id })
    }

    fn commit_and_cleanup<F>(
        &self,
        journal: CredentialMutationJournalRecord,
        projection_factory: &mut F,
    ) -> Result<CredentialJournalCompletion, P05CredentialJournalError>
    where
        F: CredentialProjectionFactory,
    {
        let status = self.pending_committed_status(&journal)?;
        let projections = projection_factory.projections(&journal, status)?;
        let metadata_commit = self
            .persistence
            .commit_credential_mutation_metadata(journal.journal_id, projections)?;
        self.persistence.transition_credential_mutation_journal(
            journal.journal_id,
            CredentialMutationJournalState::MetadataCommitted,
            CredentialMutationJournalState::CleanupPending,
        )?;
        let staging_ref = journal.staging_credential_ref()?;
        let cleanup_completed = match self.backend.delete(&staging_ref) {
            Ok(_) => {
                self.persistence.transition_credential_mutation_journal(
                    journal.journal_id,
                    CredentialMutationJournalState::CleanupPending,
                    CredentialMutationJournalState::Completed,
                )?;
                true
            }
            Err(_) => false,
        };
        Ok(CredentialJournalCompletion { metadata_commit, cleanup_completed })
    }

    fn pending_committed_status(
        &self,
        journal: &CredentialMutationJournalRecord,
    ) -> Result<EndpointCredentialStatus, P05CredentialJournalError> {
        let record = self
            .persistence
            .model_credential_record(journal.credential_id)?
            .ok_or(PersistenceError::CredentialRecordNotFound)?;
        let (state, active_credential_revision, last_committed_credential_revision) = match journal
            .journal_kind
        {
            CredentialMutationJournalKind::Configure | CredentialMutationJournalKind::Replace => {
                let revision = journal
                    .reserved_credential_revision
                    .ok_or(PersistenceError::CredentialJournalStateConflict)?;
                (EndpointCredentialState::Configured, Some(revision), Some(revision))
            }
            CredentialMutationJournalKind::Clear => {
                (EndpointCredentialState::Cleared, None, record.last_committed_credential_revision)
            }
            CredentialMutationJournalKind::Delete => {
                return Err(PersistenceError::CredentialJournalStateConflict.into());
            }
        };
        let status = EndpointCredentialStatus {
            endpoint_id: record.endpoint_id,
            slot_name: journal.binding.credential_slot,
            credential_id: journal.credential_id,
            state,
            active_credential_revision,
            last_committed_credential_revision,
            row_revision: journal.target_row_revision,
            error_code: None,
        };
        status
            .validate()
            .map_err(|error| P05CredentialJournalError::Projection(error.to_string()))?;
        Ok(status)
    }

    fn reconcile_error<T, F>(
        &self,
        journal: &CredentialMutationJournalRecord,
        projection_factory: &mut F,
    ) -> Result<T, P05CredentialJournalError>
    where
        F: CredentialProjectionFactory,
    {
        self.reconcile(journal, projection_factory)?;
        Err(P05CredentialJournalError::ReconciliationRequired { journal_id: journal.journal_id })
    }

    fn reconcile<F>(
        &self,
        journal: &CredentialMutationJournalRecord,
        projection_factory: &mut F,
    ) -> Result<(), P05CredentialJournalError>
    where
        F: CredentialProjectionFactory,
    {
        let status = self.persistence.credential_reconciliation_status(journal.journal_id, None)?;
        let projections = projection_factory.projections(journal, status)?;
        self.persistence.mark_credential_reconciliation_required(
            journal.journal_id,
            journal.state,
            None,
            projections,
        )?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum WinCredentialRecordState {
    Staged,
    Committed,
}

fn journal_attributes(
    journal: &CredentialMutationJournalRecord,
    state: WinCredentialRecordState,
) -> Result<Vec<CredentialApplicationAttribute>, P05CredentialJournalError> {
    let credential_revision = journal
        .reserved_credential_revision
        .ok_or(PersistenceError::CredentialJournalStateConflict)?;
    [
        (ATTRIBUTE_CREDENTIAL_ID, journal.credential_id.to_string()),
        (ATTRIBUTE_CREDENTIAL_REVISION, credential_revision.to_string()),
        (ATTRIBUTE_TRANSACTION_ID, journal.journal_id.to_string()),
        (
            ATTRIBUTE_RECORD_STATE,
            match state {
                WinCredentialRecordState::Staged => "staged",
                WinCredentialRecordState::Committed => "committed",
            }
            .to_owned(),
        ),
        (ATTRIBUTE_BUNDLE_NAMESPACE_HASH, journal.binding.bundle_namespace_key.clone()),
    ]
    .into_iter()
    .map(|(keyword, value)| {
        CredentialApplicationAttribute::new(keyword, value).map_err(P05CredentialJournalError::from)
    })
    .collect()
}

fn record_matches(
    actual: &BackendCredentialRecord,
    expected_attributes: &[CredentialApplicationAttribute],
    expected_secret: Option<&SecretBytes>,
) -> bool {
    let mut actual_attributes = actual.attributes.clone();
    let mut expected_attributes = expected_attributes.to_vec();
    actual_attributes.sort();
    expected_attributes.sort();
    actual_attributes == expected_attributes
        && expected_secret
            .is_none_or(|secret| constant_time_bytes_equal(actual.secret.expose(), secret.expose()))
}

fn delete_record_matches(record: &ModelCredentialRecord, actual: &BackendCredentialRecord) -> bool {
    let Some(credential_revision) = record.last_committed_credential_revision else {
        return false;
    };
    let attributes = actual
        .attributes
        .iter()
        .map(|attribute| (attribute.keyword(), attribute.value()))
        .collect::<std::collections::BTreeMap<_, _>>();
    attributes.len() == 5
        && attributes.get(ATTRIBUTE_CREDENTIAL_ID).copied()
            == Some(record.credential_id.to_string().as_str())
        && attributes.get(ATTRIBUTE_CREDENTIAL_REVISION).copied()
            == Some(credential_revision.to_string().as_str())
        && attributes.get(ATTRIBUTE_RECORD_STATE).copied() == Some("committed")
        && attributes.get(ATTRIBUTE_BUNDLE_NAMESPACE_HASH).copied()
            == Some(record.binding.bundle_namespace_key.as_str())
        && attributes
            .get(ATTRIBUTE_TRANSACTION_ID)
            .is_some_and(|value| value.parse::<MutationJournalId>().is_ok())
}

fn constant_time_bytes_equal(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    let longest = left.len().max(right.len());
    for index in 0..longest {
        let left_byte = left.get(index).copied().unwrap_or_default();
        let right_byte = right.get(index).copied().unwrap_or_default();
        difference |= usize::from(left_byte ^ right_byte);
    }
    difference == 0
}

#[cfg(test)]
mod tests;
