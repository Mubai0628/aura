//! Deleting the credentials a model-settings deletion leaves behind, once that settings change is
//! in effect.
//!
//! The deletion's own journal tries each credential once and then completes, so the model-settings
//! lease is not held while Windows Credential Manager refuses a delete. A credential still there
//! keeps its own delete journal under its own credential lease, and is tried again while Aura runs,
//! after 5 s, 30 s, 2 min and 10 min and then every 30 min, and whenever the user asks.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use aura_core_base::append_core_log;
use aura_persistence::{
    CredentialMutationJournalKind, CredentialMutationJournalRecord, CredentialMutationJournalState,
    ModelCredentialRecordState, PersistenceError,
};

use super::{
    CredentialBackend, CredentialDeleteChildOutcome, CredentialJournalCoordinator,
    P05CredentialJournal, P05CredentialJournalError, delete_record_matches,
};

/// The waits before the first background retries; after them, [`RETRY_EVERY`].
const RETRY_AFTER: [Duration; 4] = [
    Duration::from_secs(5),
    Duration::from_secs(30),
    Duration::from_secs(120),
    Duration::from_secs(600),
];
const RETRY_EVERY: Duration = Duration::from_secs(1800);

/// What one retry of the credentials completed changes left did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RetriedDeletes {
    /// Deleted by this retry.
    pub(crate) removed: usize,
    /// Still in Windows Credential Manager.
    pub(crate) pending: usize,
}

/// Deletes the credentials of a model-settings change that is in effect.
pub(crate) trait CredentialCleanup {
    /// Deletes one credential the change left, or says why it is still there.
    fn delete_published_child(
        &self,
        journal: CredentialMutationJournalRecord,
    ) -> Result<CredentialDeleteChildOutcome, P05CredentialJournalError>;

    /// Tries every credential a completed change left once more.
    fn retry_pending_deletes(&self) -> Result<RetriedDeletes, P05CredentialJournalError>;

    /// Whether a completed change still left a credential.
    fn has_pending_deletes(&self) -> bool;

    /// Keeps trying in the background while any is still there.
    fn schedule_retry(&self) {}
}

impl<B> CredentialCleanup for CredentialJournalCoordinator<B>
where
    B: CredentialBackend,
{
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

    fn retry_pending_deletes(&self) -> Result<RetriedDeletes, P05CredentialJournalError> {
        let mut retried = RetriedDeletes::default();
        for delete in self.persistence.pending_credential_deletes()? {
            // After a restart the delete journal takes its own lease again; while Aura runs it
            // still holds it.
            let outcome = self
                .persistence
                .adopt_credential_mutation_journal(delete.journal_id)
                .map_err(P05CredentialJournalError::from)
                .and_then(|delete| self.delete_published_child(delete));
            match outcome {
                Ok(CredentialDeleteChildOutcome::Completed) => retried.removed += 1,
                // An entry that no longer matches its record is left to reconciliation, as at
                // the change's own attempt; retrying cannot delete it.
                Ok(CredentialDeleteChildOutcome::ReconciliationRequired) => {}
                Ok(CredentialDeleteChildOutcome::CleanupPending) | Err(_) => retried.pending += 1,
            }
        }
        Ok(retried)
    }

    fn has_pending_deletes(&self) -> bool {
        self.persistence.pending_credential_deletes().is_ok_and(|pending| !pending.is_empty())
    }
}

impl CredentialCleanup for P05CredentialJournal {
    fn delete_published_child(
        &self,
        journal: CredentialMutationJournalRecord,
    ) -> Result<CredentialDeleteChildOutcome, P05CredentialJournalError> {
        self.coordinator.delete_published_child(journal)
    }

    fn retry_pending_deletes(&self) -> Result<RetriedDeletes, P05CredentialJournalError> {
        let _one_attempt_at_a_time =
            self.retry.attempt.lock().unwrap_or_else(PoisonError::into_inner);
        self.coordinator.retry_pending_deletes()
    }

    fn has_pending_deletes(&self) -> bool {
        self.coordinator.has_pending_deletes()
    }

    fn schedule_retry(&self) {
        retry_in_background(
            self.clone(),
            Arc::clone(&self.retry),
            RETRY_AFTER.into_iter().chain(std::iter::repeat(RETRY_EVERY)),
        );
    }
}

/// One background retry and one attempt at a time.
#[derive(Default)]
pub(crate) struct RetryState {
    running: AtomicBool,
    attempt: Mutex<()>,
}

/// Retries after each of `delays` until no credential is left, on a thread of its own: an attempt
/// is synchronous calls to Credential Manager and the store.
pub(crate) fn retry_in_background<C>(
    cleanup: C,
    state: Arc<RetryState>,
    delays: impl Iterator<Item = Duration> + Send + 'static,
) where
    C: CredentialCleanup + Send + 'static,
{
    if state.running.swap(true, Ordering::SeqCst) {
        return;
    }
    let thread_state = Arc::clone(&state);
    let started =
        std::thread::Builder::new().name("aura-credential-cleanup".to_owned()).spawn(move || {
            for delay in delays {
                std::thread::sleep(delay);
                match cleanup.retry_pending_deletes() {
                    Ok(RetriedDeletes { pending: 0, .. }) => {
                        thread_state.running.store(false, Ordering::SeqCst);
                        // A deletion that ended while this thread was stopping found it running.
                        if !cleanup.has_pending_deletes()
                            || thread_state.running.swap(true, Ordering::SeqCst)
                        {
                            return;
                        }
                    }
                    Ok(retried) => append_core_log(
                        "credential_cleanup_still_pending",
                        serde_json::json!({"credentials": retried.pending}),
                    ),
                    Err(error) => append_core_log(
                        "credential_cleanup_retry_failed",
                        serde_json::json!({"reason": error.to_string()}),
                    ),
                }
            }
            thread_state.running.store(false, Ordering::SeqCst);
        });
    if started.is_err() {
        state.running.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests;
