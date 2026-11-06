//! A deletion whose credential Credential Manager refuses to remove: the settings change completes
//! and gives up the model-config lease at once, the key keeps its own delete journal, and a later
//! retry removes it. Credential Manager itself is a fake here.
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use aura_domain_common::{CommandInvocationId, CorrelationId, CredentialId, CredentialRef};
use aura_persistence::{
    CredentialMutationJournalRecord, CredentialMutationJournalState,
    ModelConfigMutationJournalState,
};
use aura_windows_platform::{CredentialApplicationAttribute, CredentialStoreError, SecretBytes};
use serde_json::json;

use super::super::LEASE_WAIT;
use super::fixture::{Fixture, credential_deletion_plan, unwritten_credential_slot};
use crate::p05_credential::cleanup::RetriedDeletes;
use crate::p05_credential::{
    BackendCredentialRecord, CredentialBackend, CredentialCleanup, CredentialDeleteChildOutcome,
    CredentialJournalCoordinator, P05CredentialJournalError,
};

/// Credential Manager, holding the keys in memory, refusing every delete while `refusing` is set.
#[derive(Clone, Default)]
struct FakeCredentialManager {
    keys: Arc<Mutex<BTreeSet<String>>>,
    refusing: Arc<AtomicBool>,
}

impl CredentialBackend for FakeCredentialManager {
    fn write(
        &self,
        reference: &CredentialRef,
        _: &SecretBytes,
        _: &[CredentialApplicationAttribute],
    ) -> Result<(), CredentialStoreError> {
        self.keys.lock().expect("keys").insert(reference.to_string());
        Ok(())
    }

    fn read(
        &self,
        _: &CredentialRef,
    ) -> Result<Option<BackendCredentialRecord>, CredentialStoreError> {
        Ok(None)
    }

    fn delete(&self, reference: &CredentialRef) -> Result<bool, CredentialStoreError> {
        if self.refusing.load(Ordering::SeqCst) {
            return Err(CredentialStoreError::InvalidBuffer);
        }
        Ok(self.keys.lock().expect("keys").remove(&reference.to_string()))
    }
}

/// The cleanup over the fake, counting the background retries it is asked to start.
struct Cleanup {
    coordinator: CredentialJournalCoordinator<FakeCredentialManager>,
    scheduled: AtomicUsize,
}

impl CredentialCleanup for Cleanup {
    fn delete_published_child(
        &self,
        journal: CredentialMutationJournalRecord,
    ) -> Result<CredentialDeleteChildOutcome, P05CredentialJournalError> {
        self.coordinator.delete_published_child(journal)
    }

    fn retry_pending_deletes(&self) -> Result<RetriedDeletes, P05CredentialJournalError> {
        self.coordinator.retry_pending_deletes()
    }

    fn has_pending_deletes(&self) -> bool {
        self.coordinator.has_pending_deletes()
    }

    fn schedule_retry(&self) {
        self.scheduled.fetch_add(1, Ordering::SeqCst);
    }
}

/// One credential slot, and a deletion of it that Credential Manager refuses.
fn refused_deletion() -> (Fixture, Cleanup, CommandInvocationId) {
    let fixture = Fixture::new();
    let credential_id = CredentialId::new();
    fixture
        .persistence
        .create_model_credential_slot(unwritten_credential_slot(credential_id))
        .expect("credential slot");
    let manager = FakeCredentialManager::default();
    manager.refusing.store(true, Ordering::SeqCst);
    let cleanup = Cleanup {
        coordinator: CredentialJournalCoordinator {
            persistence: Arc::clone(&fixture.persistence),
            backend: manager,
        },
        scheduled: AtomicUsize::new(0),
    };
    let command = CommandInvocationId::new();
    let completion = fixture
        .coordinator
        .execute_with_credential_deletes(&cleanup, command, CorrelationId::new(), |journal_id| {
            let plan = fixture.provider_plan(journal_id, CorrelationId::new(), "Deleted")?;
            let mut deletion = credential_deletion_plan(plan, vec![credential_id]);
            deletion.success_result = json!({"value": {"deleted": true}});
            Ok(deletion)
        })
        .expect("the deletion is in effect");
    assert_eq!(completion.success_result["value"]["credential_cleanup_pending"], true);
    (fixture, cleanup, command)
}

#[test]
fn a_refused_credential_delete_completes_the_change_and_frees_the_model_config_lease() {
    let (fixture, cleanup, command) = refused_deletion();
    let parent = fixture
        .persistence
        .model_config_mutation_journal_by_command(command)
        .expect("query the deletion")
        .expect("the deletion's journal");
    assert_eq!(parent.state, ModelConfigMutationJournalState::Completed);

    let started = Instant::now();
    fixture.create_provider("Next").expect("the next model-settings write is admitted");
    assert!(
        started.elapsed() < LEASE_WAIT,
        "it did not wait for the lease: {:?}",
        started.elapsed()
    );

    let pending = fixture.persistence.pending_credential_deletes().expect("pending deletes");
    assert_eq!(pending.len(), 1, "the key keeps its own delete journal");
    assert_eq!(pending[0].state, CredentialMutationJournalState::Staged);
    assert_eq!(cleanup.scheduled.load(Ordering::SeqCst), 1, "a background retry is started");

    let replay = fixture
        .coordinator
        .execute_with_credential_deletes(&cleanup, command, CorrelationId::new(), |_| {
            panic!("a replay does not prepare the deletion again")
        })
        .expect("replay");
    assert_eq!(replay.success_result["value"]["credential_cleanup_pending"], true);
}

#[test]
fn a_later_retry_removes_the_key_and_a_replay_no_longer_reports_it() {
    let (fixture, cleanup, command) = refused_deletion();
    let refused = cleanup.retry_pending_deletes().expect("retry");
    assert_eq!(refused, RetriedDeletes { removed: 0, pending: 1 }, "still refused");

    cleanup.coordinator.backend.refusing.store(false, Ordering::SeqCst);
    let retried = cleanup.retry_pending_deletes().expect("retry");
    assert_eq!(retried, RetriedDeletes { removed: 1, pending: 0 });
    assert!(fixture.persistence.pending_credential_deletes().expect("pending deletes").is_empty());
    assert!(!cleanup.has_pending_deletes());

    let replay = fixture
        .coordinator
        .execute_with_credential_deletes(&cleanup, command, CorrelationId::new(), |_| {
            panic!("a replay does not prepare the deletion again")
        })
        .expect("replay");
    assert_eq!(replay.success_result["value"]["credential_cleanup_pending"], false);
}
