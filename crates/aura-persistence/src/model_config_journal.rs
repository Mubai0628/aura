use std::collections::BTreeSet;

use aura_domain_common::{
    CommandInvocationId, CorrelationId, CredentialId, ErrorCode, MutationJournalId, Revision,
    UtcTimestamp,
};
use aura_projection_contract::ProjectionChange;
use serde::{Deserialize, Serialize};

use crate::{
    CreateModelCredentialSlot, CredentialMutationJournalRecord, CredentialMutationJournalState,
    ModelCredentialRecord, ModelProjectionDraft, MutationJournalMetadataCommit,
    MutationJournalRecord, PersistenceCoordinator, PersistenceError, PersistentEventDraft,
    PrepareMutationJournal, PreparedSessionModelBindingClear,
    coordinator::insert_event,
    model_binding::clear_prepared_session_model_bindings,
    model_credential::{
        PrepareModelCredentialDeleteChild, PrepareNewModelCredentialChild,
        commit_new_configured_model_credential_slot, insert_model_credential_slot,
        insert_prepared_model_credential_delete_child, insert_prepared_new_model_credential_child,
        load_credential_journal, mark_model_credential_delete_pending,
        validate_credential_projection_drafts,
    },
    model_projection::insert_model_projections,
    mutation_journal::{
        assert_current_mutation_lease, insert_prepared_mutation_journal, load_mutation_journal,
        load_mutation_journal_by_idempotency_key,
    },
};
use rusqlite::params;

pub const MODEL_CONFIG_MUTATION_JOURNAL_KIND: &str = "model_config";
pub const MODEL_CONFIG_MUTATION_JOURNAL_SCHEMA_VERSION: u32 = 2;
const MODEL_CONFIG_RESOURCE_KIND: &str = "model_config";
const MODEL_CONFIG_RESOURCE_ID: &str = "global";

const LEGACY_MODEL_CONFIG_MUTATION_JOURNAL_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelConfigCredentialChildPhase {
    BeforeConfigPublish,
    AfterConfigPublish,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelConfigCredentialChildOperation {
    Configure,
    Replace,
    Delete,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfigCredentialChildRef {
    pub credential_id: CredentialId,
    pub child_journal_id: MutationJournalId,
    pub phase: ModelConfigCredentialChildPhase,
    pub operation: ModelConfigCredentialChildOperation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelConfigMutationJournalState {
    Prepared,
    CredentialStaged,
    ConfigPublished,
    CredentialMutationCompleted,
    MetadataCommitted,
    CleanupPending,
    Completed,
    Aborted,
    ReconciliationRequired,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfigMutationJournalPayload {
    #[serde(default = "legacy_model_config_mutation_journal_schema_version")]
    pub schema_version: u32,
    pub command_invocation_id: CommandInvocationId,
    pub correlation_id: CorrelationId,
    pub prepared_config_bundle: serde_json::Value,
    pub expected_models_revision: Revision,
    pub reserved_models_revision: Option<Revision>,
    pub expected_aura_revision: Option<Revision>,
    pub reserved_aura_revision: Option<Revision>,
    pub credential_children: Vec<ModelConfigCredentialChildRef>,
    pub credential_slot_creations: Vec<CreateModelCredentialSlot>,
    pub session_binding_clears: Vec<PreparedSessionModelBindingClear>,
    pub metadata_events: Vec<PersistentEventDraft>,
    pub projection_drafts: Vec<ModelProjectionDraft>,
    pub success_result: serde_json::Value,
}

impl ModelConfigMutationJournalPayload {
    #[must_use]
    pub fn credential_child_phase(&self) -> Option<ModelConfigCredentialChildPhase> {
        self.credential_children.first().map(|child| child.phase)
    }

    pub fn validate(&self, journal_id: MutationJournalId) -> Result<(), PersistenceError> {
        if self.schema_version != MODEL_CONFIG_MUTATION_JOURNAL_SCHEMA_VERSION {
            return Err(PersistenceError::InvalidRequest(format!(
                "unsupported model config mutation journal schema version {}",
                self.schema_version
            )));
        }
        validate_reserved_revision(
            self.expected_models_revision,
            self.reserved_models_revision,
            "models",
        )?;
        match (self.expected_aura_revision, self.reserved_aura_revision) {
            (None, None) => {}
            (Some(expected), reserved) => {
                validate_reserved_revision(expected, reserved, "aura")?;
            }
            (None, Some(_)) => {
                return Err(PersistenceError::InvalidRequest(
                    "model config journal reserves aura revision without an expected revision"
                        .to_owned(),
                ));
            }
        }
        let mut previous_credential_id = None;
        let mut child_journal_ids = BTreeSet::new();
        let child_phase = self.credential_child_phase();
        for child in &self.credential_children {
            if child.child_journal_id == journal_id {
                return Err(PersistenceError::InvalidRequest(
                    "model config parent and credential child journal ids must differ".to_owned(),
                ));
            }
            if !child_journal_ids.insert(child.child_journal_id) {
                return Err(PersistenceError::InvalidRequest(
                    "model config journal contains duplicate credential child journal ids"
                        .to_owned(),
                ));
            }
            if previous_credential_id.is_some_and(|previous| previous >= child.credential_id) {
                return Err(PersistenceError::InvalidRequest(
                    "model config credential children must be uniquely sorted by CredentialId"
                        .to_owned(),
                ));
            }
            previous_credential_id = Some(child.credential_id);
            if Some(child.phase) != child_phase {
                return Err(PersistenceError::InvalidRequest(
                    "model config v1 forbids mixed credential child phases".to_owned(),
                ));
            }
        }
        match child_phase {
            Some(ModelConfigCredentialChildPhase::BeforeConfigPublish) => {
                if self.credential_children.len() != 1
                    || !matches!(
                        self.credential_children[0].operation,
                        ModelConfigCredentialChildOperation::Configure
                            | ModelConfigCredentialChildOperation::Replace
                    )
                {
                    return Err(PersistenceError::InvalidRequest(
                        "before-publish model config mutations require exactly one configure or replace child"
                            .to_owned(),
                    ));
                }
            }
            Some(ModelConfigCredentialChildPhase::AfterConfigPublish)
                if self.credential_children.iter().any(|child| {
                    child.operation != ModelConfigCredentialChildOperation::Delete
                }) =>
            {
                return Err(PersistenceError::InvalidRequest(
                    "after-publish model config children must all be credential deletes".to_owned(),
                ));
            }
            Some(ModelConfigCredentialChildPhase::AfterConfigPublish) => {}
            None => {}
        }
        if self.metadata_events.is_empty() || self.projection_drafts.is_empty() {
            return Err(PersistenceError::InvalidRequest(
                "model config journal requires a frozen event and projection plan".to_owned(),
            ));
        }
        let mut event_ids = BTreeSet::new();
        for event in &self.metadata_events {
            if !event_ids.insert(event.event_id) {
                return Err(PersistenceError::InvalidRequest(
                    "model config journal contains duplicate EventId values".to_owned(),
                ));
            }
        }
        let mut credential_ids = BTreeSet::new();
        let mut credential_slots = BTreeSet::new();
        for slot in &self.credential_slot_creations {
            slot.binding.validate(slot.credential_id)?;
            if !credential_ids.insert(slot.credential_id) {
                return Err(PersistenceError::InvalidRequest(
                    "model config journal contains duplicate CredentialId values".to_owned(),
                ));
            }
            if !credential_slots.insert((slot.endpoint_id, slot.binding.credential_slot)) {
                return Err(PersistenceError::InvalidRequest(
                    "model config journal contains duplicate endpoint credential slots".to_owned(),
                ));
            }
        }
        let mut cleared_sessions = BTreeSet::new();
        for clear in &self.session_binding_clears {
            if clear.expected_binding_revision.value() == 0 {
                return Err(PersistenceError::InvalidRequest(
                    "model config journal cannot clear an unversioned Session binding".to_owned(),
                ));
            }
            if !cleared_sessions.insert(clear.session_id) {
                return Err(PersistenceError::InvalidRequest(
                    "model config journal contains duplicate Session binding clears".to_owned(),
                ));
            }
        }

        let bundle_transaction_id = self
            .prepared_config_bundle
            .as_object()
            .and_then(|bundle| bundle.get("transaction_id"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "model config journal prepared bundle lacks transaction_id".to_owned(),
                )
            })?;
        if bundle_transaction_id != journal_id.to_string() {
            return Err(PersistenceError::InvalidRequest(
                "model config journal id does not match prepared bundle transaction_id".to_owned(),
            ));
        }
        validate_prepared_bundle_revisions(
            &self.prepared_config_bundle,
            self.expected_models_revision,
            self.reserved_models_revision,
            self.expected_aura_revision,
            self.reserved_aura_revision,
        )?;

        let encoded = serde_json::to_value(self)?;
        if let Some(path) = first_forbidden_secret_path(&encoded, "$".to_owned()) {
            return Err(PersistenceError::InvalidRequest(format!(
                "model config journal payload contains forbidden secret-bearing field at {path}"
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelConfigMutationJournalRecord {
    pub journal_id: MutationJournalId,
    pub state: ModelConfigMutationJournalState,
    pub payload: ModelConfigMutationJournalPayload,
    pub owner_runtime_instance_id: aura_domain_common::RuntimeInstanceId,
    pub lease_epoch: u64,
    pub error_code: Option<ErrorCode>,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PrepareModelConfigMutationJournalResult {
    Prepared(Box<ModelConfigMutationJournalRecord>),
    Existing(Box<ModelConfigMutationJournalRecord>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelConfigMetadataCommit {
    pub journal: ModelConfigMutationJournalRecord,
    pub event_row_ids: Vec<i64>,
    pub projection_changes: Vec<ProjectionChange>,
    pub projection_outbox_ids: Vec<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelConfigCredentialJournalPair {
    pub parent: ModelConfigMutationJournalRecord,
    pub credential_child: CredentialMutationJournalRecord,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PrepareModelConfigCredentialJournalPairResult {
    Prepared(Box<ModelConfigCredentialJournalPair>),
    Existing(Box<ModelConfigCredentialJournalPair>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelConfigCredentialDeleteJournals {
    pub parent: ModelConfigMutationJournalRecord,
    pub credential_children: Vec<CredentialMutationJournalRecord>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PrepareModelConfigCredentialDeleteJournalsResult {
    Prepared(Box<ModelConfigCredentialDeleteJournals>),
    Existing(Box<ModelConfigCredentialDeleteJournals>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelConfigCredentialMetadataCommit {
    pub parent: ModelConfigMutationJournalRecord,
    pub credential_child: CredentialMutationJournalRecord,
    pub credential_record: ModelCredentialRecord,
    pub event_row_ids: Vec<i64>,
    pub projection_changes: Vec<ProjectionChange>,
    pub projection_outbox_ids: Vec<i64>,
}

impl PersistenceCoordinator {
    pub fn prepare_model_config_mutation_journal(
        &self,
        journal_id: MutationJournalId,
        payload: ModelConfigMutationJournalPayload,
    ) -> Result<PrepareModelConfigMutationJournalResult, PersistenceError> {
        payload.validate(journal_id)?;
        let idempotency_key = payload.command_invocation_id.to_string();
        if let Some(existing) = self.mutation_journal_by_idempotency_key(
            MODEL_CONFIG_MUTATION_JOURNAL_KIND,
            &idempotency_key,
        )? {
            return decode_model_config_journal(existing)
                .map(Box::new)
                .map(PrepareModelConfigMutationJournalResult::Existing);
        }
        let prepared = self.prepare_mutation_journal(PrepareMutationJournal {
            journal_id,
            journal_kind: MODEL_CONFIG_MUTATION_JOURNAL_KIND.to_owned(),
            idempotency_key: Some(idempotency_key.clone()),
            initial_state: model_config_journal_state_name(
                ModelConfigMutationJournalState::Prepared,
            )
            .to_owned(),
            payload: serde_json::to_value(payload)?,
            resource_kind: MODEL_CONFIG_RESOURCE_KIND.to_owned(),
            resource_id: MODEL_CONFIG_RESOURCE_ID.to_owned(),
        })?;
        decode_model_config_journal(prepared)
            .map(Box::new)
            .map(PrepareModelConfigMutationJournalResult::Prepared)
    }

    /// Atomically prepares a ModelConfig Parent Journal and its single
    /// before-publish CredentialConfigure child for
    /// `models.endpoint.create_with_credential`. Neither the Credential slot
    /// nor any Renderer-visible metadata exists at this point.
    pub fn prepare_model_config_with_credential_journals(
        &self,
        parent_journal_id: MutationJournalId,
        payload: ModelConfigMutationJournalPayload,
        credential_child_journal_id: MutationJournalId,
    ) -> Result<PrepareModelConfigCredentialJournalPairResult, PersistenceError> {
        payload.validate(parent_journal_id)?;
        let child = payload.credential_children.first();
        let slot = payload.credential_slot_creations.first();
        if payload.credential_children.len() != 1
            || payload.credential_slot_creations.len() != 1
            || child.is_none_or(|child| {
                child.child_journal_id != credential_child_journal_id
                    || child.phase != ModelConfigCredentialChildPhase::BeforeConfigPublish
                    || child.operation != ModelConfigCredentialChildOperation::Configure
                    || slot.is_none_or(|slot| slot.credential_id != child.credential_id)
            })
        {
            return Err(PersistenceError::InvalidRequest(
                "credential-bearing endpoint creation requires exactly one before-publish child"
                    .to_owned(),
            ));
        }
        let reserved_models_revision = payload.reserved_models_revision.ok_or_else(|| {
            PersistenceError::InvalidRequest(
                "credential-bearing endpoint creation must publish models.json".to_owned(),
            )
        })?;
        let slot = payload.credential_slot_creations.first().cloned().ok_or_else(|| {
            PersistenceError::InvalidRequest("credential slot is missing".to_owned())
        })?;
        let command_invocation_id = payload.command_invocation_id;
        let correlation_id = payload.correlation_id;
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            if let Some(existing) = load_mutation_journal_by_idempotency_key(
                &transaction,
                MODEL_CONFIG_MUTATION_JOURNAL_KIND,
                &command_invocation_id.to_string(),
            )? {
                let parent = decode_model_config_journal(existing)?;
                let child_id = parent
                    .payload
                    .credential_children
                    .first()
                    .map(|child| child.child_journal_id)
                    .ok_or(PersistenceError::MutationJournalKindConflict)?;
                let credential_child = load_credential_journal(&transaction, child_id)?
                    .ok_or(PersistenceError::CredentialJournalNotFound)?;
                transaction.commit()?;
                return Ok(PrepareModelConfigCredentialJournalPairResult::Existing(Box::new(
                    ModelConfigCredentialJournalPair { parent, credential_child },
                )));
            }

            let parent_generic = insert_prepared_mutation_journal(
                &transaction,
                runtime_instance_id,
                &PrepareMutationJournal {
                    journal_id: parent_journal_id,
                    journal_kind: MODEL_CONFIG_MUTATION_JOURNAL_KIND.to_owned(),
                    idempotency_key: Some(command_invocation_id.to_string()),
                    initial_state: model_config_journal_state_name(
                        ModelConfigMutationJournalState::Prepared,
                    )
                    .to_owned(),
                    payload: serde_json::to_value(payload)?,
                    resource_kind: MODEL_CONFIG_RESOURCE_KIND.to_owned(),
                    resource_id: MODEL_CONFIG_RESOURCE_ID.to_owned(),
                },
            )?;
            let parent = decode_model_config_journal(parent_generic)?;
            let credential_child = insert_prepared_new_model_credential_child(
                &transaction,
                runtime_instance_id,
                PrepareNewModelCredentialChild {
                    journal_id: credential_child_journal_id,
                    command_invocation_id,
                    slot,
                    models_revision: reserved_models_revision,
                    correlation_id,
                },
            )?;
            transaction.commit()?;
            Ok(PrepareModelConfigCredentialJournalPairResult::Prepared(Box::new(
                ModelConfigCredentialJournalPair { parent, credential_child },
            )))
        })
    }

    /// Atomically acquires every CredentialId lease and freezes the complete,
    /// sorted after-publish delete-child list before models.json can be
    /// published. Any lease or identity conflict rolls back both the Parent
    /// and every Child preparation.
    pub fn prepare_model_config_with_credential_delete_journals(
        &self,
        parent_journal_id: MutationJournalId,
        payload: ModelConfigMutationJournalPayload,
        deletion_reason: crate::ModelCredentialDeletionReason,
    ) -> Result<PrepareModelConfigCredentialDeleteJournalsResult, PersistenceError> {
        payload.validate(parent_journal_id)?;
        if payload.credential_child_phase()
            != Some(ModelConfigCredentialChildPhase::AfterConfigPublish)
            || payload
                .credential_children
                .iter()
                .any(|child| child.operation != ModelConfigCredentialChildOperation::Delete)
            || !payload.credential_slot_creations.is_empty()
        {
            return Err(PersistenceError::InvalidRequest(
                "credential deletion requires only after-publish delete children".to_owned(),
            ));
        }
        let reserved_models_revision = payload.reserved_models_revision.ok_or_else(|| {
            PersistenceError::InvalidRequest(
                "credential deletion must publish models.json".to_owned(),
            )
        })?;
        let command_invocation_id = payload.command_invocation_id;
        let correlation_id = payload.correlation_id;
        let child_refs = payload.credential_children.clone();
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            if let Some(existing) = load_mutation_journal_by_idempotency_key(
                &transaction,
                MODEL_CONFIG_MUTATION_JOURNAL_KIND,
                &command_invocation_id.to_string(),
            )? {
                let parent = decode_model_config_journal(existing)?;
                let credential_children = parent
                    .payload
                    .credential_children
                    .iter()
                    .map(|child| {
                        load_credential_journal(&transaction, child.child_journal_id)?
                            .ok_or(PersistenceError::CredentialJournalNotFound)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                transaction.commit()?;
                return Ok(PrepareModelConfigCredentialDeleteJournalsResult::Existing(Box::new(
                    ModelConfigCredentialDeleteJournals { parent, credential_children },
                )));
            }

            let parent_generic = insert_prepared_mutation_journal(
                &transaction,
                runtime_instance_id,
                &PrepareMutationJournal {
                    journal_id: parent_journal_id,
                    journal_kind: MODEL_CONFIG_MUTATION_JOURNAL_KIND.to_owned(),
                    idempotency_key: Some(command_invocation_id.to_string()),
                    initial_state: model_config_journal_state_name(
                        ModelConfigMutationJournalState::Prepared,
                    )
                    .to_owned(),
                    payload: serde_json::to_value(payload)?,
                    resource_kind: MODEL_CONFIG_RESOURCE_KIND.to_owned(),
                    resource_id: MODEL_CONFIG_RESOURCE_ID.to_owned(),
                },
            )?;
            let parent = decode_model_config_journal(parent_generic)?;
            let mut credential_children = Vec::with_capacity(child_refs.len());
            for child in child_refs {
                credential_children.push(insert_prepared_model_credential_delete_child(
                    &transaction,
                    runtime_instance_id,
                    PrepareModelCredentialDeleteChild {
                        journal_id: child.child_journal_id,
                        parent_journal_id,
                        command_invocation_id,
                        credential_id: child.credential_id,
                        deletion_reason,
                        models_revision: reserved_models_revision,
                        correlation_id,
                    },
                )?);
            }
            transaction.commit()?;
            Ok(PrepareModelConfigCredentialDeleteJournalsResult::Prepared(Box::new(
                ModelConfigCredentialDeleteJournals { parent, credential_children },
            )))
        })
    }

    pub fn model_config_mutation_journal(
        &self,
        journal_id: MutationJournalId,
    ) -> Result<Option<ModelConfigMutationJournalRecord>, PersistenceError> {
        self.mutation_journal(journal_id)?.map(decode_model_config_journal).transpose()
    }

    pub fn model_config_mutation_journal_by_command(
        &self,
        command_invocation_id: CommandInvocationId,
    ) -> Result<Option<ModelConfigMutationJournalRecord>, PersistenceError> {
        self.mutation_journal_by_idempotency_key(
            MODEL_CONFIG_MUTATION_JOURNAL_KIND,
            &command_invocation_id.to_string(),
        )?
        .map(decode_model_config_journal)
        .transpose()
    }

    /// Returns every non-terminal model-config Parent Journal in deterministic
    /// creation order so the Core composition root can adopt and resume it
    /// before exposing the P05 runtime snapshot.
    pub fn incomplete_model_config_mutation_journals(
        &self,
    ) -> Result<Vec<ModelConfigMutationJournalRecord>, PersistenceError> {
        self.active_mutation_journals()?
            .into_iter()
            .filter(|journal| journal.journal_kind == MODEL_CONFIG_MUTATION_JOURNAL_KIND)
            .map(|journal| {
                let journal_id = journal.journal_id;
                let recovery_phase = journal.state.clone();
                let schema_version = journal
                    .payload
                    .get("schema_version")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(u64::from(LEGACY_MODEL_CONFIG_MUTATION_JOURNAL_SCHEMA_VERSION));
                decode_model_config_journal(journal).map_err(|error| {
                    PersistenceError::InvalidRequest(format!(
                        "model config recovery decode failed: journal_id={journal_id} \
                         schema_version={schema_version} recovery_phase={recovery_phase} \
                         error_code=persistence.model_config_journal_decode_failed; {error}"
                    ))
                })
            })
            .collect()
    }

    pub fn adopt_model_config_mutation_journal(
        &self,
        journal_id: MutationJournalId,
    ) -> Result<ModelConfigMutationJournalRecord, PersistenceError> {
        self.model_config_mutation_journal(journal_id)?
            .ok_or(PersistenceError::MutationJournalNotFound)?;
        let adopted = self.adopt_mutation_journal_lease(
            journal_id,
            MODEL_CONFIG_MUTATION_JOURNAL_KIND,
            MODEL_CONFIG_RESOURCE_KIND,
            MODEL_CONFIG_RESOURCE_ID.to_owned(),
        )?;
        decode_model_config_journal(adopted)
    }

    pub fn transition_model_config_mutation_journal(
        &self,
        journal_id: MutationJournalId,
        expected_state: ModelConfigMutationJournalState,
        next_state: ModelConfigMutationJournalState,
        error_code: Option<ErrorCode>,
    ) -> Result<ModelConfigMutationJournalRecord, PersistenceError> {
        let current = self
            .model_config_mutation_journal(journal_id)?
            .ok_or(PersistenceError::MutationJournalNotFound)?;
        if current.state != expected_state
            || !valid_model_config_journal_transition(
                expected_state,
                next_state,
                current.payload.credential_child_phase(),
            )
        {
            return Err(PersistenceError::MutationJournalStateConflict);
        }
        let updated = self.transition_mutation_journal(
            journal_id,
            MODEL_CONFIG_MUTATION_JOURNAL_KIND,
            model_config_journal_state_name(expected_state),
            model_config_journal_state_name(next_state),
            error_code,
        )?;
        decode_model_config_journal(updated)
    }

    /// Makes every frozen delete child non-resolvable in one SQLite
    /// transaction immediately after the config-publish forward point.
    pub fn begin_model_config_credential_deletes(
        &self,
        parent_journal_id: MutationJournalId,
    ) -> Result<Vec<CredentialMutationJournalRecord>, PersistenceError> {
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let parent_generic = load_mutation_journal(&transaction, parent_journal_id)?
                .ok_or(PersistenceError::MutationJournalNotFound)?;
            assert_current_mutation_lease(&transaction, &parent_generic, runtime_instance_id)?;
            let parent = decode_model_config_journal(parent_generic)?;
            if parent.state != ModelConfigMutationJournalState::ConfigPublished
                || parent.payload.credential_child_phase()
                    != Some(ModelConfigCredentialChildPhase::AfterConfigPublish)
            {
                return Err(PersistenceError::MutationJournalStateConflict);
            }
            let now = UtcTimestamp::now();
            let mut children = Vec::with_capacity(parent.payload.credential_children.len());
            for child_ref in &parent.payload.credential_children {
                let generic = load_mutation_journal(&transaction, child_ref.child_journal_id)?
                    .ok_or(PersistenceError::CredentialJournalNotFound)?;
                assert_current_mutation_lease(&transaction, &generic, runtime_instance_id)?;
                let child = load_credential_journal(&transaction, child_ref.child_journal_id)?
                    .ok_or(PersistenceError::CredentialJournalNotFound)?;
                if child.credential_id != child_ref.credential_id
                    || child.parent_journal_id != Some(parent_journal_id)
                    || child.journal_kind != crate::CredentialMutationJournalKind::Delete
                {
                    return Err(PersistenceError::CredentialBindingMismatch);
                }
                mark_model_credential_delete_pending(&transaction, &child, now)?;
                children.push(
                    load_credential_journal(&transaction, child_ref.child_journal_id)?
                        .ok_or(PersistenceError::CredentialJournalNotFound)?,
                );
            }
            transaction.commit()?;
            Ok(children)
        })
    }

    pub fn commit_model_config_mutation_metadata(
        &self,
        journal_id: MutationJournalId,
    ) -> Result<ModelConfigMetadataCommit, PersistenceError> {
        let current = self
            .model_config_mutation_journal(journal_id)?
            .ok_or(PersistenceError::MutationJournalNotFound)?;
        let expected_state = match (current.state, current.payload.credential_children.is_empty()) {
            (ModelConfigMutationJournalState::ConfigPublished, true)
            | (ModelConfigMutationJournalState::CredentialMutationCompleted, false) => {
                current.state
            }
            _ => return Err(PersistenceError::MutationJournalStateConflict),
        };
        let credential_slot_creations = current.payload.credential_slot_creations.clone();
        let session_binding_clears = current.payload.session_binding_clears.clone();
        let committed = self.commit_mutation_journal_metadata_with(
            journal_id,
            crate::mutation_journal::MutationJournalMetadataTransition {
                journal_kind: MODEL_CONFIG_MUTATION_JOURNAL_KIND,
                expected_state: model_config_journal_state_name(expected_state),
                next_state: model_config_journal_state_name(
                    ModelConfigMutationJournalState::MetadataCommitted,
                ),
            },
            current.payload.metadata_events.clone(),
            current.payload.projection_drafts.clone(),
            move |transaction| {
                let now = UtcTimestamp::now();
                for slot in &credential_slot_creations {
                    insert_model_credential_slot(transaction, slot, now)?;
                }
                clear_prepared_session_model_bindings(transaction, &session_binding_clears)?;
                Ok((Vec::new(), Vec::new()))
            },
        )?;
        model_config_metadata_commit(committed)
    }

    /// Runtime Visibility Commit Point for endpoint creation with an inline
    /// Secret. The Endpoint event, configured Credential record/event, scoped
    /// projections, and both Journal state transitions become visible in one
    /// SQLite transaction after models.json and the committed WinCred record
    /// have crossed their respective forward-only commit points.
    pub fn commit_model_config_with_credential_metadata(
        &self,
        parent_journal_id: MutationJournalId,
    ) -> Result<ModelConfigCredentialMetadataCommit, PersistenceError> {
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let parent_generic = load_mutation_journal(&transaction, parent_journal_id)?
                .ok_or(PersistenceError::MutationJournalNotFound)?;
            assert_current_mutation_lease(&transaction, &parent_generic, runtime_instance_id)?;
            let parent = decode_model_config_journal(parent_generic)?;
            if parent.state != ModelConfigMutationJournalState::CredentialMutationCompleted
                || parent.payload.credential_child_phase()
                    != Some(ModelConfigCredentialChildPhase::BeforeConfigPublish)
                || parent.payload.credential_slot_creations.len() != 1
            {
                return Err(PersistenceError::MutationJournalStateConflict);
            }
            let child_id = parent
                .payload
                .credential_children
                .first()
                .map(|child| child.child_journal_id)
                .ok_or(PersistenceError::MutationJournalStateConflict)?;
            let child_generic = load_mutation_journal(&transaction, child_id)?
                .ok_or(PersistenceError::CredentialJournalNotFound)?;
            assert_current_mutation_lease(&transaction, &child_generic, runtime_instance_id)?;
            let child = load_credential_journal(&transaction, child_id)?
                .ok_or(PersistenceError::CredentialJournalNotFound)?;
            if child.state != CredentialMutationJournalState::FinalCredentialWritten
                || Some(child.models_revision) != parent.payload.reserved_models_revision
            {
                return Err(PersistenceError::CredentialJournalStateConflict);
            }
            let slot = parent
                .payload
                .credential_slot_creations
                .first()
                .ok_or(PersistenceError::CredentialRecordNotFound)?;
            let now = UtcTimestamp::now();
            let atomic_group_id = parent_journal_id.to_string();
            let (credential_record, credential_event) =
                commit_new_configured_model_credential_slot(
                    &transaction,
                    slot,
                    &child,
                    &atomic_group_id,
                    now,
                )?;
            let credential_status = credential_record.renderer_status()?;
            validate_credential_projection_drafts(
                &parent.payload.projection_drafts,
                slot.endpoint_id,
                &credential_status,
                child.models_revision,
            )?;

            let mut events = parent.payload.metadata_events.clone();
            for event in &mut events {
                if event.atomic_group_id.as_ref().is_some_and(|group| group != &atomic_group_id) {
                    return Err(PersistenceError::InvalidRequest(
                        "model config event has a conflicting atomic group".to_owned(),
                    ));
                }
                event.atomic_group_id = Some(atomic_group_id.clone());
            }
            events.push(credential_event);
            let mut event_row_ids = Vec::with_capacity(events.len());
            for event in events {
                event_row_ids.push(insert_event(&transaction, &event)?);
            }
            let (projection_changes, projection_outbox_ids) = insert_model_projections(
                &transaction,
                runtime_instance_id,
                parent.payload.projection_drafts.clone(),
            )?;

            let parent_changed = transaction.execute(
                "UPDATE mutation_journals
                 SET state='metadata_committed', error_code=NULL, updated_at=?1
                 WHERE journal_id=?2 AND journal_kind=?3
                   AND state='credential_mutation_completed'
                   AND owner_runtime_instance_id=?4 AND lease_epoch=?5",
                params![
                    now.to_string(),
                    parent_journal_id.to_string(),
                    MODEL_CONFIG_MUTATION_JOURNAL_KIND,
                    runtime_instance_id.to_string(),
                    i64::try_from(parent.lease_epoch).map_err(|_| {
                        PersistenceError::InvalidRequest(
                            "model config lease epoch exceeds SQLite range".to_owned(),
                        )
                    })?,
                ],
            )?;
            let child_changed = transaction.execute(
                "UPDATE mutation_journals
                 SET state='metadata_committed', error_code=NULL, updated_at=?1
                 WHERE journal_id=?2 AND journal_kind=?3
                   AND state='final_credential_written'
                   AND owner_runtime_instance_id=?4 AND lease_epoch=?5",
                params![
                    now.to_string(),
                    child_id.to_string(),
                    child_generic.journal_kind,
                    runtime_instance_id.to_string(),
                    i64::try_from(child.lease_epoch).map_err(|_| {
                        PersistenceError::InvalidRequest(
                            "credential lease epoch exceeds SQLite range".to_owned(),
                        )
                    })?,
                ],
            )?;
            if parent_changed != 1 || child_changed != 1 {
                return Err(PersistenceError::MutationJournalStateConflict);
            }
            let parent = load_mutation_journal(&transaction, parent_journal_id)?
                .map(decode_model_config_journal)
                .transpose()?
                .ok_or(PersistenceError::MutationJournalNotFound)?;
            let credential_child = load_credential_journal(&transaction, child_id)?
                .ok_or(PersistenceError::CredentialJournalNotFound)?;
            transaction.commit()?;
            Ok(ModelConfigCredentialMetadataCommit {
                parent,
                credential_child,
                credential_record,
                event_row_ids,
                projection_changes,
                projection_outbox_ids,
            })
        })
    }
}

fn decode_model_config_journal(
    generic: MutationJournalRecord,
) -> Result<ModelConfigMutationJournalRecord, PersistenceError> {
    if generic.journal_kind != MODEL_CONFIG_MUTATION_JOURNAL_KIND
        || generic.idempotency_key.is_none()
    {
        return Err(PersistenceError::MutationJournalKindConflict);
    }
    let mut encoded_payload = generic.payload;
    migrate_model_config_mutation_journal_payload(&mut encoded_payload)?;
    let payload: ModelConfigMutationJournalPayload = serde_json::from_value(encoded_payload)?;
    payload.validate(generic.journal_id)?;
    if generic.idempotency_key.as_deref()
        != Some(payload.command_invocation_id.to_string().as_str())
    {
        return Err(PersistenceError::MutationJournalKindConflict);
    }
    Ok(ModelConfigMutationJournalRecord {
        journal_id: generic.journal_id,
        state: parse_model_config_journal_state(&generic.state)?,
        payload,
        owner_runtime_instance_id: generic.owner_runtime_instance_id,
        lease_epoch: generic.lease_epoch,
        error_code: generic.error_code,
        created_at: generic.created_at,
        updated_at: generic.updated_at,
    })
}

const fn legacy_model_config_mutation_journal_schema_version() -> u32 {
    LEGACY_MODEL_CONFIG_MUTATION_JOURNAL_SCHEMA_VERSION
}

fn migrate_model_config_mutation_journal_payload(
    encoded: &mut serde_json::Value,
) -> Result<(), PersistenceError> {
    let payload = encoded.as_object_mut().ok_or_else(|| {
        PersistenceError::InvalidRequest(
            "model config mutation journal payload must be an object".to_owned(),
        )
    })?;
    let schema_version = payload
        .get("schema_version")
        .map(|value| {
            value.as_u64().and_then(|version| u32::try_from(version).ok()).ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "model config mutation journal schema version must be a u32".to_owned(),
                )
            })
        })
        .transpose()?
        .unwrap_or(LEGACY_MODEL_CONFIG_MUTATION_JOURNAL_SCHEMA_VERSION);

    match schema_version {
        LEGACY_MODEL_CONFIG_MUTATION_JOURNAL_SCHEMA_VERSION => {
            let drafts = payload
                .get_mut("projection_drafts")
                .and_then(serde_json::Value::as_array_mut)
                .ok_or_else(|| {
                    PersistenceError::InvalidRequest(
                        "legacy model config mutation journal lacks projection drafts".to_owned(),
                    )
                })?;
            for draft in drafts {
                let Some(draft) = draft.as_object_mut() else {
                    return Err(PersistenceError::InvalidRequest(
                        "legacy model config projection draft must be an object".to_owned(),
                    ));
                };
                if draft.get("projection_kind").and_then(serde_json::Value::as_str)
                    == Some("endpoint_detail")
                {
                    let endpoint = draft
                        .get_mut("payload")
                        .and_then(serde_json::Value::as_object_mut)
                        .ok_or_else(|| {
                            PersistenceError::InvalidRequest(
                                "legacy endpoint detail projection payload must be an object"
                                    .to_owned(),
                            )
                        })?;
                    endpoint.remove("latest_availability_test");
                }
            }
            payload.insert(
                "schema_version".to_owned(),
                serde_json::Value::from(MODEL_CONFIG_MUTATION_JOURNAL_SCHEMA_VERSION),
            );
            Ok(())
        }
        MODEL_CONFIG_MUTATION_JOURNAL_SCHEMA_VERSION => Ok(()),
        unsupported => Err(PersistenceError::InvalidRequest(format!(
            "unsupported model config mutation journal schema version {unsupported}"
        ))),
    }
}

fn model_config_metadata_commit(
    generic: MutationJournalMetadataCommit,
) -> Result<ModelConfigMetadataCommit, PersistenceError> {
    Ok(ModelConfigMetadataCommit {
        journal: decode_model_config_journal(generic.journal)?,
        event_row_ids: generic.event_row_ids,
        projection_changes: generic.projection_changes,
        projection_outbox_ids: generic.projection_outbox_ids,
    })
}

fn validate_reserved_revision(
    expected: Revision,
    reserved: Option<Revision>,
    label: &str,
) -> Result<(), PersistenceError> {
    if let Some(reserved) = reserved
        && reserved.value()
            != expected.value().checked_add(1).ok_or_else(|| {
                PersistenceError::InvalidRequest(format!("model config {label} revision overflow"))
            })?
    {
        return Err(PersistenceError::InvalidRequest(format!(
            "model config {label} reserved revision must immediately follow expected revision"
        )));
    }
    Ok(())
}

fn validate_prepared_bundle_revisions(
    bundle: &serde_json::Value,
    expected_models_revision: Revision,
    reserved_models_revision: Option<Revision>,
    expected_aura_revision: Option<Revision>,
    reserved_aura_revision: Option<Revision>,
) -> Result<(), PersistenceError> {
    let entries = bundle
        .as_object()
        .and_then(|value| value.get("entries"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            PersistenceError::InvalidRequest(
                "model config journal prepared bundle lacks entries".to_owned(),
            )
        })?;
    let mut seen_models = false;
    let mut seen_aura = false;
    for entry in entries {
        let entry = entry.as_object().ok_or_else(|| {
            PersistenceError::InvalidRequest(
                "model config journal prepared entry must be an object".to_owned(),
            )
        })?;
        let module = entry.get("module").and_then(serde_json::Value::as_str).ok_or_else(|| {
            PersistenceError::InvalidRequest(
                "model config journal prepared entry lacks module".to_owned(),
            )
        })?;
        let before =
            entry.get("before_revision").and_then(serde_json::Value::as_u64).ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "model config journal prepared entry lacks before_revision".to_owned(),
                )
            })?;
        let candidate = entry
            .get("candidate")
            .and_then(serde_json::Value::as_object)
            .and_then(|candidate| candidate.get("revision"))
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "model config journal prepared entry lacks candidate revision".to_owned(),
                )
            })?;
        match module {
            "models" if !seen_models => {
                seen_models = true;
                if before != expected_models_revision.value()
                    || Some(candidate) != reserved_models_revision.map(Revision::value)
                {
                    return Err(PersistenceError::InvalidRequest(
                        "model config journal models revisions do not match prepared bundle"
                            .to_owned(),
                    ));
                }
            }
            "aura" if !seen_aura => {
                seen_aura = true;
                if Some(before) != expected_aura_revision.map(Revision::value)
                    || Some(candidate) != reserved_aura_revision.map(Revision::value)
                {
                    return Err(PersistenceError::InvalidRequest(
                        "model config journal aura revisions do not match prepared bundle"
                            .to_owned(),
                    ));
                }
            }
            "models" | "aura" => {
                return Err(PersistenceError::InvalidRequest(
                    "model config journal prepared bundle contains a duplicate module".to_owned(),
                ));
            }
            _ => {
                return Err(PersistenceError::InvalidRequest(
                    "model config journal prepared bundle contains a non-P05 module".to_owned(),
                ));
            }
        }
    }
    if seen_models != reserved_models_revision.is_some()
        || seen_aura != reserved_aura_revision.is_some()
    {
        return Err(PersistenceError::InvalidRequest(
            "model config journal reserved revisions do not match prepared modules".to_owned(),
        ));
    }
    Ok(())
}

fn valid_model_config_journal_transition(
    current: ModelConfigMutationJournalState,
    next: ModelConfigMutationJournalState,
    child_phase: Option<ModelConfigCredentialChildPhase>,
) -> bool {
    use ModelConfigCredentialChildPhase::{AfterConfigPublish, BeforeConfigPublish};
    use ModelConfigMutationJournalState::{
        Aborted, CleanupPending, Completed, ConfigPublished, CredentialMutationCompleted,
        CredentialStaged, MetadataCommitted, Prepared, ReconciliationRequired,
    };

    if next == ReconciliationRequired {
        return !matches!(current, Completed | Aborted | ReconciliationRequired);
    }
    matches!(
        (current, next, child_phase),
        (Prepared, Aborted, _)
            | (CredentialStaged, Aborted, Some(BeforeConfigPublish))
            | (Prepared, CredentialStaged, Some(BeforeConfigPublish))
            | (Prepared, ConfigPublished, None | Some(AfterConfigPublish))
            | (CredentialStaged, ConfigPublished, Some(BeforeConfigPublish))
            | (ConfigPublished, CredentialMutationCompleted, Some(_))
            | (ConfigPublished, MetadataCommitted, None)
            | (CredentialMutationCompleted, MetadataCommitted, Some(_))
            | (MetadataCommitted, CleanupPending, _)
            | (CleanupPending, Completed, _)
    )
}

const fn model_config_journal_state_name(state: ModelConfigMutationJournalState) -> &'static str {
    match state {
        ModelConfigMutationJournalState::Prepared => "prepared",
        ModelConfigMutationJournalState::CredentialStaged => "credential_staged",
        ModelConfigMutationJournalState::ConfigPublished => "config_published",
        ModelConfigMutationJournalState::CredentialMutationCompleted => {
            "credential_mutation_completed"
        }
        ModelConfigMutationJournalState::MetadataCommitted => "metadata_committed",
        ModelConfigMutationJournalState::CleanupPending => "cleanup_pending",
        ModelConfigMutationJournalState::Completed => "completed",
        ModelConfigMutationJournalState::Aborted => "aborted",
        ModelConfigMutationJournalState::ReconciliationRequired => "reconciliation_required",
    }
}

fn parse_model_config_journal_state(
    value: &str,
) -> Result<ModelConfigMutationJournalState, PersistenceError> {
    match value {
        "prepared" => Ok(ModelConfigMutationJournalState::Prepared),
        "credential_staged" => Ok(ModelConfigMutationJournalState::CredentialStaged),
        "config_published" => Ok(ModelConfigMutationJournalState::ConfigPublished),
        "credential_mutation_completed" => {
            Ok(ModelConfigMutationJournalState::CredentialMutationCompleted)
        }
        "metadata_committed" => Ok(ModelConfigMutationJournalState::MetadataCommitted),
        "cleanup_pending" => Ok(ModelConfigMutationJournalState::CleanupPending),
        "completed" => Ok(ModelConfigMutationJournalState::Completed),
        "aborted" => Ok(ModelConfigMutationJournalState::Aborted),
        "reconciliation_required" => Ok(ModelConfigMutationJournalState::ReconciliationRequired),
        _ => Err(PersistenceError::MutationJournalStateConflict),
    }
}

fn first_forbidden_secret_path(value: &serde_json::Value, path: String) -> Option<String> {
    match value {
        serde_json::Value::Object(values) => values.iter().find_map(|(key, value)| {
            let normalized = key.to_ascii_lowercase().replace('-', "_");
            if matches!(
                normalized.as_str(),
                "secret" | "api_key" | "authorization" | "x_api_key" | "staging_target"
            ) {
                return Some(format!("{path}.{key}"));
            }
            first_forbidden_secret_path(value, format!("{path}.{key}"))
        }),
        serde_json::Value::Array(values) => values.iter().enumerate().find_map(|(index, value)| {
            first_forbidden_secret_path(value, format!("{path}[{index}]"))
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{
        CommandInvocationId, ContentHash, CorrelationId, CredentialId, CredentialRevision,
        EndpointId, ExpectedRevision, MutationJournalId, Revision, RuntimeInstanceId,
    };
    use aura_model_domain::{
        CredentialBindingFingerprint, EndpointAuthScheme, EndpointCredentialState,
        EndpointCredentialStatus, ModelCredentialSlot, ProviderCatalogProjection,
        ProviderCatalogState,
    };
    use aura_permission_domain::NormalizedOrigin;

    use crate::model_projection_fixture::credential_projection_plan;
    use crate::{
        CreateModelCredentialSlot, CredentialMutationJournalState, ModelCredentialDeletionReason,
        ModelCredentialRecordState, ModelCredentialStorageBinding, ModelCredentialWriteIntent,
        ModelProjectionDraft, PersistenceCoordinator, PersistenceOptions, PersistenceStartup,
        PersistentEventDraft, PrepareModelConfigCredentialDeleteJournalsResult,
        PrepareModelConfigMutationJournalResult, PrepareModelCredentialWrite,
        PrepareModelCredentialWriteResult,
    };

    use super::{
        MODEL_CONFIG_MUTATION_JOURNAL_SCHEMA_VERSION, ModelConfigCredentialChildOperation,
        ModelConfigCredentialChildPhase, ModelConfigCredentialChildRef,
        ModelConfigMutationJournalPayload, ModelConfigMutationJournalState,
        migrate_model_config_mutation_journal_payload, valid_model_config_journal_transition,
    };

    fn start() -> (tempfile::TempDir, PersistenceCoordinator) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
            directory.path().join("data/aura.db"),
            directory.path().join("data/artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("start persistence")
        {
            PersistenceStartup::Ready(coordinator) => coordinator,
            PersistenceStartup::RecoveryRequired(recovery) => {
                panic!("unexpected recovery: {}", recovery.reason)
            }
        };
        (directory, persistence)
    }

    #[test]
    fn legacy_journal_migration_removes_only_the_retired_endpoint_field() {
        let mut encoded = serde_json::json!({
            "projection_drafts": [
                {
                    "projection_kind": "endpoint_detail",
                    "payload": {
                        "endpoint_id": "preserved",
                        "latest_availability_test": { "legacy": true }
                    }
                },
                {
                    "projection_kind": "model_settings",
                    "payload": { "latest_availability_test": "unrelated" }
                }
            ]
        });

        migrate_model_config_mutation_journal_payload(&mut encoded)
            .expect("legacy journal migration");

        assert_eq!(
            encoded.get("schema_version").and_then(serde_json::Value::as_u64),
            Some(u64::from(MODEL_CONFIG_MUTATION_JOURNAL_SCHEMA_VERSION))
        );
        let drafts = encoded["projection_drafts"].as_array().expect("projection drafts");
        assert_eq!(drafts[0]["payload"]["endpoint_id"], "preserved");
        assert!(drafts[0]["payload"].get("latest_availability_test").is_none());
        assert_eq!(drafts[1]["payload"]["latest_availability_test"], "unrelated");
    }

    #[test]
    fn journal_migration_rejects_an_unknown_schema_version() {
        let mut encoded = serde_json::json!({
            "schema_version": MODEL_CONFIG_MUTATION_JOURNAL_SCHEMA_VERSION + 1,
            "projection_drafts": []
        });

        let error = migrate_model_config_mutation_journal_payload(&mut encoded)
            .expect_err("unknown schema must fail closed");
        assert!(error.to_string().contains("unsupported model config mutation journal schema"));
    }

    #[test]
    fn incomplete_future_schema_reports_stable_recovery_diagnostics() {
        let (directory, persistence) = start();
        let database_path = directory.path().join("data/aura.db");
        let artifact_root = directory.path().join("data/artifacts");
        let journal_id = MutationJournalId::new();
        persistence
            .prepare_model_config_mutation_journal(
                journal_id,
                payload(journal_id, CommandInvocationId::new(), CorrelationId::new()),
            )
            .expect("prepare future-schema parent");
        drop(persistence);

        let future_schema_version = MODEL_CONFIG_MUTATION_JOURNAL_SCHEMA_VERSION + 1;
        let connection = rusqlite::Connection::open(&database_path).expect("open journal database");
        let payload_json = connection
            .query_row(
                "SELECT payload_json FROM mutation_journals WHERE journal_id=?1",
                [journal_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("read journal payload");
        let mut future: serde_json::Value =
            serde_json::from_str(&payload_json).expect("decode journal payload");
        future["schema_version"] = serde_json::Value::from(future_schema_version);
        connection
            .execute(
                "UPDATE mutation_journals SET payload_json=?1 WHERE journal_id=?2",
                rusqlite::params![
                    serde_json::to_string(&future).expect("encode future journal payload"),
                    journal_id.to_string(),
                ],
            )
            .expect("persist future journal payload");
        drop(connection);

        let recovered = match PersistenceCoordinator::start(PersistenceOptions::new(
            database_path,
            artifact_root,
            RuntimeInstanceId::new(),
        ))
        .expect("restart persistence")
        {
            PersistenceStartup::Ready(coordinator) => coordinator,
            PersistenceStartup::RecoveryRequired(recovery) => {
                panic!("unexpected storage recovery requirement: {}", recovery.reason)
            }
        };
        let error = recovered
            .incomplete_model_config_mutation_journals()
            .expect_err("future schema must fail closed");
        let diagnostic = error.to_string();
        assert!(diagnostic.contains(&format!("journal_id={journal_id}")));
        assert!(diagnostic.contains(&format!("schema_version={future_schema_version}")));
        assert!(diagnostic.contains("recovery_phase=prepared"));
        assert!(diagnostic.contains("error_code=persistence.model_config_journal_decode_failed"));
        assert!(!diagnostic.contains("credential_ref"));
        assert!(!diagnostic.contains("prepared_config_bundle"));
    }

    fn payload(
        journal_id: MutationJournalId,
        command_invocation_id: CommandInvocationId,
        correlation_id: CorrelationId,
    ) -> ModelConfigMutationJournalPayload {
        ModelConfigMutationJournalPayload {
            schema_version: MODEL_CONFIG_MUTATION_JOURNAL_SCHEMA_VERSION,
            command_invocation_id,
            correlation_id,
            prepared_config_bundle: serde_json::json!({
                "transaction_id": journal_id,
                "entries": [{
                    "module": "models",
                    "before_revision": 1,
                    "before_content_hash": "blake3:placeholder",
                    "candidate": {
                        "revision": 2,
                        "values": {},
                    },
                }],
                "created_at": "2026-08-31T00:00:00Z",
            }),
            expected_models_revision: Revision::new(1),
            reserved_models_revision: Some(Revision::new(2)),
            expected_aura_revision: None,
            reserved_aura_revision: None,
            credential_children: Vec::new(),
            credential_slot_creations: Vec::new(),
            session_binding_clears: Vec::new(),
            metadata_events: vec![PersistentEventDraft::system(
                "ModelProviderCreated",
                correlation_id,
                serde_json::json!({"provider_id":"prv_test"}),
            )],
            projection_drafts: vec![ModelProjectionDraft::ProviderCatalog(
                ProviderCatalogProjection {
                    state: ProviderCatalogState::Unavailable,
                    catalog_revision: None,
                    catalog_content_hash: None,
                    checked_at: None,
                    last_successful_refresh_at: None,
                    cards: Vec::new(),
                    last_error_code: None,
                    projection_revision: Revision::INITIAL,
                },
            )],
            success_result: serde_json::json!({"models_revision":"2"}),
        }
    }

    fn credential_slot(
        credential_id: CredentialId,
        endpoint_id: EndpointId,
    ) -> CreateModelCredentialSlot {
        let bundle_namespace_key = "0123456789abcdef".to_owned();
        CreateModelCredentialSlot {
            credential_id,
            endpoint_id,
            binding: ModelCredentialStorageBinding {
                credential_ref: format!(
                    "wincred://aura/{bundle_namespace_key}/credential/{credential_id}"
                )
                .parse()
                .expect("credential ref"),
                credential_binding_fingerprint: CredentialBindingFingerprint(ContentHash::sha256(
                    b"model config slot binding",
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

    fn commit_credential_slots(
        persistence: &PersistenceCoordinator,
        slots: Vec<CreateModelCredentialSlot>,
    ) {
        let journal_id = MutationJournalId::new();
        let mut frozen = payload(journal_id, CommandInvocationId::new(), CorrelationId::new());
        frozen.credential_slot_creations = slots;
        persistence
            .prepare_model_config_mutation_journal(journal_id, frozen)
            .expect("prepare credential slot allocation");
        persistence
            .transition_model_config_mutation_journal(
                journal_id,
                ModelConfigMutationJournalState::Prepared,
                ModelConfigMutationJournalState::ConfigPublished,
                None,
            )
            .expect("publish credential slot config");
        persistence
            .commit_model_config_mutation_metadata(journal_id)
            .expect("commit credential slot metadata");
        persistence
            .transition_model_config_mutation_journal(
                journal_id,
                ModelConfigMutationJournalState::MetadataCommitted,
                ModelConfigMutationJournalState::CleanupPending,
                None,
            )
            .expect("slot cleanup pending");
        persistence
            .transition_model_config_mutation_journal(
                journal_id,
                ModelConfigMutationJournalState::CleanupPending,
                ModelConfigMutationJournalState::Completed,
                None,
            )
            .expect("slot allocation completed");
    }

    fn delete_payload(
        parent_journal_id: MutationJournalId,
        credentials: &[(CredentialId, MutationJournalId)],
    ) -> ModelConfigMutationJournalPayload {
        let mut frozen =
            payload(parent_journal_id, CommandInvocationId::new(), CorrelationId::new());
        frozen.credential_children = credentials
            .iter()
            .map(|(credential_id, child_journal_id)| ModelConfigCredentialChildRef {
                credential_id: *credential_id,
                child_journal_id: *child_journal_id,
                phase: ModelConfigCredentialChildPhase::AfterConfigPublish,
                operation: ModelConfigCredentialChildOperation::Delete,
            })
            .collect();
        frozen
    }

    #[test]
    fn child_phase_controls_the_only_valid_parent_paths() {
        assert!(valid_model_config_journal_transition(
            ModelConfigMutationJournalState::Prepared,
            ModelConfigMutationJournalState::CredentialStaged,
            Some(ModelConfigCredentialChildPhase::BeforeConfigPublish),
        ));
        assert!(!valid_model_config_journal_transition(
            ModelConfigMutationJournalState::Prepared,
            ModelConfigMutationJournalState::CredentialStaged,
            Some(ModelConfigCredentialChildPhase::AfterConfigPublish),
        ));
        assert!(valid_model_config_journal_transition(
            ModelConfigMutationJournalState::Prepared,
            ModelConfigMutationJournalState::ConfigPublished,
            Some(ModelConfigCredentialChildPhase::AfterConfigPublish),
        ));
        assert!(!valid_model_config_journal_transition(
            ModelConfigMutationJournalState::ConfigPublished,
            ModelConfigMutationJournalState::MetadataCommitted,
            Some(ModelConfigCredentialChildPhase::AfterConfigPublish),
        ));
    }

    #[test]
    fn command_idempotency_returns_the_original_parent_and_metadata_commits_once() {
        let (_directory, persistence) = start();
        let command_invocation_id = CommandInvocationId::new();
        let correlation_id = CorrelationId::new();
        let first_id = MutationJournalId::new();
        let first = persistence
            .prepare_model_config_mutation_journal(
                first_id,
                payload(first_id, command_invocation_id, correlation_id),
            )
            .expect("prepare parent");
        assert!(matches!(first, PrepareModelConfigMutationJournalResult::Prepared(_)));

        let duplicate_id = MutationJournalId::new();
        let duplicate = persistence
            .prepare_model_config_mutation_journal(
                duplicate_id,
                payload(duplicate_id, command_invocation_id, correlation_id),
            )
            .expect("load idempotent parent");
        let PrepareModelConfigMutationJournalResult::Existing(existing) = duplicate else {
            panic!("duplicate command must resolve to the existing parent")
        };
        assert_eq!(existing.journal_id, first_id);

        persistence
            .transition_model_config_mutation_journal(
                first_id,
                ModelConfigMutationJournalState::Prepared,
                ModelConfigMutationJournalState::ConfigPublished,
                None,
            )
            .expect("publish config");
        let committed =
            persistence.commit_model_config_mutation_metadata(first_id).expect("commit metadata");
        assert_eq!(committed.event_row_ids.len(), 1);
        assert_eq!(committed.projection_outbox_ids.len(), 1);
        assert_eq!(committed.journal.state, ModelConfigMutationJournalState::MetadataCommitted);
    }

    #[test]
    fn legacy_before_publish_parent_forwards_from_credential_completed_and_restarts_idempotently() {
        let (directory, persistence) = start();
        let database_path = directory.path().join("data/aura.db");
        let artifact_root = directory.path().join("data/artifacts");
        let parent_journal_id = MutationJournalId::new();
        let child_journal_id = MutationJournalId::new();
        let credential_id = CredentialId::new();
        let endpoint_id = EndpointId::new();
        let mut frozen =
            payload(parent_journal_id, CommandInvocationId::new(), CorrelationId::new());
        frozen.credential_children = vec![ModelConfigCredentialChildRef {
            credential_id,
            child_journal_id,
            phase: ModelConfigCredentialChildPhase::BeforeConfigPublish,
            operation: ModelConfigCredentialChildOperation::Configure,
        }];
        frozen.credential_slot_creations = vec![credential_slot(credential_id, endpoint_id)];
        frozen.projection_drafts = credential_projection_plan(
            endpoint_id,
            Revision::new(2),
            EndpointCredentialStatus {
                endpoint_id,
                slot_name: ModelCredentialSlot::ApiKey,
                credential_id,
                state: EndpointCredentialState::Configured,
                active_credential_revision: Some(CredentialRevision::INITIAL),
                last_committed_credential_revision: Some(CredentialRevision::INITIAL),
                row_revision: Revision::new(2),
                error_code: None,
            },
        );
        persistence
            .prepare_model_config_with_credential_journals(
                parent_journal_id,
                frozen,
                child_journal_id,
            )
            .expect("prepare parent and child");
        persistence
            .transition_credential_mutation_journal(
                child_journal_id,
                CredentialMutationJournalState::Prepared,
                CredentialMutationJournalState::Staged,
            )
            .expect("stage child");
        persistence
            .transition_credential_mutation_journal(
                child_journal_id,
                CredentialMutationJournalState::Staged,
                CredentialMutationJournalState::FinalCredentialWritten,
            )
            .expect("write final credential");
        persistence
            .transition_model_config_mutation_journal(
                parent_journal_id,
                ModelConfigMutationJournalState::Prepared,
                ModelConfigMutationJournalState::CredentialStaged,
                None,
            )
            .expect("stage parent credential");
        persistence
            .transition_model_config_mutation_journal(
                parent_journal_id,
                ModelConfigMutationJournalState::CredentialStaged,
                ModelConfigMutationJournalState::ConfigPublished,
                None,
            )
            .expect("publish config");
        persistence
            .transition_model_config_mutation_journal(
                parent_journal_id,
                ModelConfigMutationJournalState::ConfigPublished,
                ModelConfigMutationJournalState::CredentialMutationCompleted,
                None,
            )
            .expect("persist credential commit point");
        drop(persistence);

        let connection = rusqlite::Connection::open(&database_path).expect("open crash database");
        let payload_json = connection
            .query_row(
                "SELECT payload_json FROM mutation_journals WHERE journal_id=?1",
                [parent_journal_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("read parent payload");
        let mut legacy: serde_json::Value =
            serde_json::from_str(&payload_json).expect("decode parent payload");
        legacy["schema_version"] = serde_json::Value::from(1);
        let drafts = legacy["projection_drafts"].as_array_mut().expect("projection drafts");
        let endpoint = drafts
            .iter_mut()
            .find(|draft| draft["projection_kind"] == "endpoint_detail")
            .and_then(serde_json::Value::as_object_mut)
            .and_then(|draft| draft.get_mut("payload"))
            .and_then(serde_json::Value::as_object_mut)
            .expect("endpoint detail payload");
        endpoint.insert("latest_availability_test".to_owned(), serde_json::Value::Null);
        connection
            .execute(
                "UPDATE mutation_journals SET payload_json=?1 WHERE journal_id=?2",
                rusqlite::params![
                    serde_json::to_string(&legacy).expect("encode legacy payload"),
                    parent_journal_id.to_string(),
                ],
            )
            .expect("persist synthetic legacy payload");
        drop(connection);

        let recovered = match PersistenceCoordinator::start(PersistenceOptions::new(
            database_path.clone(),
            artifact_root.clone(),
            RuntimeInstanceId::new(),
        ))
        .expect("restart persistence")
        {
            PersistenceStartup::Ready(coordinator) => coordinator,
            PersistenceStartup::RecoveryRequired(recovery) => {
                panic!("unexpected recovery requirement: {}", recovery.reason)
            }
        };
        let incomplete =
            recovered.incomplete_model_config_mutation_journals().expect("decode migrated parent");
        assert_eq!(incomplete.len(), 1);
        assert_eq!(
            incomplete[0].payload.schema_version,
            MODEL_CONFIG_MUTATION_JOURNAL_SCHEMA_VERSION
        );
        recovered.adopt_model_config_mutation_journal(parent_journal_id).expect("adopt parent");
        recovered.adopt_credential_mutation_journal(child_journal_id).expect("adopt child");
        let committed = recovered
            .commit_model_config_with_credential_metadata(parent_journal_id)
            .expect("forward metadata atomically");
        assert_eq!(committed.parent.state, ModelConfigMutationJournalState::MetadataCommitted);
        assert_eq!(
            committed.credential_child.state,
            CredentialMutationJournalState::MetadataCommitted
        );
        assert_eq!(committed.projection_changes.len(), 3);
        recovered
            .transition_credential_mutation_journal(
                child_journal_id,
                CredentialMutationJournalState::MetadataCommitted,
                CredentialMutationJournalState::CleanupPending,
            )
            .expect("child cleanup pending");
        recovered
            .transition_credential_mutation_journal(
                child_journal_id,
                CredentialMutationJournalState::CleanupPending,
                CredentialMutationJournalState::Completed,
            )
            .expect("child completed");
        recovered
            .transition_model_config_mutation_journal(
                parent_journal_id,
                ModelConfigMutationJournalState::MetadataCommitted,
                ModelConfigMutationJournalState::CleanupPending,
                None,
            )
            .expect("parent cleanup pending");
        recovered
            .transition_model_config_mutation_journal(
                parent_journal_id,
                ModelConfigMutationJournalState::CleanupPending,
                ModelConfigMutationJournalState::Completed,
                None,
            )
            .expect("parent completed");
        drop(recovered);

        let restarted = match PersistenceCoordinator::start(PersistenceOptions::new(
            database_path,
            artifact_root,
            RuntimeInstanceId::new(),
        ))
        .expect("restart completed persistence")
        {
            PersistenceStartup::Ready(coordinator) => coordinator,
            PersistenceStartup::RecoveryRequired(recovery) => {
                panic!("unexpected repeated recovery: {}", recovery.reason)
            }
        };
        assert!(
            restarted
                .incomplete_model_config_mutation_journals()
                .expect("read completed parents")
                .is_empty()
        );
        assert!(
            restarted
                .incomplete_credential_mutation_journals()
                .expect("read completed children")
                .is_empty()
        );
    }

    #[test]
    fn credential_slot_becomes_visible_only_with_parent_metadata_commit() {
        let (_directory, persistence) = start();
        let journal_id = MutationJournalId::new();
        let credential_id = CredentialId::new();
        let endpoint_id = EndpointId::new();
        let mut frozen = payload(journal_id, CommandInvocationId::new(), CorrelationId::new());
        frozen.credential_slot_creations.push(credential_slot(credential_id, endpoint_id));
        persistence
            .prepare_model_config_mutation_journal(journal_id, frozen)
            .expect("prepare parent with slot allocation");
        persistence
            .transition_model_config_mutation_journal(
                journal_id,
                ModelConfigMutationJournalState::Prepared,
                ModelConfigMutationJournalState::ConfigPublished,
                None,
            )
            .expect("publish config");

        assert!(
            persistence
                .model_credential_record(credential_id)
                .expect("query hidden credential slot")
                .is_none()
        );

        persistence
            .commit_model_config_mutation_metadata(journal_id)
            .expect("commit slot, event and projection atomically");
        let record = persistence
            .model_credential_record(credential_id)
            .expect("query committed credential slot")
            .expect("credential slot exists after visibility commit");
        assert_eq!(record.endpoint_id, endpoint_id);
        assert_eq!(record.state, ModelCredentialRecordState::Unconfigured);
        assert_eq!(record.row_revision, Revision::INITIAL);
    }

    #[test]
    fn after_publish_delete_children_are_sorted_non_resolvable_and_tombstoned_without_events() {
        let (directory, persistence) = start();
        let mut subjects =
            [(CredentialId::new(), EndpointId::new()), (CredentialId::new(), EndpointId::new())];
        subjects.sort_by_key(|(credential_id, _)| *credential_id);
        commit_credential_slots(
            &persistence,
            subjects
                .iter()
                .map(|(credential_id, endpoint_id)| credential_slot(*credential_id, *endpoint_id))
                .collect(),
        );

        let parent_journal_id = MutationJournalId::new();
        let children = subjects
            .iter()
            .map(|(credential_id, _)| (*credential_id, MutationJournalId::new()))
            .collect::<Vec<_>>();
        let prepared = persistence
            .prepare_model_config_with_credential_delete_journals(
                parent_journal_id,
                delete_payload(parent_journal_id, &children),
                ModelCredentialDeletionReason::ProviderDeleted,
            )
            .expect("prepare frozen delete child set");
        let PrepareModelConfigCredentialDeleteJournalsResult::Prepared(prepared) = prepared else {
            panic!("new delete command must prepare one parent and all children")
        };
        assert_eq!(
            prepared
                .parent
                .payload
                .credential_children
                .iter()
                .map(|child| child.credential_id)
                .collect::<Vec<_>>(),
            subjects.iter().map(|(id, _)| *id).collect::<Vec<_>>()
        );

        persistence
            .transition_model_config_mutation_journal(
                parent_journal_id,
                ModelConfigMutationJournalState::Prepared,
                ModelConfigMutationJournalState::ConfigPublished,
                None,
            )
            .expect("models deletion published");
        let staged = persistence
            .begin_model_config_credential_deletes(parent_journal_id)
            .expect("make every deleted credential non-resolvable atomically");
        assert_eq!(staged.len(), 2);
        for ((credential_id, endpoint_id), child) in subjects.iter().zip(staged) {
            assert_eq!(child.credential_id, *credential_id);
            assert_eq!(child.state, CredentialMutationJournalState::Staged);
            let pending = persistence
                .model_credential_record(*credential_id)
                .expect("read deletion-pending record")
                .expect("record remains until secret cleanup completes");
            assert_eq!(pending.endpoint_id, *endpoint_id);
            assert_eq!(pending.state, ModelCredentialRecordState::DeletionPending);
            assert_eq!(pending.state.renderer_state(), None);

            persistence
                .transition_credential_mutation_journal(
                    child.journal_id,
                    CredentialMutationJournalState::Staged,
                    CredentialMutationJournalState::FinalCredentialWritten,
                )
                .expect("native credential deletion succeeded");
            let committed = persistence
                .commit_model_credential_delete_metadata(child.journal_id)
                .expect("write tombstone and delete record atomically");
            assert_eq!(committed.tombstone.credential_id, *credential_id);
            assert_eq!(committed.tombstone.former_owner.endpoint_id, *endpoint_id);
            assert_eq!(
                committed.tombstone.deletion_reason,
                ModelCredentialDeletionReason::ProviderDeleted
            );
            assert_eq!(committed.tombstone.parent_journal_id, parent_journal_id);
            assert_eq!(committed.tombstone.child_journal_id, child.journal_id);
            assert_eq!(committed.tombstone.last_committed_credential_revision, None);
            assert!(
                persistence
                    .model_credential_record(*credential_id)
                    .expect("credential record after tombstone")
                    .is_none()
            );
            persistence
                .transition_credential_mutation_journal(
                    child.journal_id,
                    CredentialMutationJournalState::MetadataCommitted,
                    CredentialMutationJournalState::CleanupPending,
                )
                .expect("child cleanup pending");
            persistence
                .transition_credential_mutation_journal(
                    child.journal_id,
                    CredentialMutationJournalState::CleanupPending,
                    CredentialMutationJournalState::Completed,
                )
                .expect("delete child completed");
        }

        persistence
            .transition_model_config_mutation_journal(
                parent_journal_id,
                ModelConfigMutationJournalState::ConfigPublished,
                ModelConfigMutationJournalState::CredentialMutationCompleted,
                None,
            )
            .expect("all credential deletes completed");
        persistence
            .commit_model_config_mutation_metadata(parent_journal_id)
            .expect("commit public endpoint/provider deletion facts");

        drop(persistence);
        let connection = rusqlite::Connection::open(directory.path().join("data/aura.db"))
            .expect("open completed delete database");
        let public_credential_delete_events = connection
            .query_row(
                "SELECT COUNT(*) FROM events
                 WHERE event_type IN ('CredentialDeleted', 'ModelEndpointCredentialDeleted')",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("query forbidden public credential delete events");
        assert_eq!(public_credential_delete_events, 0);
        let tombstone_count = connection
            .query_row("SELECT COUNT(*) FROM credential_tombstones", [], |row| row.get::<_, i64>(0))
            .expect("query credential tombstones");
        assert_eq!(tombstone_count, 2);
    }

    #[test]
    fn any_credential_lease_conflict_aborts_the_entire_delete_preparation() {
        let (_directory, persistence) = start();
        let mut subjects = vec![
            (CredentialId::new(), EndpointId::new()),
            (CredentialId::new(), EndpointId::new()),
        ];
        subjects.sort_by_key(|(credential_id, _)| *credential_id);
        commit_credential_slots(
            &persistence,
            subjects
                .iter()
                .map(|(credential_id, endpoint_id)| credential_slot(*credential_id, *endpoint_id))
                .collect(),
        );
        let (leased_credential_id, leased_endpoint_id) = subjects[1];
        let lease = persistence
            .prepare_model_credential_write(PrepareModelCredentialWrite {
                command_invocation_id: CommandInvocationId::new(),
                endpoint_id: leased_endpoint_id,
                credential_id: leased_credential_id,
                credential_slot: ModelCredentialSlot::ApiKey,
                expected_row_revision: ExpectedRevision::new(1),
                intent: ModelCredentialWriteIntent::Configure,
                binding: credential_slot(leased_credential_id, leased_endpoint_id).binding,
                models_revision: Revision::new(2),
                correlation_id: CorrelationId::new(),
            })
            .expect("hold one credential mutation lease");
        assert!(matches!(lease, PrepareModelCredentialWriteResult::Prepared(_)));

        let parent_journal_id = MutationJournalId::new();
        let children = subjects
            .iter()
            .map(|(credential_id, _)| (*credential_id, MutationJournalId::new()))
            .collect::<Vec<_>>();
        assert!(
            persistence
                .prepare_model_config_with_credential_delete_journals(
                    parent_journal_id,
                    delete_payload(parent_journal_id, &children),
                    ModelCredentialDeletionReason::ProviderDeleted,
                )
                .is_err()
        );
        assert!(
            persistence
                .model_config_mutation_journal(parent_journal_id)
                .expect("query rolled-back parent")
                .is_none()
        );
        for (_, child_journal_id) in children {
            assert!(
                persistence
                    .credential_mutation_journal(child_journal_id)
                    .expect("query rolled-back child")
                    .is_none()
            );
        }
        for (credential_id, _) in subjects {
            assert_eq!(
                persistence
                    .model_credential_record(credential_id)
                    .expect("query preserved credential")
                    .expect("credential remains after aborted delete")
                    .state,
                ModelCredentialRecordState::Unconfigured
            );
        }
    }
}
