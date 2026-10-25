//! Attachment grants are prepared as one receipted legacy transaction before message publication.
use aura_domain_common::{CorrelationId, ExternalGrantId};
use aura_permission_domain::{
    ExternalGrantCapability, ExternalGrantLifecycle, ExternalGrantRecord, ExternalGrantTargetKind,
    ExternalGrantTerm,
};
use rusqlite::TransactionBehavior;

use super::{insert_external_grant, load_external_grant, validate_external_grant};
use crate::{PersistenceCoordinator, PersistenceError};

impl PersistenceCoordinator {
    pub fn create_external_grant(
        &self,
        record: ExternalGrantRecord,
        correlation_id: CorrelationId,
    ) -> Result<ExternalGrantRecord, PersistenceError> {
        validate_external_grant(&record)?;
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            insert_external_grant(&transaction, &record, correlation_id)?;
            transaction.commit()?;
            Ok(record)
        })
    }

    /// Prepare this message's single-file reads without replacing or reactivating any grant.
    ///
    /// Await the admitted job's original receipt; never replay it after losing a timed wait.
    /// Only newly inserted IDs are returned, so a known publication failure can revoke those
    /// grants without withdrawing authority that an earlier message already established.
    pub fn prepare_attachment_grants(
        &self,
        records: Vec<ExternalGrantRecord>,
        correlation_id: CorrelationId,
    ) -> Result<
        tokio::sync::oneshot::Receiver<Result<Vec<ExternalGrantId>, PersistenceError>>,
        PersistenceError,
    > {
        for record in &records {
            validate_attachment_candidate(record)?;
        }
        let identity = format!(
            "attachment-grants:{}",
            records.iter().map(|record| record.id.to_string()).collect::<Vec<_>>().join(",")
        );
        self.dispatch_receipt(identity, move |connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mut created = Vec::new();
            for candidate in records {
                if let Some(existing) = load_external_grant(&transaction, candidate.id)? {
                    if validate_attachment_candidate(&existing).is_err()
                        || !same_attachment_binding(&existing, &candidate)
                    {
                        return Err(PersistenceError::ExternalGrantStateConflict);
                    }
                } else {
                    insert_external_grant(&transaction, &candidate, correlation_id)?;
                    created.push(candidate.id);
                }
            }
            transaction.commit()?;
            Ok(created)
        })
    }
}

fn validate_attachment_candidate(record: &ExternalGrantRecord) -> Result<(), PersistenceError> {
    validate_external_grant(record)?;
    if record.lifecycle != ExternalGrantLifecycle::Active
        || record.target_kind != ExternalGrantTargetKind::File
        || record.capabilities.as_slice() != [ExternalGrantCapability::Read]
        || record.term != ExternalGrantTerm::Session
        || record.recursive_scope
        || record.permission_grant_id.is_some()
        || record.turn_task_id.is_some()
        || record.expires_at.is_some()
    {
        return Err(PersistenceError::InvalidRequest(
            "attachment grant must be an active session-scoped single-file read".to_owned(),
        ));
    }
    Ok(())
}

/// Retry timestamps may differ; the authority revision and every security binding must match.
fn same_attachment_binding(
    existing: &ExternalGrantRecord,
    candidate: &ExternalGrantRecord,
) -> bool {
    existing.session_id == candidate.session_id
        && existing.workspace_id == candidate.workspace_id
        && existing.permission_grant_id == candidate.permission_grant_id
        && existing.selected_root == candidate.selected_root
        && existing.canonical_root == candidate.canonical_root
        && existing.root_identity == candidate.root_identity
        && existing.target_kind == candidate.target_kind
        && existing.capabilities == candidate.capabilities
        && existing.recursive_scope == candidate.recursive_scope
        && existing.term == candidate.term
        && existing.turn_task_id == candidate.turn_task_id
        && existing.expires_at == candidate.expires_at
        && existing.revalidation_revision == candidate.revalidation_revision
}
