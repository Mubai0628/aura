//! Tool Secrets have their own authority and mutation journal. The platform store is shared,
//! never the model credential rows, grant rules, or recovery state.
use aura_core_base::{append_core_log, credential_io};
use aura_domain_common::{CommandInvocationId, CredentialId, CredentialRevision};
use aura_persistence::{
    PersistenceCoordinator, PersistenceError, ToolCredentialMutationIntentV1,
    ToolCredentialMutationJournalV1,
};
use aura_tool_domain::{
    ToolCredentialManagementTargetV1, ToolCredentialMutationDispositionV1 as Disposition,
    ToolCredentialMutationResultV1,
};
use aura_windows_platform::{
    CredentialApplicationAttribute, CredentialStoreError, SecretBytes, StoredCredential,
    WindowsCredentialStore,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock, Weak},
};
use thiserror::Error;
pub mod execution;

#[derive(Debug, Error)]
pub enum ToolSecretError {
    #[error("tool credential metadata changed; refresh before confirming")]
    Conflict,
    #[error("tool credential is not available for this owner, scope, tool and placement")]
    NotAuthorized,
    #[error("the stored tool credential is missing")]
    Missing,
    #[error("Windows denied access to the tool credential")]
    AccessDenied,
    #[error("tool credential input was cancelled")]
    Cancelled,
    #[error("tool credential state could not be obtained")]
    NotObtained,
    #[error("tool credential identity could not be verified")]
    IdentityMismatch,
}
impl From<PersistenceError> for ToolSecretError {
    fn from(error: PersistenceError) -> Self {
        match error {
            PersistenceError::CredentialRowRevisionConflict
            | PersistenceError::ToolEntityRevisionConflict
            | PersistenceError::CredentialJournalStateConflict => Self::Conflict,
            PersistenceError::CredentialBindingMismatch
            | PersistenceError::CredentialUnavailable
            | PersistenceError::CredentialRecordNotFound
            | PersistenceError::ToolEntityNotFound => Self::NotAuthorized,
            _ => Self::NotObtained,
        }
    }
}
impl From<CredentialStoreError> for ToolSecretError {
    fn from(error: CredentialStoreError) -> Self {
        match error {
            CredentialStoreError::NotFound { .. } => Self::Missing,
            CredentialStoreError::Windows(error) if error.raw_os_error() == Some(5) => {
                Self::AccessDenied
            }
            CredentialStoreError::Windows(error) if error.raw_os_error() == Some(1223) => {
                Self::Cancelled
            }
            _ => Self::NotObtained,
        }
    }
}

#[derive(Clone)]
pub struct ToolCredentialService {
    persistence: Arc<PersistenceCoordinator>,
}
impl ToolCredentialService {
    pub fn new(persistence: Arc<PersistenceCoordinator>) -> Self {
        Self { persistence }
    }

    pub fn mutate(
        &self,
        command: CommandInvocationId,
        intent: ToolCredentialMutationIntentV1,
        secret: Option<&SecretBytes>,
    ) -> Result<ToolCredentialMutationResultV1, ToolSecretError> {
        if secret.is_some_and(|s| s.expose().is_empty())
            || intent.writes_secret() != secret.is_some()
        {
            return Err(ToolSecretError::NotAuthorized);
        }
        let owner = mutation_owner(command)?;
        let _guard = owner.try_lock().map_err(|_| ToolSecretError::Conflict)?;
        let mut journal = self.persistence.prepare_tool_credential_mutation_v1(command, intent)?;
        match journal.state.as_str() {
            "cleaned" | "committed" => return self.finish(journal),
            "prepared" => (),
            // The original caller may still own the platform write. Never replay input on a retry.
            _ => return Err(ToolSecretError::Conflict),
        }
        if let Some(secret) = secret {
            if secret.expose().is_empty() {
                return Err(ToolSecretError::NotAuthorized);
            }
            let reference = journal.new_ref.as_ref().ok_or(ToolSecretError::IdentityMismatch)?;
            let attrs = attributes(&journal)?;
            if let Err(error) =
                WindowsCredentialStore.write_with_attributes(reference, secret, &attrs)
            {
                // Keep the original platform failure. A failed journal update does not replace it.
                let _ = self.persistence.transition_tool_credential_mutation_v1(
                    journal.journal_id,
                    "prepared",
                    "aborted",
                );
                return Err(error.into());
            }
            verify_record(&credential_io::read(reference)?, &journal)?;
            journal = self.persistence.transition_tool_credential_mutation_v1(
                journal.journal_id,
                "prepared",
                "staged",
            )?;
        }
        self.finish(journal)
    }

    fn finish(
        &self,
        mut journal: ToolCredentialMutationJournalV1,
    ) -> Result<ToolCredentialMutationResultV1, ToolSecretError> {
        if journal.state == "staged" {
            verify_record(
                &credential_io::read(
                    journal.new_ref.as_ref().ok_or(ToolSecretError::IdentityMismatch)?,
                )?,
                &journal,
            )?;
        }
        if matches!(journal.state.as_str(), "prepared" | "staged") {
            match self.persistence.commit_tool_credential_mutation_v1(journal.journal_id) {
                Ok(committed) => journal = committed,
                Err(
                    PersistenceError::CredentialRowRevisionConflict
                    | PersistenceError::ToolEntityRevisionConflict
                    | PersistenceError::ToolEntityNotFound,
                ) => {
                    if self.abort_uncommitted(&journal).is_err() {
                        // The original CAS conflict remains the cause. The durable pending
                        // journal keeps the independently failed cleanup visible for recovery.
                        append_core_log(
                            "tool_credential_abort_pending",
                            serde_json::json!({"journal_id":journal.journal_id,"primary":"metadata_conflict","cleanup":"not_obtained"}),
                        );
                    }
                    return Err(ToolSecretError::Conflict);
                }
                Err(error) => return Err(error.into()),
            }
        }
        if journal.state != "cleaned" {
            // An old value is removed only after the new metadata/Secret binding is committed.
            if let Some(reference) = &journal.old_ref {
                match credential_io::read(reference) {
                    Ok(stored) => {
                        if verify_old_record(&stored, &journal).is_err()
                            || WindowsCredentialStore.delete(reference).is_err()
                        {
                            return Ok(committed(journal, true));
                        }
                    }
                    Err(CredentialStoreError::NotFound { .. }) => (),
                    Err(_) => return Ok(committed(journal, true)),
                }
            }
            match self.persistence.transition_tool_credential_mutation_v1(
                journal.journal_id,
                "committed",
                "cleaned",
            ) {
                Ok(cleaned) => journal = cleaned,
                Err(_) => return Ok(committed(journal, true)),
            }
        }
        if self
            .persistence
            .refresh_managed_tool_credential_projections(journal.intent.target().clone())
            .and_then(|changes| self.persistence.publish_tool_projection_changes(&changes))
            .is_err()
        {
            append_core_log(
                "tool_credential_projection_refresh_failed",
                serde_json::json!({"journal_id":journal.journal_id,"reason":"projection_unavailable"}),
            );
        }
        append_core_log(
            "tool_credential_committed",
            serde_json::json!({"credential_id":journal.record.credential_id,"revision":journal.record.credential_revision,"journal_id":journal.journal_id}),
        );
        Ok(committed(journal, false))
    }

    fn abort_uncommitted(
        &self,
        journal: &ToolCredentialMutationJournalV1,
    ) -> Result<(), ToolSecretError> {
        if let Some(reference) = &journal.new_ref {
            match credential_io::read(reference) {
                Ok(stored) => {
                    verify_record(&stored, journal)?;
                    WindowsCredentialStore.delete(reference)?;
                }
                Err(CredentialStoreError::NotFound { .. }) => (),
                Err(error) => return Err(error.into()),
            }
        }
        self.persistence.transition_tool_credential_mutation_v1(
            journal.journal_id,
            &journal.state,
            "aborted",
        )?;
        Ok(())
    }

    /// Startup or explicit user recovery only, never called to race another live mutation.
    /// Prepared writes have no committed input and are aborted; staged writes retain the
    /// original user's authorized intent. Other credentials remain usable if one fails.
    pub fn recover(
        &self,
        credential: Option<CredentialId>,
    ) -> Result<Vec<Result<ToolCredentialMutationResultV1, ToolSecretError>>, ToolSecretError> {
        let journals = self.persistence.incomplete_tool_credential_mutations_v1()?;
        let owner = self.persistence.current_tool_principal()?;
        Ok(journals
            .into_iter()
            .filter(|j| {
                j.record.authenticated_owner == owner
                    && credential.is_none_or(|id| j.record.credential_id == id)
            })
            .map(|journal| {
                let owner = mutation_owner(journal.command_invocation_id)?;
                let _guard = owner.try_lock().map_err(|_| ToolSecretError::Conflict)?;
                if journal.state == "prepared" && journal.intent.writes_secret() {
                    self.abort_uncommitted(&journal)?;
                    return Ok(ToolCredentialMutationResultV1 {
                        credential: None,
                        disposition: Disposition::Aborted,
                        cleanup_pending: false,
                    });
                }
                self.finish(journal)
            })
            .collect())
    }

    /// Only Core execution adapters call this. No Renderer API returns a secret value.
    pub fn read_authorized(
        &self,
        target: ToolCredentialManagementTargetV1,
        id: CredentialId,
        revision: CredentialRevision,
    ) -> Result<SecretBytes, ToolSecretError> {
        let (record, reference) =
            self.persistence.authorized_tool_secret_binding_v1(target, id, revision)?;
        let stored = credential_io::read(&reference)?;
        let attrs = attribute_map(&stored);
        let owner = record.authenticated_owner.to_string();
        if attrs.len() != 5
            || attrs.get("aura.purpose").copied() != Some("tool_secret_v1")
            || attrs.get("aura.credential_id").copied() != Some(id.to_string().as_str())
            || attrs.get("aura.revision").copied() != Some(revision.to_string().as_str())
            || attrs.get("aura.owner").copied() != Some(owner.as_str())
            || attrs
                .get("aura.journal_id")
                .is_none_or(|v| v.parse::<aura_domain_common::MutationJournalId>().is_err())
        {
            return Err(ToolSecretError::IdentityMismatch);
        }
        let expected_prefix = format!("tool/{}/", blake3::hash(owner.as_bytes()).to_hex());
        if !reference.path().starts_with(&expected_prefix) {
            return Err(ToolSecretError::IdentityMismatch);
        }
        Ok(stored.into_secret())
    }
}

fn committed(
    journal: ToolCredentialMutationJournalV1,
    cleanup_pending: bool,
) -> ToolCredentialMutationResultV1 {
    ToolCredentialMutationResultV1 {
        credential: Some(journal.record),
        disposition: Disposition::Committed,
        cleanup_pending,
    }
}

fn mutation_owner(command: CommandInvocationId) -> Result<Arc<Mutex<()>>, ToolSecretError> {
    static OWNERS: OnceLock<Mutex<BTreeMap<CommandInvocationId, Weak<Mutex<()>>>>> =
        OnceLock::new();
    let mut owners =
        OWNERS.get_or_init(Mutex::default).lock().map_err(|_| ToolSecretError::NotObtained)?;
    owners.retain(|_, entry| entry.strong_count() > 0);
    let owner =
        owners.get(&command).and_then(Weak::upgrade).unwrap_or_else(|| Arc::new(Mutex::new(())));
    owners.insert(command, Arc::downgrade(&owner));
    Ok(owner)
}

pub fn attributes(
    journal: &ToolCredentialMutationJournalV1,
) -> Result<Vec<CredentialApplicationAttribute>, ToolSecretError> {
    [
        ("aura.purpose", "tool_secret_v1".into()),
        ("aura.credential_id", journal.record.credential_id.to_string()),
        ("aura.revision", journal.record.credential_revision.to_string()),
        ("aura.owner", journal.record.authenticated_owner.to_string()),
        ("aura.journal_id", journal.journal_id.to_string()),
    ]
    .into_iter()
    .map(|(k, v)| CredentialApplicationAttribute::new(k, v).map_err(Into::into))
    .collect()
}
fn attribute_map(stored: &StoredCredential) -> BTreeMap<&str, &str> {
    stored.attributes().iter().map(|a| (a.keyword(), a.value())).collect()
}
fn verify_record(
    stored: &StoredCredential,
    journal: &ToolCredentialMutationJournalV1,
) -> Result<(), ToolSecretError> {
    let expected = attributes(journal)?;
    if attribute_map(stored) != expected.iter().map(|a| (a.keyword(), a.value())).collect() {
        return Err(ToolSecretError::IdentityMismatch);
    }
    Ok(())
}
fn verify_old_record(
    stored: &StoredCredential,
    journal: &ToolCredentialMutationJournalV1,
) -> Result<(), ToolSecretError> {
    let attrs = attribute_map(stored);
    if attrs.len() != 5
        || attrs.get("aura.purpose").copied() != Some("tool_secret_v1")
        || attrs.get("aura.credential_id").copied()
            != Some(journal.record.credential_id.to_string().as_str())
        || attrs.get("aura.owner").copied()
            != Some(journal.record.authenticated_owner.to_string().as_str())
        || attrs.get("aura.revision").is_none_or(|s| {
            s.parse::<u64>().ok() != journal.record.credential_revision.value().checked_sub(1)
        })
    {
        return Err(ToolSecretError::IdentityMismatch);
    }
    Ok(())
}
