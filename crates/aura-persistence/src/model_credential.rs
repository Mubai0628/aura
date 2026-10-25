use std::str::FromStr;

use aura_domain_common::{
    CommandInvocationId, CorrelationId, CredentialId, CredentialRef, CredentialRevision,
    EndpointId, ErrorCode, EventId, ExpectedRevision, MutationJournalId, NormalizedIssueCode,
    Revision, UtcTimestamp,
};
use aura_model_domain::{
    CredentialBindingFingerprint, EndpointAuthScheme, EndpointCredentialState,
    EndpointCredentialStatus, ModelCredentialSlot, ModelEndpointCredentialClearedPayload,
    ModelEndpointCredentialConfiguredPayload,
};
use aura_permission_domain::NormalizedOrigin;
use aura_projection_contract::ProjectionChange;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::{
    PersistenceCoordinator, PersistenceError, PersistentEventDraft,
    coordinator::{insert_event, to_sql_u64},
    model_projection::{ModelProjectionDraft, insert_model_projections},
    mutation_journal::{
        MutationJournalRecord, PrepareMutationJournal, insert_prepared_mutation_journal,
        load_mutation_journal,
    },
};

const CREDENTIAL_BACKEND_KIND: &str = "wincred";
const WINCRED_TYPE: &str = "generic";
const CREDENTIAL_RESOURCE_KIND: &str = "model_credential";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelCredentialRecordState {
    Unconfigured,
    Configured,
    Cleared,
    Unavailable,
    ReconciliationRequired,
    DeletionPending,
}

impl ModelCredentialRecordState {
    #[must_use]
    pub const fn renderer_state(self) -> Option<EndpointCredentialState> {
        match self {
            Self::Unconfigured => Some(EndpointCredentialState::Unconfigured),
            Self::Configured => Some(EndpointCredentialState::Configured),
            Self::Cleared => Some(EndpointCredentialState::Cleared),
            Self::Unavailable => Some(EndpointCredentialState::Unavailable),
            Self::ReconciliationRequired => Some(EndpointCredentialState::ReconciliationRequired),
            Self::DeletionPending => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialMutationJournalKind {
    Configure,
    Replace,
    Clear,
    Delete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialMutationJournalState {
    Prepared,
    Staged,
    FinalCredentialWritten,
    MetadataCommitted,
    CleanupPending,
    Completed,
    Aborted,
    ReconciliationRequired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCredentialStorageBinding {
    pub credential_ref: CredentialRef,
    pub credential_binding_fingerprint: CredentialBindingFingerprint,
    pub bundle_namespace_key: String,
    pub backend_kind: String,
    pub wincred_type: String,
    pub final_target: String,
    pub normalized_origin: NormalizedOrigin,
    pub auth_scheme: EndpointAuthScheme,
    pub credential_slot: ModelCredentialSlot,
}

impl ModelCredentialStorageBinding {
    pub fn validate(&self, credential_id: CredentialId) -> Result<(), PersistenceError> {
        if self.backend_kind != CREDENTIAL_BACKEND_KIND
            || self.wincred_type != WINCRED_TYPE
            || self.auth_scheme == EndpointAuthScheme::None
            || self.bundle_namespace_key.trim().is_empty()
        {
            return Err(PersistenceError::InvalidRequest(
                "model credential storage binding has invalid fixed metadata".to_owned(),
            ));
        }
        let expected_target =
            format!("Aura/{}/credential/{credential_id}", self.bundle_namespace_key);
        if self.final_target != expected_target
            || self.credential_ref.windows_target_name() != expected_target
        {
            return Err(PersistenceError::InvalidRequest(
                "model credential storage locator does not match CredentialId".to_owned(),
            ));
        }
        if self.normalized_origin.scheme.trim().is_empty()
            || self.normalized_origin.host.trim().is_empty()
            || self.normalized_origin.port == 0
        {
            return Err(PersistenceError::InvalidRequest(
                "model credential normalized origin is invalid".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelCredentialRecord {
    pub credential_id: CredentialId,
    pub endpoint_id: EndpointId,
    pub credential_slot: ModelCredentialSlot,
    pub state: ModelCredentialRecordState,
    pub row_revision: Revision,
    pub last_committed_credential_revision: Option<CredentialRevision>,
    pub binding: ModelCredentialStorageBinding,
    pub error_code: Option<NormalizedIssueCode>,
    pub configured_at: Option<UtcTimestamp>,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

impl ModelCredentialRecord {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        if self.row_revision.value() == 0 {
            return Err(PersistenceError::InvalidRequest(
                "model credential row revision must be at least 1".to_owned(),
            ));
        }
        self.binding.validate(self.credential_id)?;
        match self.state {
            ModelCredentialRecordState::Unconfigured
                if self.last_committed_credential_revision.is_some() =>
            {
                Err(PersistenceError::InvalidRequest(
                    "unconfigured credential record has a committed revision".to_owned(),
                ))
            }
            ModelCredentialRecordState::Configured | ModelCredentialRecordState::Cleared
                if self.last_committed_credential_revision.is_none() =>
            {
                Err(PersistenceError::InvalidRequest(
                    "configured or cleared credential record lacks a committed revision".to_owned(),
                ))
            }
            _ => Ok(()),
        }
    }

    pub fn renderer_status(&self) -> Result<EndpointCredentialStatus, PersistenceError> {
        let state = self.state.renderer_state().ok_or(PersistenceError::CredentialUnavailable)?;
        let active_credential_revision = (self.state == ModelCredentialRecordState::Configured)
            .then_some(self.last_committed_credential_revision)
            .flatten();
        let status = EndpointCredentialStatus {
            endpoint_id: self.endpoint_id,
            slot_name: self.credential_slot,
            credential_id: self.credential_id,
            state,
            active_credential_revision,
            last_committed_credential_revision: self.last_committed_credential_revision,
            row_revision: self.row_revision,
            error_code: self.error_code.clone(),
        };
        status.validate().map_err(|error| {
            PersistenceError::InvalidRequest(format!(
                "persisted model credential status is invalid: {error}"
            ))
        })?;
        Ok(status)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateModelCredentialSlot {
    pub credential_id: CredentialId,
    pub endpoint_id: EndpointId,
    pub binding: ModelCredentialStorageBinding,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrepareModelCredentialWrite {
    pub command_invocation_id: CommandInvocationId,
    pub endpoint_id: EndpointId,
    pub credential_id: CredentialId,
    pub credential_slot: ModelCredentialSlot,
    pub expected_row_revision: ExpectedRevision,
    pub intent: ModelCredentialWriteIntent,
    pub binding: ModelCredentialStorageBinding,
    pub models_revision: Revision,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelCredentialWriteIntent {
    Configure,
    Replace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrepareModelCredentialClear {
    pub command_invocation_id: CommandInvocationId,
    pub endpoint_id: EndpointId,
    pub credential_id: CredentialId,
    pub credential_slot: ModelCredentialSlot,
    pub expected_row_revision: ExpectedRevision,
    pub models_revision: Revision,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrepareModelCredentialClearResult {
    NoChange(EndpointCredentialStatus),
    Prepared(Box<CredentialMutationJournalRecord>),
    Existing(Box<CredentialMutationJournalRecord>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrepareModelCredentialWriteResult {
    Prepared(Box<CredentialMutationJournalRecord>),
    Existing(Box<CredentialMutationJournalRecord>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialMutationJournalRecord {
    pub journal_id: MutationJournalId,
    pub journal_kind: CredentialMutationJournalKind,
    pub state: CredentialMutationJournalState,
    pub command_invocation_id: CommandInvocationId,
    pub credential_id: CredentialId,
    pub reserved_credential_revision: Option<CredentialRevision>,
    pub source_row_revision: Revision,
    pub target_row_revision: Revision,
    pub binding: ModelCredentialStorageBinding,
    pub event_id: Option<EventId>,
    pub parent_journal_id: Option<MutationJournalId>,
    pub former_owner: Option<ModelCredentialFormerOwner>,
    pub deletion_reason: Option<ModelCredentialDeletionReason>,
    pub correlation_id: CorrelationId,
    pub models_revision: Revision,
    pub owner_runtime_instance_id: aura_domain_common::RuntimeInstanceId,
    pub lease_epoch: u64,
    pub error_code: Option<ErrorCode>,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCredentialFormerOwner {
    pub endpoint_id: EndpointId,
    pub credential_slot: ModelCredentialSlot,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCredentialDeletionReason {
    EndpointDeleted,
    ProviderDeleted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelCredentialTombstone {
    pub credential_id: CredentialId,
    pub last_committed_credential_revision: Option<CredentialRevision>,
    pub former_owner: ModelCredentialFormerOwner,
    pub deletion_reason: ModelCredentialDeletionReason,
    pub parent_journal_id: MutationJournalId,
    pub child_journal_id: MutationJournalId,
    pub deleted_at: UtcTimestamp,
}

pub(crate) struct PrepareNewModelCredentialChild {
    pub journal_id: MutationJournalId,
    pub command_invocation_id: CommandInvocationId,
    pub slot: CreateModelCredentialSlot,
    pub models_revision: Revision,
    pub correlation_id: CorrelationId,
}

pub(crate) struct PrepareModelCredentialDeleteChild {
    pub journal_id: MutationJournalId,
    pub parent_journal_id: MutationJournalId,
    pub command_invocation_id: CommandInvocationId,
    pub credential_id: CredentialId,
    pub deletion_reason: ModelCredentialDeletionReason,
    pub models_revision: Revision,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CredentialMutationJournalPayload {
    command_invocation_id: CommandInvocationId,
    credential_id: CredentialId,
    reserved_credential_revision: Option<CredentialRevision>,
    source_row_revision: Revision,
    target_row_revision: Revision,
    binding: ModelCredentialStorageBinding,
    event_id: Option<EventId>,
    parent_journal_id: Option<MutationJournalId>,
    former_owner: Option<ModelCredentialFormerOwner>,
    deletion_reason: Option<ModelCredentialDeletionReason>,
    correlation_id: CorrelationId,
    models_revision: Revision,
}

impl CredentialMutationJournalRecord {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        if self.source_row_revision.value() == 0
            || self.target_row_revision.value()
                != self.source_row_revision.value().checked_add(1).ok_or_else(|| {
                    PersistenceError::InvalidRequest("credential row revision overflow".to_owned())
                })?
        {
            return Err(PersistenceError::InvalidRequest(
                "credential journal row revisions are invalid".to_owned(),
            ));
        }
        match self.journal_kind {
            CredentialMutationJournalKind::Configure | CredentialMutationJournalKind::Replace
                if self.reserved_credential_revision.is_none() =>
            {
                return Err(PersistenceError::InvalidRequest(
                    "credential write journal lacks its reserved revision".to_owned(),
                ));
            }
            CredentialMutationJournalKind::Clear if self.reserved_credential_revision.is_some() => {
                return Err(PersistenceError::InvalidRequest(
                    "credential clear journal must not reserve a revision".to_owned(),
                ));
            }
            CredentialMutationJournalKind::Delete
                if self.reserved_credential_revision.is_some()
                    || self.event_id.is_some()
                    || self.parent_journal_id.is_none()
                    || self.former_owner.is_none()
                    || self.deletion_reason.is_none() =>
            {
                return Err(PersistenceError::InvalidRequest(
                    "credential delete journal has invalid delete metadata".to_owned(),
                ));
            }
            _ => {}
        }
        if self.journal_kind != CredentialMutationJournalKind::Delete
            && (self.event_id.is_none()
                || self.parent_journal_id.is_some()
                || self.former_owner.is_some()
                || self.deletion_reason.is_some())
        {
            return Err(PersistenceError::InvalidRequest(
                "credential non-delete journal has invalid delete metadata".to_owned(),
            ));
        }
        self.binding.validate(self.credential_id)
    }

    #[must_use]
    pub const fn command_invocation_id(&self) -> CommandInvocationId {
        self.command_invocation_id
    }

    pub fn staging_credential_ref(&self) -> Result<CredentialRef, PersistenceError> {
        if self.journal_kind == CredentialMutationJournalKind::Delete {
            return Err(PersistenceError::CredentialJournalStateConflict);
        }
        format!("wincred://aura/{}/staging/{}", self.binding.bundle_namespace_key, self.journal_id)
            .parse()
            .map_err(|_| {
                PersistenceError::InvalidRequest(
                    "credential journal produced an invalid staging locator".to_owned(),
                )
            })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelTransportCredentialResolution {
    pub credential_id: CredentialId,
    pub credential_revision: CredentialRevision,
    pub credential_ref: CredentialRef,
    pub normalized_origin: NormalizedOrigin,
    pub auth_scheme: EndpointAuthScheme,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelCredentialMetadataCommit {
    pub credential_record: ModelCredentialRecord,
    pub journal: CredentialMutationJournalRecord,
    pub projection_changes: Vec<ProjectionChange>,
    pub projection_outbox_ids: Vec<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelCredentialDeleteMetadataCommit {
    pub tombstone: ModelCredentialTombstone,
    pub journal: CredentialMutationJournalRecord,
}

impl PersistenceCoordinator {
    pub fn create_model_credential_slot(
        &self,
        input: CreateModelCredentialSlot,
    ) -> Result<ModelCredentialRecord, PersistenceError> {
        input.binding.validate(input.credential_id)?;
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let now = UtcTimestamp::now();
            insert_model_credential_slot(&transaction, &input, now)?;
            let record = load_credential_record_by_id(&transaction, input.credential_id)?
                .ok_or(PersistenceError::CredentialRecordNotFound)?;
            transaction.commit()?;
            Ok(record)
        })
    }

    pub fn model_credential_record(
        &self,
        credential_id: CredentialId,
    ) -> Result<Option<ModelCredentialRecord>, PersistenceError> {
        self.with_reader(|connection| load_credential_record_by_id(connection, credential_id))
    }

    pub fn model_credential_record_for_endpoint(
        &self,
        endpoint_id: EndpointId,
        credential_slot: ModelCredentialSlot,
    ) -> Result<Option<ModelCredentialRecord>, PersistenceError> {
        self.with_reader(|connection| {
            load_credential_record_for_endpoint(connection, endpoint_id, credential_slot)
        })
    }

    pub fn prepare_model_credential_write(
        &self,
        input: PrepareModelCredentialWrite,
    ) -> Result<PrepareModelCredentialWriteResult, PersistenceError> {
        input.binding.validate(input.credential_id)?;
        let requested_journal_kind = match input.intent {
            ModelCredentialWriteIntent::Configure => CredentialMutationJournalKind::Configure,
            ModelCredentialWriteIntent::Replace => CredentialMutationJournalKind::Replace,
        };
        if let Some(existing) = self.mutation_journal_by_idempotency_key(
            journal_kind_name(requested_journal_kind),
            &input.command_invocation_id.to_string(),
        )? {
            return decode_credential_journal(existing)
                .map(Box::new)
                .map(PrepareModelCredentialWriteResult::Existing);
        }
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let record = load_credential_record_by_id(&transaction, input.credential_id)?
                .ok_or(PersistenceError::CredentialRecordNotFound)?;
            validate_mutation_subject(
                &record,
                input.endpoint_id,
                input.credential_slot,
                input.expected_row_revision,
            )?;
            ensure_no_credential_lease(&transaction, input.credential_id)?;
            if record.state == ModelCredentialRecordState::ReconciliationRequired {
                return Err(PersistenceError::CredentialStateConflict);
            }

            match (input.intent, record.state) {
                (
                    ModelCredentialWriteIntent::Configure,
                    ModelCredentialRecordState::Unconfigured | ModelCredentialRecordState::Cleared,
                )
                | (ModelCredentialWriteIntent::Replace, ModelCredentialRecordState::Configured) => {
                }
                _ => return Err(PersistenceError::CredentialStateConflict),
            }

            let reserved_credential_revision = match record.last_committed_credential_revision {
                Some(revision) => revision.next().map_err(|_| {
                    PersistenceError::InvalidRequest("credential revision overflow".to_owned())
                })?,
                None => CredentialRevision::INITIAL,
            };
            let journal = insert_prepared_journal(
                &transaction,
                InsertPreparedCredentialJournal {
                    journal_kind: requested_journal_kind,
                    reserved_credential_revision: Some(reserved_credential_revision),
                    record: &record,
                    binding: &input.binding,
                    models_revision: input.models_revision,
                    correlation_id: input.correlation_id,
                    command_invocation_id: input.command_invocation_id,
                    runtime_instance_id,
                },
            )?;
            transaction.commit()?;
            Ok(PrepareModelCredentialWriteResult::Prepared(Box::new(journal)))
        })
    }

    pub fn prepare_model_credential_clear(
        &self,
        input: PrepareModelCredentialClear,
    ) -> Result<PrepareModelCredentialClearResult, PersistenceError> {
        if let Some(existing) = self.mutation_journal_by_idempotency_key(
            journal_kind_name(CredentialMutationJournalKind::Clear),
            &input.command_invocation_id.to_string(),
        )? {
            return decode_credential_journal(existing)
                .map(Box::new)
                .map(PrepareModelCredentialClearResult::Existing);
        }
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let record = load_credential_record_by_id(&transaction, input.credential_id)?
                .ok_or(PersistenceError::CredentialRecordNotFound)?;
            validate_mutation_subject(
                &record,
                input.endpoint_id,
                input.credential_slot,
                input.expected_row_revision,
            )?;
            ensure_no_credential_lease(&transaction, input.credential_id)?;
            if matches!(
                record.state,
                ModelCredentialRecordState::Unconfigured | ModelCredentialRecordState::Cleared
            ) {
                return Ok(PrepareModelCredentialClearResult::NoChange(record.renderer_status()?));
            }
            if record.state == ModelCredentialRecordState::ReconciliationRequired
                || record.last_committed_credential_revision.is_none()
            {
                return Err(PersistenceError::CredentialStateConflict);
            }
            let journal = insert_prepared_journal(
                &transaction,
                InsertPreparedCredentialJournal {
                    journal_kind: CredentialMutationJournalKind::Clear,
                    reserved_credential_revision: None,
                    record: &record,
                    binding: &record.binding,
                    models_revision: input.models_revision,
                    correlation_id: input.correlation_id,
                    command_invocation_id: input.command_invocation_id,
                    runtime_instance_id,
                },
            )?;
            transaction.commit()?;
            Ok(PrepareModelCredentialClearResult::Prepared(Box::new(journal)))
        })
    }

    pub fn credential_mutation_journal(
        &self,
        journal_id: MutationJournalId,
    ) -> Result<Option<CredentialMutationJournalRecord>, PersistenceError> {
        self.with_reader(|connection| load_credential_journal(connection, journal_id))
    }

    pub fn incomplete_credential_mutation_journals(
        &self,
    ) -> Result<Vec<CredentialMutationJournalRecord>, PersistenceError> {
        self.with_reader(load_incomplete_credential_journals)
    }

    pub fn adopt_credential_mutation_journal(
        &self,
        journal_id: MutationJournalId,
    ) -> Result<CredentialMutationJournalRecord, PersistenceError> {
        let existing = self
            .credential_mutation_journal(journal_id)?
            .ok_or(PersistenceError::CredentialJournalNotFound)?;
        let adopted = self.adopt_mutation_journal_lease(
            journal_id,
            journal_kind_name(existing.journal_kind),
            CREDENTIAL_RESOURCE_KIND,
            existing.credential_id.to_string(),
        )?;
        decode_credential_journal(adopted)
    }

    pub fn credential_reconciliation_status(
        &self,
        journal_id: MutationJournalId,
        error_code: Option<NormalizedIssueCode>,
    ) -> Result<EndpointCredentialStatus, PersistenceError> {
        self.with_reader(|connection| {
            let journal = load_credential_journal(connection, journal_id)?
                .ok_or(PersistenceError::CredentialJournalNotFound)?;
            if matches!(
                journal.state,
                CredentialMutationJournalState::Completed
                    | CredentialMutationJournalState::Aborted
                    | CredentialMutationJournalState::ReconciliationRequired
            ) {
                return Err(PersistenceError::CredentialJournalStateConflict);
            }
            let record = load_credential_record_by_id(connection, journal.credential_id)?
                .ok_or(PersistenceError::CredentialRecordNotFound)?;
            let row_revision =
                record.row_revision.value().checked_add(1).map(Revision::new).ok_or_else(|| {
                    PersistenceError::InvalidRequest("credential row revision overflow".to_owned())
                })?;
            let status = EndpointCredentialStatus {
                endpoint_id: record.endpoint_id,
                slot_name: record.credential_slot,
                credential_id: record.credential_id,
                state: EndpointCredentialState::ReconciliationRequired,
                active_credential_revision: None,
                last_committed_credential_revision: record.last_committed_credential_revision,
                row_revision,
                error_code,
            };
            status.validate().map_err(|error| {
                PersistenceError::InvalidRequest(format!(
                    "credential reconciliation status is invalid: {error}"
                ))
            })?;
            Ok(status)
        })
    }

    pub fn transition_credential_mutation_journal(
        &self,
        journal_id: MutationJournalId,
        expected_state: CredentialMutationJournalState,
        next_state: CredentialMutationJournalState,
    ) -> Result<CredentialMutationJournalRecord, PersistenceError> {
        if !valid_journal_transition(expected_state, next_state) {
            return Err(PersistenceError::CredentialJournalStateConflict);
        }
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let current = load_credential_journal(&transaction, journal_id)?
                .ok_or(PersistenceError::CredentialJournalNotFound)?;
            if current.state != expected_state {
                return Err(PersistenceError::CredentialJournalStateConflict);
            }
            let changed = transaction.execute(
                "UPDATE mutation_journals
                 SET state=?1, updated_at=?2
                 WHERE journal_id=?3 AND journal_kind=?4 AND state=?5",
                params![
                    journal_state_name(next_state),
                    UtcTimestamp::now().to_string(),
                    journal_id.to_string(),
                    journal_kind_name(current.journal_kind),
                    journal_state_name(expected_state),
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::CredentialJournalStateConflict);
            }
            if matches!(
                next_state,
                CredentialMutationJournalState::Completed | CredentialMutationJournalState::Aborted
            ) {
                transaction.execute(
                    "DELETE FROM mutation_leases WHERE journal_id=?1",
                    [journal_id.to_string()],
                )?;
            }
            let journal = load_credential_journal(&transaction, journal_id)?
                .ok_or(PersistenceError::CredentialJournalNotFound)?;
            transaction.commit()?;
            Ok(journal)
        })
    }

    pub fn commit_credential_mutation_metadata(
        &self,
        journal_id: MutationJournalId,
        projection_drafts: Vec<ModelProjectionDraft>,
    ) -> Result<ModelCredentialMetadataCommit, PersistenceError> {
        if projection_drafts.is_empty() {
            return Err(PersistenceError::InvalidRequest(
                "credential metadata commit requires at least one projection".to_owned(),
            ));
        }
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let journal = load_credential_journal(&transaction, journal_id)?
                .ok_or(PersistenceError::CredentialJournalNotFound)?;
            if journal.state != CredentialMutationJournalState::FinalCredentialWritten {
                return Err(PersistenceError::CredentialJournalStateConflict);
            }
            ensure_journal_lease(&transaction, &journal)?;
            let record = load_credential_record_by_id(&transaction, journal.credential_id)?
                .ok_or(PersistenceError::CredentialRecordNotFound)?;
            if record.row_revision != journal.source_row_revision {
                return Err(PersistenceError::CredentialRowRevisionConflict);
            }

            let now = UtcTimestamp::now();
            let event = match journal.journal_kind {
                CredentialMutationJournalKind::Configure
                | CredentialMutationJournalKind::Replace => {
                    let credential_revision = journal
                        .reserved_credential_revision
                        .ok_or(PersistenceError::CredentialJournalStateConflict)?;
                    transaction.execute(
                        "UPDATE credential_records
                         SET state='configured', row_revision=?1,
                             last_committed_credential_revision=?2,
                             credential_ref=?3, credential_binding_fingerprint=?4,
                             bundle_namespace_key=?5, backend_kind=?6, wincred_type=?7,
                             final_target=?8, normalized_origin_json=?9, auth_scheme=?10,
                             error_code=NULL, configured_at=?11, updated_at=?11
                         WHERE credential_id=?12 AND row_revision=?13",
                        params![
                            to_sql_u64(journal.target_row_revision.value())?,
                            to_sql_u64(credential_revision.value())?,
                            journal.binding.credential_ref.to_string(),
                            serde_json::to_string(&journal.binding.credential_binding_fingerprint)?,
                            journal.binding.bundle_namespace_key,
                            journal.binding.backend_kind,
                            journal.binding.wincred_type,
                            journal.binding.final_target,
                            serde_json::to_string(&journal.binding.normalized_origin)?,
                            auth_scheme_name(journal.binding.auth_scheme),
                            now.to_string(),
                            journal.credential_id.to_string(),
                            to_sql_u64(journal.source_row_revision.value())?,
                        ],
                    )?;
                    let payload = ModelEndpointCredentialConfiguredPayload {
                        endpoint_id: record.endpoint_id,
                        credential_id: journal.credential_id,
                        active_credential_revision: credential_revision,
                        row_revision: journal.target_row_revision,
                        credential_binding_fingerprint: journal
                            .binding
                            .credential_binding_fingerprint
                            .clone(),
                        models_revision: journal.models_revision,
                    };
                    credential_event(
                        &journal,
                        "ModelEndpointCredentialConfigured",
                        serde_json::to_value(payload)?,
                        now,
                    )
                }
                CredentialMutationJournalKind::Clear => {
                    let last_committed_credential_revision = record
                        .last_committed_credential_revision
                        .ok_or(PersistenceError::CredentialStateConflict)?;
                    transaction.execute(
                        "UPDATE credential_records
                         SET state='cleared', row_revision=?1, error_code=NULL,
                             configured_at=NULL, updated_at=?2
                         WHERE credential_id=?3 AND row_revision=?4",
                        params![
                            to_sql_u64(journal.target_row_revision.value())?,
                            now.to_string(),
                            journal.credential_id.to_string(),
                            to_sql_u64(journal.source_row_revision.value())?,
                        ],
                    )?;
                    let payload = ModelEndpointCredentialClearedPayload {
                        endpoint_id: record.endpoint_id,
                        credential_id: journal.credential_id,
                        last_committed_credential_revision,
                        row_revision: journal.target_row_revision,
                        previous_credential_binding_fingerprint: record
                            .binding
                            .credential_binding_fingerprint,
                        models_revision: journal.models_revision,
                    };
                    credential_event(
                        &journal,
                        "ModelEndpointCredentialCleared",
                        serde_json::to_value(payload)?,
                        now,
                    )
                }
                CredentialMutationJournalKind::Delete => {
                    return Err(PersistenceError::CredentialJournalStateConflict);
                }
            };
            insert_event(&transaction, &event)?;

            let credential_record =
                load_credential_record_by_id(&transaction, journal.credential_id)?
                    .ok_or(PersistenceError::CredentialRecordNotFound)?;
            if credential_record.row_revision != journal.target_row_revision {
                return Err(PersistenceError::CredentialRowRevisionConflict);
            }
            let credential_status = credential_record.renderer_status()?;
            let (projection_changes, projection_outbox_ids) = insert_credential_projections(
                &transaction,
                runtime_instance_id,
                record.endpoint_id,
                &credential_status,
                journal.models_revision,
                projection_drafts,
            )?;
            let changed = transaction.execute(
                "UPDATE mutation_journals
                 SET state='metadata_committed', updated_at=?1
                 WHERE journal_id=?2 AND journal_kind=?3
                   AND state='final_credential_written'",
                params![
                    now.to_string(),
                    journal_id.to_string(),
                    journal_kind_name(journal.journal_kind),
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::CredentialJournalStateConflict);
            }
            let journal = load_credential_journal(&transaction, journal_id)?
                .ok_or(PersistenceError::CredentialJournalNotFound)?;
            transaction.commit()?;
            Ok(ModelCredentialMetadataCommit {
                credential_record,
                journal,
                projection_changes,
                projection_outbox_ids,
            })
        })
    }

    pub fn mark_credential_reconciliation_required(
        &self,
        journal_id: MutationJournalId,
        expected_state: CredentialMutationJournalState,
        error_code: Option<NormalizedIssueCode>,
        projection_drafts: Vec<ModelProjectionDraft>,
    ) -> Result<ModelCredentialMetadataCommit, PersistenceError> {
        if projection_drafts.is_empty()
            || matches!(
                expected_state,
                CredentialMutationJournalState::Completed
                    | CredentialMutationJournalState::Aborted
                    | CredentialMutationJournalState::ReconciliationRequired
            )
        {
            return Err(PersistenceError::InvalidRequest(
                "credential reconciliation requires an active Journal and projections".to_owned(),
            ));
        }
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let journal = load_credential_journal(&transaction, journal_id)?
                .ok_or(PersistenceError::CredentialJournalNotFound)?;
            if journal.state != expected_state {
                return Err(PersistenceError::CredentialJournalStateConflict);
            }
            ensure_journal_lease(&transaction, &journal)?;
            let record = load_credential_record_by_id(&transaction, journal.credential_id)?
                .ok_or(PersistenceError::CredentialRecordNotFound)?;
            let target_row_revision =
                record.row_revision.value().checked_add(1).map(Revision::new).ok_or_else(|| {
                    PersistenceError::InvalidRequest("credential row revision overflow".to_owned())
                })?;
            let now = UtcTimestamp::now();
            let changed = transaction.execute(
                "UPDATE credential_records
                 SET state='reconciliation_required', row_revision=?1,
                     error_code=?2, configured_at=NULL, updated_at=?3
                 WHERE credential_id=?4 AND row_revision=?5",
                params![
                    to_sql_u64(target_row_revision.value())?,
                    error_code.as_ref().map(ToString::to_string),
                    now.to_string(),
                    journal.credential_id.to_string(),
                    to_sql_u64(record.row_revision.value())?,
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::CredentialRowRevisionConflict);
            }
            let changed = transaction.execute(
                "UPDATE mutation_journals
                 SET state='reconciliation_required', error_code=?1, updated_at=?2
                 WHERE journal_id=?3 AND journal_kind=?4 AND state=?5",
                params![
                    error_code.as_ref().map(ToString::to_string),
                    now.to_string(),
                    journal_id.to_string(),
                    journal_kind_name(journal.journal_kind),
                    journal_state_name(expected_state),
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::CredentialJournalStateConflict);
            }
            let credential_record =
                load_credential_record_by_id(&transaction, journal.credential_id)?
                    .ok_or(PersistenceError::CredentialRecordNotFound)?;
            let credential_status = credential_record.renderer_status()?;
            let (projection_changes, projection_outbox_ids) = insert_credential_projections(
                &transaction,
                runtime_instance_id,
                record.endpoint_id,
                &credential_status,
                journal.models_revision,
                projection_drafts,
            )?;
            transaction.execute(
                "DELETE FROM mutation_leases WHERE journal_id=?1",
                [journal_id.to_string()],
            )?;
            let journal = load_credential_journal(&transaction, journal_id)?
                .ok_or(PersistenceError::CredentialJournalNotFound)?;
            transaction.commit()?;
            Ok(ModelCredentialMetadataCommit {
                credential_record,
                journal,
                projection_changes,
                projection_outbox_ids,
            })
        })
    }

    pub fn credential_for_model_transport(
        &self,
        credential_id: CredentialId,
        credential_revision: CredentialRevision,
        expected_binding_fingerprint: &CredentialBindingFingerprint,
    ) -> Result<ModelTransportCredentialResolution, PersistenceError> {
        self.with_reader(|connection| {
            ensure_no_credential_lease(connection, credential_id)?;
            let record = load_credential_record_by_id(connection, credential_id)?
                .ok_or(PersistenceError::CredentialRecordNotFound)?;
            if record.state != ModelCredentialRecordState::Configured
                || record.last_committed_credential_revision != Some(credential_revision)
            {
                return Err(PersistenceError::CredentialUnavailable);
            }
            if &record.binding.credential_binding_fingerprint != expected_binding_fingerprint {
                return Err(PersistenceError::CredentialBindingMismatch);
            }
            Ok(ModelTransportCredentialResolution {
                credential_id,
                credential_revision,
                credential_ref: record.binding.credential_ref,
                normalized_origin: record.binding.normalized_origin,
                auth_scheme: record.binding.auth_scheme,
            })
        })
    }

    pub fn commit_model_credential_delete_metadata(
        &self,
        journal_id: MutationJournalId,
    ) -> Result<ModelCredentialDeleteMetadataCommit, PersistenceError> {
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let journal = load_credential_journal(&transaction, journal_id)?
                .ok_or(PersistenceError::CredentialJournalNotFound)?;
            let tombstone = commit_model_credential_delete_tombstone(
                &transaction,
                &journal,
                UtcTimestamp::now(),
            )?;
            let journal = load_credential_journal(&transaction, journal_id)?
                .ok_or(PersistenceError::CredentialJournalNotFound)?;
            transaction.commit()?;
            Ok(ModelCredentialDeleteMetadataCommit { tombstone, journal })
        })
    }

    pub fn model_credential_tombstone(
        &self,
        credential_id: CredentialId,
    ) -> Result<Option<ModelCredentialTombstone>, PersistenceError> {
        self.with_reader(|connection| load_credential_tombstone(connection, credential_id))
    }
}

pub(crate) fn insert_model_credential_slot(
    transaction: &Transaction<'_>,
    input: &CreateModelCredentialSlot,
    now: UtcTimestamp,
) -> Result<(), PersistenceError> {
    input.binding.validate(input.credential_id)?;
    transaction
        .execute(
            "INSERT INTO credential_records(
                credential_id, endpoint_id, credential_slot, state, row_revision,
                last_committed_credential_revision, credential_ref,
                credential_binding_fingerprint, bundle_namespace_key, backend_kind,
                wincred_type, final_target, normalized_origin_json, auth_scheme,
                error_code, configured_at, created_at, updated_at
             ) VALUES (
                ?1, ?2, ?3, 'unconfigured', 1, NULL, ?4, ?5, ?6, ?7, ?8,
                ?9, ?10, ?11, NULL, NULL, ?12, ?12
             )",
            params![
                input.credential_id.to_string(),
                input.endpoint_id.to_string(),
                credential_slot_name(input.binding.credential_slot),
                input.binding.credential_ref.to_string(),
                serde_json::to_string(&input.binding.credential_binding_fingerprint)?,
                input.binding.bundle_namespace_key,
                input.binding.backend_kind,
                input.binding.wincred_type,
                input.binding.final_target,
                serde_json::to_string(&input.binding.normalized_origin)?,
                auth_scheme_name(input.binding.auth_scheme),
                now.to_string(),
            ],
        )
        .map_err(map_slot_insert_error)?;
    Ok(())
}

pub(crate) fn insert_prepared_new_model_credential_child(
    connection: &Connection,
    runtime_instance_id: aura_domain_common::RuntimeInstanceId,
    input: PrepareNewModelCredentialChild,
) -> Result<CredentialMutationJournalRecord, PersistenceError> {
    input.slot.binding.validate(input.slot.credential_id)?;
    if load_credential_record_by_id(connection, input.slot.credential_id)?.is_some() {
        return Err(PersistenceError::CredentialStateConflict);
    }
    ensure_no_credential_lease(connection, input.slot.credential_id)?;
    let payload = CredentialMutationJournalPayload {
        command_invocation_id: input.command_invocation_id,
        credential_id: input.slot.credential_id,
        reserved_credential_revision: Some(CredentialRevision::INITIAL),
        source_row_revision: Revision::INITIAL,
        target_row_revision: Revision::new(2),
        binding: input.slot.binding,
        event_id: Some(EventId::new()),
        parent_journal_id: None,
        former_owner: None,
        deletion_reason: None,
        correlation_id: input.correlation_id,
        models_revision: input.models_revision,
    };
    let generic = insert_prepared_mutation_journal(
        connection,
        runtime_instance_id,
        &PrepareMutationJournal {
            journal_id: input.journal_id,
            journal_kind: journal_kind_name(CredentialMutationJournalKind::Configure).to_owned(),
            idempotency_key: Some(input.command_invocation_id.to_string()),
            initial_state: journal_state_name(CredentialMutationJournalState::Prepared).to_owned(),
            payload: serde_json::to_value(payload)?,
            resource_kind: CREDENTIAL_RESOURCE_KIND.to_owned(),
            resource_id: input.slot.credential_id.to_string(),
        },
    )?;
    decode_credential_journal(generic)
}

pub(crate) fn insert_prepared_model_credential_delete_child(
    connection: &Connection,
    runtime_instance_id: aura_domain_common::RuntimeInstanceId,
    input: PrepareModelCredentialDeleteChild,
) -> Result<CredentialMutationJournalRecord, PersistenceError> {
    let record = load_credential_record_by_id(connection, input.credential_id)?
        .ok_or(PersistenceError::CredentialRecordNotFound)?;
    ensure_no_credential_lease(connection, input.credential_id)?;
    if record.state == ModelCredentialRecordState::DeletionPending {
        return Err(PersistenceError::CredentialStateConflict);
    }
    let target_row_revision =
        record.row_revision.value().checked_add(1).map(Revision::new).ok_or_else(|| {
            PersistenceError::InvalidRequest("credential row revision overflow".to_owned())
        })?;
    let payload = CredentialMutationJournalPayload {
        command_invocation_id: input.command_invocation_id,
        credential_id: input.credential_id,
        reserved_credential_revision: None,
        source_row_revision: record.row_revision,
        target_row_revision,
        binding: record.binding.clone(),
        event_id: None,
        parent_journal_id: Some(input.parent_journal_id),
        former_owner: Some(ModelCredentialFormerOwner {
            endpoint_id: record.endpoint_id,
            credential_slot: record.credential_slot,
        }),
        deletion_reason: Some(input.deletion_reason),
        correlation_id: input.correlation_id,
        models_revision: input.models_revision,
    };
    let generic = insert_prepared_mutation_journal(
        connection,
        runtime_instance_id,
        &PrepareMutationJournal {
            journal_id: input.journal_id,
            journal_kind: journal_kind_name(CredentialMutationJournalKind::Delete).to_owned(),
            idempotency_key: Some(format!(
                "{}:{}",
                input.command_invocation_id, input.credential_id
            )),
            initial_state: journal_state_name(CredentialMutationJournalState::Prepared).to_owned(),
            payload: serde_json::to_value(payload)?,
            resource_kind: CREDENTIAL_RESOURCE_KIND.to_owned(),
            resource_id: input.credential_id.to_string(),
        },
    )?;
    decode_credential_journal(generic)
}

pub(crate) fn commit_new_configured_model_credential_slot(
    transaction: &Transaction<'_>,
    slot: &CreateModelCredentialSlot,
    journal: &CredentialMutationJournalRecord,
    atomic_group_id: &str,
    now: UtcTimestamp,
) -> Result<(ModelCredentialRecord, PersistentEventDraft), PersistenceError> {
    if journal.journal_kind != CredentialMutationJournalKind::Configure
        || journal.state != CredentialMutationJournalState::FinalCredentialWritten
        || journal.credential_id != slot.credential_id
        || journal.source_row_revision != Revision::INITIAL
        || journal.target_row_revision != Revision::new(2)
        || journal.reserved_credential_revision != Some(CredentialRevision::INITIAL)
        || journal.binding != slot.binding
    {
        return Err(PersistenceError::CredentialJournalStateConflict);
    }
    ensure_journal_lease(transaction, journal)?;
    insert_model_credential_slot(transaction, slot, now)?;
    let changed = transaction.execute(
        "UPDATE credential_records
         SET state='configured', row_revision=?1,
             last_committed_credential_revision=?2,
             error_code=NULL, configured_at=?3, updated_at=?3
         WHERE credential_id=?4 AND row_revision=1 AND state='unconfigured'",
        params![
            to_sql_u64(journal.target_row_revision.value())?,
            to_sql_u64(CredentialRevision::INITIAL.value())?,
            now.to_string(),
            journal.credential_id.to_string(),
        ],
    )?;
    if changed != 1 {
        return Err(PersistenceError::CredentialRowRevisionConflict);
    }
    let record = load_credential_record_by_id(transaction, journal.credential_id)?
        .ok_or(PersistenceError::CredentialRecordNotFound)?;
    let payload = ModelEndpointCredentialConfiguredPayload {
        endpoint_id: slot.endpoint_id,
        credential_id: journal.credential_id,
        active_credential_revision: CredentialRevision::INITIAL,
        row_revision: journal.target_row_revision,
        credential_binding_fingerprint: journal.binding.credential_binding_fingerprint.clone(),
        models_revision: journal.models_revision,
    };
    let mut event = credential_event(
        journal,
        "ModelEndpointCredentialConfigured",
        serde_json::to_value(payload)?,
        now,
    );
    event.atomic_group_id = Some(atomic_group_id.to_owned());
    Ok((record, event))
}

pub(crate) fn mark_model_credential_delete_pending(
    transaction: &Transaction<'_>,
    journal: &CredentialMutationJournalRecord,
    now: UtcTimestamp,
) -> Result<ModelCredentialRecord, PersistenceError> {
    if journal.journal_kind != CredentialMutationJournalKind::Delete
        || journal.state != CredentialMutationJournalState::Prepared
    {
        return Err(PersistenceError::CredentialJournalStateConflict);
    }
    ensure_journal_lease(transaction, journal)?;
    let former_owner =
        journal.former_owner.ok_or(PersistenceError::CredentialJournalStateConflict)?;
    let record = load_credential_record_by_id(transaction, journal.credential_id)?
        .ok_or(PersistenceError::CredentialRecordNotFound)?;
    if record.endpoint_id != former_owner.endpoint_id
        || record.credential_slot != former_owner.credential_slot
        || record.row_revision != journal.source_row_revision
        || record.binding != journal.binding
        || record.state == ModelCredentialRecordState::DeletionPending
    {
        return Err(PersistenceError::CredentialBindingMismatch);
    }
    let record_changed = transaction.execute(
        "UPDATE credential_records
         SET state='deletion_pending', row_revision=?1, error_code=NULL, updated_at=?2
         WHERE credential_id=?3 AND row_revision=?4 AND state<>'deletion_pending'",
        params![
            to_sql_u64(journal.target_row_revision.value())?,
            now.to_string(),
            journal.credential_id.to_string(),
            to_sql_u64(journal.source_row_revision.value())?,
        ],
    )?;
    let journal_changed = transaction.execute(
        "UPDATE mutation_journals
         SET state='staged', error_code=NULL, updated_at=?1
         WHERE journal_id=?2 AND journal_kind='credential_delete' AND state='prepared'",
        params![now.to_string(), journal.journal_id.to_string()],
    )?;
    if record_changed != 1 || journal_changed != 1 {
        return Err(PersistenceError::CredentialJournalStateConflict);
    }
    load_credential_record_by_id(transaction, journal.credential_id)?
        .ok_or(PersistenceError::CredentialRecordNotFound)
}

pub(crate) fn commit_model_credential_delete_tombstone(
    transaction: &Transaction<'_>,
    journal: &CredentialMutationJournalRecord,
    now: UtcTimestamp,
) -> Result<ModelCredentialTombstone, PersistenceError> {
    if journal.journal_kind != CredentialMutationJournalKind::Delete
        || journal.state != CredentialMutationJournalState::FinalCredentialWritten
    {
        return Err(PersistenceError::CredentialJournalStateConflict);
    }
    ensure_journal_lease(transaction, journal)?;
    let parent_journal_id =
        journal.parent_journal_id.ok_or(PersistenceError::CredentialJournalStateConflict)?;
    let former_owner =
        journal.former_owner.ok_or(PersistenceError::CredentialJournalStateConflict)?;
    let deletion_reason =
        journal.deletion_reason.ok_or(PersistenceError::CredentialJournalStateConflict)?;
    let record = load_credential_record_by_id(transaction, journal.credential_id)?
        .ok_or(PersistenceError::CredentialRecordNotFound)?;
    if record.state != ModelCredentialRecordState::DeletionPending
        || record.row_revision != journal.target_row_revision
        || record.endpoint_id != former_owner.endpoint_id
        || record.credential_slot != former_owner.credential_slot
        || record.binding != journal.binding
    {
        return Err(PersistenceError::CredentialBindingMismatch);
    }
    transaction.execute(
        "INSERT INTO credential_tombstones(
            credential_id, last_committed_credential_revision, former_owner_json,
            deletion_reason, parent_journal_id, child_journal_id, deleted_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            record.credential_id.to_string(),
            record
                .last_committed_credential_revision
                .map(|revision| to_sql_u64(revision.value()))
                .transpose()?,
            serde_json::to_string(&former_owner)?,
            deletion_reason_name(deletion_reason),
            parent_journal_id.to_string(),
            journal.journal_id.to_string(),
            now.to_string(),
        ],
    )?;
    let deleted =
        transaction.execute(
            "DELETE FROM credential_records
         WHERE credential_id=?1 AND row_revision=?2 AND state='deletion_pending'",
            params![
                record.credential_id.to_string(),
                to_sql_u64(journal.target_row_revision.value())?,
            ],
        )?;
    let journal_changed = transaction.execute(
        "UPDATE mutation_journals
         SET state='metadata_committed', error_code=NULL, updated_at=?1
         WHERE journal_id=?2 AND journal_kind='credential_delete'
           AND state='final_credential_written'",
        params![now.to_string(), journal.journal_id.to_string()],
    )?;
    if deleted != 1 || journal_changed != 1 {
        return Err(PersistenceError::CredentialJournalStateConflict);
    }
    Ok(ModelCredentialTombstone {
        credential_id: record.credential_id,
        last_committed_credential_revision: record.last_committed_credential_revision,
        former_owner,
        deletion_reason,
        parent_journal_id,
        child_journal_id: journal.journal_id,
        deleted_at: now,
    })
}

fn validate_mutation_subject(
    record: &ModelCredentialRecord,
    endpoint_id: EndpointId,
    credential_slot: ModelCredentialSlot,
    expected_row_revision: ExpectedRevision,
) -> Result<(), PersistenceError> {
    if record.endpoint_id != endpoint_id || record.credential_slot != credential_slot {
        return Err(PersistenceError::CredentialBindingMismatch);
    }
    if record.row_revision.value() != expected_row_revision.value() {
        return Err(PersistenceError::CredentialRowRevisionConflict);
    }
    Ok(())
}

pub(crate) fn ensure_journal_lease(
    connection: &Connection,
    journal: &CredentialMutationJournalRecord,
) -> Result<(), PersistenceError> {
    let lease = connection
        .query_row(
            "SELECT journal_id, runtime_instance_id, lease_epoch
             FROM mutation_leases
             WHERE resource_kind=?1 AND resource_id=?2",
            params![CREDENTIAL_RESOURCE_KIND, journal.credential_id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?)),
        )
        .optional()?;
    if lease.is_some_and(|(journal_id, runtime_instance_id, lease_epoch)| {
        journal_id == journal.journal_id.to_string()
            && runtime_instance_id == journal.owner_runtime_instance_id.to_string()
            && u64::try_from(lease_epoch).ok() == Some(journal.lease_epoch)
    }) {
        Ok(())
    } else {
        Err(PersistenceError::CredentialMutationLeaseConflict)
    }
}

fn ensure_no_credential_lease(
    connection: &Connection,
    credential_id: CredentialId,
) -> Result<(), PersistenceError> {
    let leased = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM mutation_leases
            WHERE resource_kind=?1 AND resource_id=?2
         )",
        params![CREDENTIAL_RESOURCE_KIND, credential_id.to_string()],
        |row| row.get::<_, bool>(0),
    )?;
    if leased { Err(PersistenceError::CredentialMutationLeaseConflict) } else { Ok(()) }
}

struct InsertPreparedCredentialJournal<'a> {
    journal_kind: CredentialMutationJournalKind,
    reserved_credential_revision: Option<CredentialRevision>,
    record: &'a ModelCredentialRecord,
    binding: &'a ModelCredentialStorageBinding,
    models_revision: Revision,
    correlation_id: CorrelationId,
    command_invocation_id: CommandInvocationId,
    runtime_instance_id: aura_domain_common::RuntimeInstanceId,
}

fn insert_prepared_journal(
    transaction: &Transaction<'_>,
    input: InsertPreparedCredentialJournal<'_>,
) -> Result<CredentialMutationJournalRecord, PersistenceError> {
    let InsertPreparedCredentialJournal {
        journal_kind,
        reserved_credential_revision,
        record,
        binding,
        models_revision,
        correlation_id,
        command_invocation_id,
        runtime_instance_id,
    } = input;
    let journal_id = MutationJournalId::new();
    let event_id = EventId::new();
    let target_row_revision =
        record.row_revision.value().checked_add(1).map(Revision::new).ok_or_else(|| {
            PersistenceError::InvalidRequest("credential row revision overflow".to_owned())
        })?;
    let payload = CredentialMutationJournalPayload {
        command_invocation_id,
        credential_id: record.credential_id,
        reserved_credential_revision,
        source_row_revision: record.row_revision,
        target_row_revision,
        binding: binding.clone(),
        event_id: Some(event_id),
        parent_journal_id: None,
        former_owner: None,
        deletion_reason: None,
        correlation_id,
        models_revision,
    };
    let generic = insert_prepared_mutation_journal(
        transaction,
        runtime_instance_id,
        &PrepareMutationJournal {
            journal_id,
            journal_kind: journal_kind_name(journal_kind).to_owned(),
            idempotency_key: Some(command_invocation_id.to_string()),
            initial_state: "prepared".to_owned(),
            payload: serde_json::to_value(payload)?,
            resource_kind: CREDENTIAL_RESOURCE_KIND.to_owned(),
            resource_id: record.credential_id.to_string(),
        },
    )?;
    decode_credential_journal(generic)
}

fn credential_event(
    journal: &CredentialMutationJournalRecord,
    event_type: &str,
    payload: serde_json::Value,
    created_at: UtcTimestamp,
) -> PersistentEventDraft {
    let mut event = PersistentEventDraft::system(event_type, journal.correlation_id, payload);
    event.event_id =
        journal.event_id.expect("validated non-delete credential Journal has an EventId");
    event.created_at = created_at;
    event
}

fn insert_credential_projections(
    transaction: &Transaction<'_>,
    runtime_instance_id: aura_domain_common::RuntimeInstanceId,
    endpoint_id: EndpointId,
    credential_status: &EndpointCredentialStatus,
    models_revision: Revision,
    projection_drafts: Vec<ModelProjectionDraft>,
) -> Result<(Vec<ProjectionChange>, Vec<i64>), PersistenceError> {
    validate_credential_projection_drafts(
        &projection_drafts,
        endpoint_id,
        credential_status,
        models_revision,
    )?;
    insert_model_projections(transaction, runtime_instance_id, projection_drafts)
}

pub(crate) fn validate_credential_projection_drafts(
    drafts: &[ModelProjectionDraft],
    endpoint_id: EndpointId,
    credential_status: &EndpointCredentialStatus,
    models_revision: Revision,
) -> Result<(), PersistenceError> {
    let mut settings_coverage = 0_u8;
    let mut endpoint_coverage = 0_u8;
    for draft in drafts {
        let valid = match draft {
            ModelProjectionDraft::ModelSettings(payload) => {
                settings_coverage = settings_coverage.saturating_add(1);
                let matching_active = payload
                    .active_credentials
                    .iter()
                    .filter(|status| status.credential_id == credential_status.credential_id)
                    .collect::<Vec<_>>();
                let matching_dormant = payload
                    .dormant_credentials
                    .iter()
                    .filter(|status| status.credential_id == credential_status.credential_id)
                    .count();
                payload.models_revision == models_revision
                    && matching_active.as_slice() == [credential_status]
                    && matching_dormant == 0
            }
            ModelProjectionDraft::EndpointDetail(payload) if payload.endpoint_id == endpoint_id => {
                endpoint_coverage = endpoint_coverage.saturating_add(1);
                payload.endpoint.id == endpoint_id
                    && payload.models_revision == models_revision
                    && payload.authentication.active_credential.as_ref() == Some(credential_status)
                    && payload.authentication.dormant_credential.is_none()
            }
            ModelProjectionDraft::EndpointDetail(payload) => {
                payload
                    .authentication
                    .active_credential
                    .as_ref()
                    .is_none_or(|status| status.credential_id != credential_status.credential_id)
                    && payload.authentication.dormant_credential.as_ref().is_none_or(|status| {
                        status.credential_id != credential_status.credential_id
                    })
            }
            ModelProjectionDraft::ModelProfile(payload) if payload.endpoint.id == endpoint_id => {
                let expected_executable = payload.profile.enabled
                    && payload
                        .profile
                        .effective_max_output_tokens()
                        .is_some_and(|tokens| tokens > 0)
                    && credential_status.state == EndpointCredentialState::Configured;
                payload.models_revision == models_revision
                    && payload.execution_readiness.is_executable == expected_executable
            }
            ModelProjectionDraft::SessionModelSelector(payload) => {
                payload.models.iter().filter(|model| model.endpoint_id == endpoint_id).all(
                    |model| {
                        credential_status.state == EndpointCredentialState::Configured
                            || !model.execution_readiness.is_executable
                    },
                )
            }
            ModelProjectionDraft::ModelProfile(_)
            | ModelProjectionDraft::ProviderCatalog(_)
            | ModelProjectionDraft::ModelAvailabilityTest(_)
            | ModelProjectionDraft::ModelDiscovery(_)
            | ModelProjectionDraft::ContextWindowCorrection(_)
            | ModelProjectionDraft::PromptInspector(_) => true,
        };
        if !valid {
            return Err(PersistenceError::InvalidRequest(
                "credential projection plan conflicts with the committed Renderer-safe status"
                    .to_owned(),
            ));
        }
    }
    if settings_coverage != 1 || endpoint_coverage != 1 {
        return Err(PersistenceError::InvalidRequest(
            "credential projection plan must cover exactly one ModelSettings and owning EndpointDetail projection"
                .to_owned(),
        ));
    }
    Ok(())
}

fn valid_journal_transition(
    current: CredentialMutationJournalState,
    next: CredentialMutationJournalState,
) -> bool {
    matches!(
        (current, next),
        (
            CredentialMutationJournalState::Prepared,
            CredentialMutationJournalState::Staged | CredentialMutationJournalState::Aborted
        ) | (
            CredentialMutationJournalState::Staged,
            CredentialMutationJournalState::FinalCredentialWritten
                | CredentialMutationJournalState::Aborted
        ) | (
            CredentialMutationJournalState::FinalCredentialWritten,
            CredentialMutationJournalState::MetadataCommitted
        ) | (
            CredentialMutationJournalState::MetadataCommitted,
            CredentialMutationJournalState::CleanupPending
        ) | (
            CredentialMutationJournalState::CleanupPending,
            CredentialMutationJournalState::Completed
        )
    )
}

fn load_credential_record_by_id(
    connection: &Connection,
    credential_id: CredentialId,
) -> Result<Option<ModelCredentialRecord>, PersistenceError> {
    load_credential_record(connection, "credential_id=?1", credential_id.to_string())
}

fn load_credential_record_for_endpoint(
    connection: &Connection,
    endpoint_id: EndpointId,
    credential_slot: ModelCredentialSlot,
) -> Result<Option<ModelCredentialRecord>, PersistenceError> {
    let raw = connection
        .query_row(
            "SELECT credential_id, endpoint_id, credential_slot, state, row_revision,
                    last_committed_credential_revision, credential_ref,
                    credential_binding_fingerprint, bundle_namespace_key, backend_kind,
                    wincred_type, final_target, normalized_origin_json, auth_scheme,
                    error_code, configured_at, created_at, updated_at
             FROM credential_records
             WHERE endpoint_id=?1 AND credential_slot=?2",
            params![endpoint_id.to_string(), credential_slot_name(credential_slot)],
            credential_record_row,
        )
        .optional()?;
    raw.map(decode_credential_record).transpose()
}

fn load_credential_record(
    connection: &Connection,
    predicate: &str,
    value: String,
) -> Result<Option<ModelCredentialRecord>, PersistenceError> {
    let sql = format!(
        "SELECT credential_id, endpoint_id, credential_slot, state, row_revision,
                last_committed_credential_revision, credential_ref,
                credential_binding_fingerprint, bundle_namespace_key, backend_kind,
                wincred_type, final_target, normalized_origin_json, auth_scheme,
                error_code, configured_at, created_at, updated_at
         FROM credential_records WHERE {predicate}"
    );
    let raw = connection.query_row(&sql, [value], credential_record_row).optional()?;
    raw.map(decode_credential_record).transpose()
}

fn load_credential_tombstone(
    connection: &Connection,
    credential_id: CredentialId,
) -> Result<Option<ModelCredentialTombstone>, PersistenceError> {
    let raw = connection
        .query_row(
            "SELECT last_committed_credential_revision, former_owner_json,
                    deletion_reason, parent_journal_id, child_journal_id, deleted_at
             FROM credential_tombstones WHERE credential_id=?1",
            [credential_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .optional()?;
    raw.map(
        |(
            last_committed_credential_revision,
            former_owner_json,
            deletion_reason,
            parent_journal_id,
            child_journal_id,
            deleted_at,
        )| {
            Ok(ModelCredentialTombstone {
                credential_id,
                last_committed_credential_revision: last_committed_credential_revision
                    .map(|value| {
                        CredentialRevision::new(Revision::new(from_sql_u64(
                            value,
                            "credential tombstone revision",
                        )?))
                        .map_err(|error| PersistenceError::InvalidRequest(error.to_string()))
                    })
                    .transpose()?,
                former_owner: serde_json::from_str(&former_owner_json)?,
                deletion_reason: parse_deletion_reason(&deletion_reason)?,
                parent_journal_id: MutationJournalId::from_str(&parent_journal_id)?,
                child_journal_id: MutationJournalId::from_str(&child_journal_id)?,
                deleted_at: parse_timestamp(&deleted_at, "credential tombstone deleted_at")?,
            })
        },
    )
    .transpose()
}

type CredentialRecordRow = (
    String,
    String,
    String,
    String,
    i64,
    Option<i64>,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
);

fn credential_record_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CredentialRecordRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
        row.get(12)?,
        row.get(13)?,
        row.get(14)?,
        row.get(15)?,
        row.get(16)?,
        row.get(17)?,
    ))
}

fn decode_credential_record(
    row: CredentialRecordRow,
) -> Result<ModelCredentialRecord, PersistenceError> {
    let (
        credential_id,
        endpoint_id,
        credential_slot,
        state,
        row_revision,
        last_committed_credential_revision,
        credential_ref,
        credential_binding_fingerprint,
        bundle_namespace_key,
        backend_kind,
        wincred_type,
        final_target,
        normalized_origin_json,
        auth_scheme,
        error_code,
        configured_at,
        created_at,
        updated_at,
    ) = row;
    let record = ModelCredentialRecord {
        credential_id: CredentialId::from_str(&credential_id)?,
        endpoint_id: EndpointId::from_str(&endpoint_id)?,
        credential_slot: parse_credential_slot(&credential_slot)?,
        state: parse_credential_record_state(&state)?,
        row_revision: Revision::new(from_sql_u64(row_revision, "credential row revision")?),
        last_committed_credential_revision: last_committed_credential_revision
            .map(|value| {
                CredentialRevision::new(Revision::new(from_sql_u64(value, "credential revision")?))
                    .map_err(|error| PersistenceError::InvalidRequest(error.to_string()))
            })
            .transpose()?,
        binding: ModelCredentialStorageBinding {
            credential_ref: CredentialRef::from_str(&credential_ref).map_err(|_| {
                PersistenceError::InvalidRequest(
                    "persisted model credential reference is invalid".to_owned(),
                )
            })?,
            credential_binding_fingerprint: serde_json::from_str(&credential_binding_fingerprint)?,
            bundle_namespace_key,
            backend_kind,
            wincred_type,
            final_target,
            normalized_origin: serde_json::from_str(&normalized_origin_json)?,
            auth_scheme: parse_auth_scheme(&auth_scheme)?,
            credential_slot: parse_credential_slot(&credential_slot)?,
        },
        error_code: error_code
            .map(|value| {
                NormalizedIssueCode::from_str(&value).map_err(|_| {
                    PersistenceError::InvalidRequest(
                        "persisted model credential error code is invalid".to_owned(),
                    )
                })
            })
            .transpose()?,
        configured_at: configured_at
            .map(|value| parse_timestamp(&value, "credential configured_at"))
            .transpose()?,
        created_at: parse_timestamp(&created_at, "credential created_at")?,
        updated_at: parse_timestamp(&updated_at, "credential updated_at")?,
    };
    record.validate()?;
    Ok(record)
}

pub(crate) fn load_credential_journal(
    connection: &Connection,
    journal_id: MutationJournalId,
) -> Result<Option<CredentialMutationJournalRecord>, PersistenceError> {
    load_mutation_journal(connection, journal_id)?.map(decode_credential_journal).transpose()
}

fn decode_credential_journal(
    record: MutationJournalRecord,
) -> Result<CredentialMutationJournalRecord, PersistenceError> {
    let payload: CredentialMutationJournalPayload = serde_json::from_value(record.payload)?;
    let expected_idempotency_key = if record.journal_kind == "credential_delete" {
        format!("{}:{}", payload.command_invocation_id, payload.credential_id)
    } else {
        payload.command_invocation_id.to_string()
    };
    if record.idempotency_key.as_deref() != Some(expected_idempotency_key.as_str()) {
        return Err(PersistenceError::MutationJournalKindConflict);
    }
    let journal = CredentialMutationJournalRecord {
        journal_id: record.journal_id,
        journal_kind: parse_journal_kind(&record.journal_kind)?,
        state: parse_journal_state(&record.state)?,
        command_invocation_id: payload.command_invocation_id,
        credential_id: payload.credential_id,
        reserved_credential_revision: payload.reserved_credential_revision,
        source_row_revision: payload.source_row_revision,
        target_row_revision: payload.target_row_revision,
        binding: payload.binding,
        event_id: payload.event_id,
        parent_journal_id: payload.parent_journal_id,
        former_owner: payload.former_owner,
        deletion_reason: payload.deletion_reason,
        correlation_id: payload.correlation_id,
        models_revision: payload.models_revision,
        owner_runtime_instance_id: record.owner_runtime_instance_id,
        lease_epoch: record.lease_epoch,
        error_code: record.error_code,
        created_at: record.created_at,
        updated_at: record.updated_at,
    };
    journal.validate()?;
    Ok(journal)
}

fn load_incomplete_credential_journals(
    connection: &Connection,
) -> Result<Vec<CredentialMutationJournalRecord>, PersistenceError> {
    let mut statement = connection.prepare(
        "SELECT journal_id
         FROM mutation_journals
         WHERE journal_kind IN (
            'credential_configure', 'credential_replace', 'credential_clear'
         ) AND state NOT IN ('completed', 'aborted', 'reconciliation_required')
         ORDER BY created_at, journal_id",
    )?;
    let ids =
        statement.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    ids.into_iter()
        .map(|value| {
            let journal_id = MutationJournalId::from_str(&value)?;
            load_credential_journal(connection, journal_id)?
                .ok_or(PersistenceError::CredentialJournalNotFound)
        })
        .collect()
}

fn map_slot_insert_error(error: rusqlite::Error) -> PersistenceError {
    if matches!(
        error,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error { code: rusqlite::ErrorCode::ConstraintViolation, .. },
            _
        )
    ) {
        PersistenceError::CredentialSlotConflict
    } else {
        PersistenceError::Sqlite(error)
    }
}

const fn credential_slot_name(slot: ModelCredentialSlot) -> &'static str {
    match slot {
        ModelCredentialSlot::ApiKey => "api-key",
    }
}

const fn deletion_reason_name(reason: ModelCredentialDeletionReason) -> &'static str {
    match reason {
        ModelCredentialDeletionReason::EndpointDeleted => "endpoint_deleted",
        ModelCredentialDeletionReason::ProviderDeleted => "provider_deleted",
    }
}

fn parse_deletion_reason(value: &str) -> Result<ModelCredentialDeletionReason, PersistenceError> {
    match value {
        "endpoint_deleted" => Ok(ModelCredentialDeletionReason::EndpointDeleted),
        "provider_deleted" => Ok(ModelCredentialDeletionReason::ProviderDeleted),
        _ => Err(PersistenceError::InvalidRequest(
            "persisted credential deletion reason is invalid".to_owned(),
        )),
    }
}

fn parse_credential_slot(value: &str) -> Result<ModelCredentialSlot, PersistenceError> {
    match value {
        "api-key" => Ok(ModelCredentialSlot::ApiKey),
        _ => Err(PersistenceError::InvalidRequest(
            "persisted model credential slot is invalid".to_owned(),
        )),
    }
}

const fn auth_scheme_name(scheme: EndpointAuthScheme) -> &'static str {
    match scheme {
        EndpointAuthScheme::None => "none",
        EndpointAuthScheme::OpenAiBearerApiKey => "open_ai_bearer_api_key",
        EndpointAuthScheme::AnthropicApiKey => "anthropic_api_key",
    }
}

fn parse_auth_scheme(value: &str) -> Result<EndpointAuthScheme, PersistenceError> {
    match value {
        "open_ai_bearer_api_key" => Ok(EndpointAuthScheme::OpenAiBearerApiKey),
        "anthropic_api_key" => Ok(EndpointAuthScheme::AnthropicApiKey),
        _ => Err(PersistenceError::InvalidRequest(
            "persisted model credential auth scheme is invalid".to_owned(),
        )),
    }
}

fn parse_credential_record_state(
    value: &str,
) -> Result<ModelCredentialRecordState, PersistenceError> {
    match value {
        "unconfigured" => Ok(ModelCredentialRecordState::Unconfigured),
        "configured" => Ok(ModelCredentialRecordState::Configured),
        "cleared" => Ok(ModelCredentialRecordState::Cleared),
        "unavailable" => Ok(ModelCredentialRecordState::Unavailable),
        "reconciliation_required" => Ok(ModelCredentialRecordState::ReconciliationRequired),
        "deletion_pending" => Ok(ModelCredentialRecordState::DeletionPending),
        _ => Err(PersistenceError::InvalidRequest(
            "persisted model credential state is invalid".to_owned(),
        )),
    }
}

const fn journal_kind_name(kind: CredentialMutationJournalKind) -> &'static str {
    match kind {
        CredentialMutationJournalKind::Configure => "credential_configure",
        CredentialMutationJournalKind::Replace => "credential_replace",
        CredentialMutationJournalKind::Clear => "credential_clear",
        CredentialMutationJournalKind::Delete => "credential_delete",
    }
}

fn parse_journal_kind(value: &str) -> Result<CredentialMutationJournalKind, PersistenceError> {
    match value {
        "credential_configure" => Ok(CredentialMutationJournalKind::Configure),
        "credential_replace" => Ok(CredentialMutationJournalKind::Replace),
        "credential_clear" => Ok(CredentialMutationJournalKind::Clear),
        "credential_delete" => Ok(CredentialMutationJournalKind::Delete),
        _ => Err(PersistenceError::InvalidRequest(
            "persisted credential journal kind is invalid".to_owned(),
        )),
    }
}

const fn journal_state_name(state: CredentialMutationJournalState) -> &'static str {
    match state {
        CredentialMutationJournalState::Prepared => "prepared",
        CredentialMutationJournalState::Staged => "staged",
        CredentialMutationJournalState::FinalCredentialWritten => "final_credential_written",
        CredentialMutationJournalState::MetadataCommitted => "metadata_committed",
        CredentialMutationJournalState::CleanupPending => "cleanup_pending",
        CredentialMutationJournalState::Completed => "completed",
        CredentialMutationJournalState::Aborted => "aborted",
        CredentialMutationJournalState::ReconciliationRequired => "reconciliation_required",
    }
}

fn parse_journal_state(value: &str) -> Result<CredentialMutationJournalState, PersistenceError> {
    match value {
        "prepared" => Ok(CredentialMutationJournalState::Prepared),
        "staged" => Ok(CredentialMutationJournalState::Staged),
        "final_credential_written" => Ok(CredentialMutationJournalState::FinalCredentialWritten),
        "metadata_committed" => Ok(CredentialMutationJournalState::MetadataCommitted),
        "cleanup_pending" => Ok(CredentialMutationJournalState::CleanupPending),
        "completed" => Ok(CredentialMutationJournalState::Completed),
        "aborted" => Ok(CredentialMutationJournalState::Aborted),
        "reconciliation_required" => Ok(CredentialMutationJournalState::ReconciliationRequired),
        _ => Err(PersistenceError::InvalidRequest(
            "persisted credential journal state is invalid".to_owned(),
        )),
    }
}

fn parse_timestamp(value: &str, field: &str) -> Result<UtcTimestamp, PersistenceError> {
    value
        .parse()
        .map_err(|_| PersistenceError::InvalidRequest(format!("persisted {field} is invalid")))
}

fn from_sql_u64(value: i64, field: &str) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .map_err(|_| PersistenceError::InvalidRequest(format!("persisted {field} is negative")))
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{
        CommandInvocationId, ContentHash, CorrelationId, CredentialId, CredentialRevision,
        EndpointId, ExpectedRevision, Revision, RuntimeInstanceId,
    };
    use aura_model_domain::{
        CredentialBindingFingerprint, EndpointAuthScheme, EndpointCredentialState,
        ModelCredentialSlot,
    };
    use aura_permission_domain::NormalizedOrigin;

    use super::{
        CreateModelCredentialSlot, CredentialMutationJournalKind, CredentialMutationJournalState,
        ModelCredentialRecordState, ModelCredentialStorageBinding, ModelCredentialWriteIntent,
        PrepareModelCredentialClear, PrepareModelCredentialClearResult,
        PrepareModelCredentialWrite, PrepareModelCredentialWriteResult,
    };
    use crate::model_projection_fixture::credential_projection_plan;
    use crate::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};

    fn start(
        database_path: std::path::PathBuf,
        artifact_root: std::path::PathBuf,
    ) -> PersistenceCoordinator {
        match PersistenceCoordinator::start(PersistenceOptions::new(
            database_path,
            artifact_root,
            RuntimeInstanceId::new(),
        ))
        .expect("start persistence")
        {
            PersistenceStartup::Ready(coordinator) => coordinator,
            PersistenceStartup::RecoveryRequired(recovery) => {
                panic!("unexpected recovery requirement: {}", recovery.reason)
            }
        }
    }

    fn binding(credential_id: CredentialId) -> ModelCredentialStorageBinding {
        let bundle_namespace_key = "0123456789abcdef".to_owned();
        let credential_ref =
            format!("wincred://aura/{bundle_namespace_key}/credential/{credential_id}")
                .parse()
                .expect("credential ref");
        let final_target = format!("Aura/{bundle_namespace_key}/credential/{credential_id}");
        ModelCredentialStorageBinding {
            credential_ref,
            credential_binding_fingerprint: CredentialBindingFingerprint(ContentHash::sha256(
                b"frozen credential binding",
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
        }
    }

    fn advance_final_written(
        coordinator: &PersistenceCoordinator,
        journal_id: aura_domain_common::MutationJournalId,
    ) {
        coordinator
            .transition_credential_mutation_journal(
                journal_id,
                CredentialMutationJournalState::Prepared,
                CredentialMutationJournalState::Staged,
            )
            .expect("staged");
        coordinator
            .transition_credential_mutation_journal(
                journal_id,
                CredentialMutationJournalState::Staged,
                CredentialMutationJournalState::FinalCredentialWritten,
            )
            .expect("final written");
    }

    fn finish_cleanup(
        coordinator: &PersistenceCoordinator,
        journal_id: aura_domain_common::MutationJournalId,
    ) {
        coordinator
            .transition_credential_mutation_journal(
                journal_id,
                CredentialMutationJournalState::MetadataCommitted,
                CredentialMutationJournalState::CleanupPending,
            )
            .expect("cleanup pending");
        coordinator
            .transition_credential_mutation_journal(
                journal_id,
                CredentialMutationJournalState::CleanupPending,
                CredentialMutationJournalState::Completed,
            )
            .expect("completed");
    }

    #[test]
    fn j_a_preserves_identity_and_separates_secret_revision_from_row_revision() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database_path = directory.path().join("data/aura.db");
        let coordinator = start(database_path.clone(), directory.path().join("data/artifacts"));
        let endpoint_id = EndpointId::new();
        let credential_id = CredentialId::new();
        let binding = binding(credential_id);
        let created = coordinator
            .create_model_credential_slot(CreateModelCredentialSlot {
                credential_id,
                endpoint_id,
                binding: binding.clone(),
            })
            .expect("create stable slot");
        assert_eq!(created.state, ModelCredentialRecordState::Unconfigured);
        assert_eq!(created.row_revision, Revision::INITIAL);
        assert_eq!(created.last_committed_credential_revision, None);

        let configure = coordinator
            .prepare_model_credential_write(PrepareModelCredentialWrite {
                command_invocation_id: CommandInvocationId::new(),
                endpoint_id,
                credential_id,
                credential_slot: ModelCredentialSlot::ApiKey,
                expected_row_revision: ExpectedRevision::new(1),
                intent: ModelCredentialWriteIntent::Configure,
                binding: binding.clone(),
                models_revision: Revision::new(7),
                correlation_id: CorrelationId::new(),
            })
            .expect("prepare configure");
        let PrepareModelCredentialWriteResult::Prepared(configure) = configure else {
            panic!("new configure command must create a Journal");
        };
        assert_eq!(configure.journal_kind, CredentialMutationJournalKind::Configure);
        assert_eq!(configure.reserved_credential_revision, Some(CredentialRevision::INITIAL));
        assert!(configure.staging_credential_ref().is_ok());
        assert!(
            coordinator
                .credential_for_model_transport(
                    credential_id,
                    CredentialRevision::INITIAL,
                    &binding.credential_binding_fingerprint,
                )
                .is_err(),
            "a live mutation lease must block new model requests"
        );
        advance_final_written(&coordinator, configure.journal_id);
        let configured_status = aura_model_domain::EndpointCredentialStatus {
            endpoint_id,
            slot_name: ModelCredentialSlot::ApiKey,
            credential_id,
            state: EndpointCredentialState::Configured,
            active_credential_revision: Some(CredentialRevision::INITIAL),
            last_committed_credential_revision: Some(CredentialRevision::INITIAL),
            row_revision: Revision::new(2),
            error_code: None,
        };
        let committed = coordinator
            .commit_credential_mutation_metadata(
                configure.journal_id,
                credential_projection_plan(endpoint_id, Revision::new(7), configured_status),
            )
            .expect("commit configure metadata");
        assert_eq!(
            committed.credential_record.last_committed_credential_revision,
            Some(CredentialRevision::INITIAL)
        );
        assert_eq!(committed.credential_record.row_revision, Revision::new(2));
        finish_cleanup(&coordinator, configure.journal_id);
        assert_eq!(
            coordinator
                .credential_for_model_transport(
                    credential_id,
                    CredentialRevision::INITIAL,
                    &binding.credential_binding_fingerprint,
                )
                .expect("resolve configured credential")
                .credential_ref,
            binding.credential_ref
        );

        let replacement = coordinator
            .prepare_model_credential_write(PrepareModelCredentialWrite {
                command_invocation_id: CommandInvocationId::new(),
                endpoint_id,
                credential_id,
                credential_slot: ModelCredentialSlot::ApiKey,
                expected_row_revision: ExpectedRevision::new(2),
                intent: ModelCredentialWriteIntent::Replace,
                binding: binding.clone(),
                models_revision: Revision::new(7),
                correlation_id: CorrelationId::new(),
            })
            .expect("prepare replacement");
        let PrepareModelCredentialWriteResult::Prepared(replacement) = replacement else {
            panic!("new replace command must create a Journal");
        };
        let credential_revision_two =
            CredentialRevision::new(Revision::new(2)).expect("credential revision two");
        assert_eq!(replacement.journal_kind, CredentialMutationJournalKind::Replace);
        assert_eq!(replacement.reserved_credential_revision, Some(credential_revision_two));
        advance_final_written(&coordinator, replacement.journal_id);
        let replacement_status = aura_model_domain::EndpointCredentialStatus {
            endpoint_id,
            slot_name: ModelCredentialSlot::ApiKey,
            credential_id,
            state: EndpointCredentialState::Configured,
            active_credential_revision: Some(credential_revision_two),
            last_committed_credential_revision: Some(credential_revision_two),
            row_revision: Revision::new(3),
            error_code: None,
        };
        coordinator
            .commit_credential_mutation_metadata(
                replacement.journal_id,
                credential_projection_plan(endpoint_id, Revision::new(7), replacement_status),
            )
            .expect("commit replacement metadata");
        finish_cleanup(&coordinator, replacement.journal_id);

        let clear = coordinator
            .prepare_model_credential_clear(PrepareModelCredentialClear {
                command_invocation_id: CommandInvocationId::new(),
                endpoint_id,
                credential_id,
                credential_slot: ModelCredentialSlot::ApiKey,
                expected_row_revision: ExpectedRevision::new(3),
                models_revision: Revision::new(7),
                correlation_id: CorrelationId::new(),
            })
            .expect("prepare clear");
        let PrepareModelCredentialClearResult::Prepared(clear) = clear else {
            panic!("configured credential must produce a clear journal");
        };
        assert_eq!(clear.reserved_credential_revision, None);
        advance_final_written(&coordinator, clear.journal_id);
        let cleared_status = aura_model_domain::EndpointCredentialStatus {
            endpoint_id,
            slot_name: ModelCredentialSlot::ApiKey,
            credential_id,
            state: EndpointCredentialState::Cleared,
            active_credential_revision: None,
            last_committed_credential_revision: Some(credential_revision_two),
            row_revision: Revision::new(4),
            error_code: None,
        };
        let cleared = coordinator
            .commit_credential_mutation_metadata(
                clear.journal_id,
                credential_projection_plan(endpoint_id, Revision::new(7), cleared_status),
            )
            .expect("commit clear metadata");
        assert_eq!(cleared.credential_record.credential_id, credential_id);
        assert_eq!(
            cleared.credential_record.last_committed_credential_revision,
            Some(credential_revision_two)
        );
        assert_eq!(cleared.credential_record.row_revision, Revision::new(4));
        finish_cleanup(&coordinator, clear.journal_id);
        assert!(
            coordinator
                .credential_for_model_transport(
                    credential_id,
                    credential_revision_two,
                    &binding.credential_binding_fingerprint,
                )
                .is_err(),
            "cleared credentials must invalidate existing model channel authorization"
        );

        let repeated_clear = coordinator
            .prepare_model_credential_clear(PrepareModelCredentialClear {
                command_invocation_id: CommandInvocationId::new(),
                endpoint_id,
                credential_id,
                credential_slot: ModelCredentialSlot::ApiKey,
                expected_row_revision: ExpectedRevision::new(4),
                models_revision: Revision::new(7),
                correlation_id: CorrelationId::new(),
            })
            .expect("repeated clear");
        let PrepareModelCredentialClearResult::NoChange(status) = repeated_clear else {
            panic!("repeated clear must be a no-op");
        };
        assert_eq!(status.credential_id, credential_id);
        assert_eq!(status.state, EndpointCredentialState::Cleared);
        assert_eq!(status.last_committed_credential_revision, Some(credential_revision_two));

        drop(coordinator);
        let connection = rusqlite::Connection::open(database_path).expect("open database");
        let event_count = connection
            .query_row(
                "SELECT COUNT(*) FROM events
                 WHERE event_type IN (
                    'ModelEndpointCredentialConfigured',
                    'ModelEndpointCredentialCleared'
                 )",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("event count");
        assert_eq!(event_count, 3);
        let outbox_payloads = connection
            .prepare(
                "SELECT payload_json FROM projection_outbox
                 WHERE projection_kind='model_settings' ORDER BY outbox_id",
            )
            .expect("prepare outbox")
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query outbox")
            .collect::<Result<Vec<_>, _>>()
            .expect("outbox payloads");
        assert_eq!(outbox_payloads.len(), 3);
        for payload in outbox_payloads {
            let lowered = payload.to_ascii_lowercase();
            for forbidden in [
                "credential_ref",
                "wincred",
                "final_target",
                "binding_fingerprint",
                "secret",
                "journal",
            ] {
                assert!(!lowered.contains(forbidden), "Renderer projection leaked {forbidden}");
            }
        }
    }

    #[test]
    fn j_a_sqlite_schema_contains_no_secret_material_columns() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database_path = directory.path().join("data/aura.db");
        let coordinator = start(database_path.clone(), directory.path().join("data/artifacts"));
        drop(coordinator);
        let connection = rusqlite::Connection::open(database_path).expect("open database");
        for table in ["credential_records", "mutation_journals", "mutation_leases"] {
            let columns = connection
                .prepare(&format!("PRAGMA table_info({table})"))
                .expect("prepare table info")
                .query_map([], |row| row.get::<_, String>(1))
                .expect("query table info")
                .collect::<Result<Vec<_>, _>>()
                .expect("table columns");
            for forbidden in ["secret", "secret_hash", "secret_backup", "credential_blob"] {
                assert!(
                    columns.iter().all(|column| column != forbidden),
                    "{table} must not persist {forbidden}"
                );
            }
        }
    }
}
