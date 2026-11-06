use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use aura_config::{PreparedConfigBundle, SettingsFacade, SettingsFacadeError};
use aura_domain_common::{
    CommandInvocationId, CorrelationId, ErrorCode, MutationJournalId, Revision,
};
use aura_persistence::{
    CreateModelCredentialSlot, CredentialMutationJournalState,
    MODEL_CONFIG_MUTATION_JOURNAL_SCHEMA_VERSION, ModelConfigCredentialChildOperation,
    ModelConfigCredentialChildPhase, ModelConfigCredentialChildRef, ModelConfigMetadataCommit,
    ModelConfigMutationJournalPayload, ModelConfigMutationJournalRecord,
    ModelConfigMutationJournalState, ModelCredentialDeletionReason, ModelProjectionDraft,
    PersistenceCoordinator, PersistenceError, PersistentEventDraft,
    PrepareModelConfigCredentialDeleteJournalsResult,
    PrepareModelConfigCredentialJournalPairResult, PrepareModelConfigMutationJournalResult,
    PreparedSessionModelBindingClear,
};
use aura_projection_contract::ProjectionChange;
use aura_windows_platform::SecretBytes;
use serde_json::Value;
use thiserror::Error;

use super::projection::{P05ProjectionBuilder, P05ProjectionError};
use crate::p05_credential::{
    CredentialChildPrePublishRecovery, CredentialCleanup, CredentialDeleteChildOutcome,
    P05CredentialJournal, P05CredentialJournalError,
};

/// How long a write waits for another model-config journal to release the lease they share.
const LEASE_WAIT: Duration = Duration::from_secs(2);
const LEASE_RETRY: Duration = Duration::from_millis(20);

/// Prepares a journal, waiting while another model-config journal still holds the lease. That
/// journal already owns its config domains, so its remaining steps never wait for this caller.
fn when_lease_free<T>(
    mut prepare: impl FnMut() -> Result<T, PersistenceError>,
) -> Result<T, PersistenceError> {
    let deadline = Instant::now() + LEASE_WAIT;
    loop {
        match prepare() {
            Err(PersistenceError::MutationLeaseConflict) if Instant::now() < deadline => {
                thread::sleep(LEASE_RETRY);
            }
            result => return result,
        }
    }
}

/// The bundle a command reserved under its journal id before that journal exists.
///
/// The reservation claims config domains in memory only, and no durable record will finish or
/// abort it until the journal with this id is prepared. Dropping the guard first ends it, so a
/// command that fails, changes nothing or meets another request's journal for the same command
/// leaves no domain claimed behind it.
struct UnownedBundle<'a> {
    settings: &'a SettingsFacade,
    journal_id: MutationJournalId,
    owned: bool,
}

impl<'a> UnownedBundle<'a> {
    fn new(settings: &'a SettingsFacade, journal_id: MutationJournalId) -> Self {
        Self { settings, journal_id, owned: false }
    }

    fn owned_by(mut self, journal_id: MutationJournalId) {
        self.owned = journal_id == self.journal_id;
    }
}

impl Drop for UnownedBundle<'_> {
    fn drop(&mut self) {
        if !self.owned {
            let _ = self.settings.abort_unpublished_module_bundle(&self.journal_id.to_string());
        }
    }
}

#[derive(Debug, Error)]
pub enum ModelConfigJournalError {
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(transparent)]
    Settings(#[from] SettingsFacadeError),
    #[error(transparent)]
    ModelConfig(#[from] aura_model_config::ModelConfigError),
    #[error(transparent)]
    Projection(#[from] P05ProjectionError),
    #[error("model config journal payload is invalid: {0}")]
    InvalidPayload(#[from] serde_json::Error),
    #[error("model config journal entered reconciliation_required: {0}")]
    ReconciliationRequired(MutationJournalId),
    #[error("model config journal was aborted: {0}")]
    Aborted(MutationJournalId),
    #[error("credential-bearing model config journal requires its dedicated coordinator")]
    CredentialChildRequiresDedicatedCoordinator,
    #[error(transparent)]
    Credential(#[from] P05CredentialJournalError),
}

pub enum ModelConfigCommandPlan {
    NoChange {
        success_result: Value,
    },
    Publish {
        prepared_bundle: Box<PreparedConfigBundle>,
        expected_models_revision: Revision,
        reserved_models_revision: Option<Revision>,
        expected_aura_revision: Option<Revision>,
        reserved_aura_revision: Option<Revision>,
        credential_slot_creations: Vec<CreateModelCredentialSlot>,
        session_binding_clears: Vec<PreparedSessionModelBindingClear>,
        metadata_events: Vec<PersistentEventDraft>,
        projection_drafts: Vec<ModelProjectionDraft>,
        success_result: Value,
    },
}

pub struct ModelConfigCredentialCommandPlan {
    pub prepared_bundle: Box<PreparedConfigBundle>,
    pub expected_models_revision: Revision,
    pub reserved_models_revision: Revision,
    pub credential_slot_creation: CreateModelCredentialSlot,
    pub metadata_events: Vec<PersistentEventDraft>,
    pub projection_drafts: Vec<ModelProjectionDraft>,
    pub success_result: Value,
}

pub struct ModelConfigCredentialDeleteCommandPlan {
    pub prepared_bundle: Box<PreparedConfigBundle>,
    pub expected_models_revision: Revision,
    pub reserved_models_revision: Revision,
    pub expected_aura_revision: Option<Revision>,
    pub reserved_aura_revision: Option<Revision>,
    pub credential_ids: Vec<aura_domain_common::CredentialId>,
    pub deletion_reason: ModelCredentialDeletionReason,
    pub session_binding_clears: Vec<PreparedSessionModelBindingClear>,
    pub metadata_events: Vec<PersistentEventDraft>,
    pub projection_drafts: Vec<ModelProjectionDraft>,
    pub success_result: Value,
}

#[derive(Clone, Debug)]
pub struct ModelConfigCommandCompletion {
    pub success_result: Value,
    pub projection_changes: Vec<ProjectionChange>,
}

/// Coordinates a non-secret P05 config mutation across the file bundle and
/// metadata/outbox transaction. It may allocate non-secret credential-slot
/// metadata at the Runtime Visibility Commit Point. Secret-bearing endpoint
/// creation uses the same Parent Journal through the credential-aware
/// coordinator, not this narrow path.
#[derive(Clone)]
pub struct ModelConfigJournalCoordinator {
    persistence: Arc<PersistenceCoordinator>,
    settings: Arc<SettingsFacade>,
    projection_builder: Option<P05ProjectionBuilder>,
}

impl ModelConfigJournalCoordinator {
    #[must_use]
    pub fn new(persistence: Arc<PersistenceCoordinator>, settings: Arc<SettingsFacade>) -> Self {
        Self { persistence, settings, projection_builder: None }
    }

    #[must_use]
    pub fn with_projection_builder(mut self, projection_builder: P05ProjectionBuilder) -> Self {
        self.projection_builder = Some(projection_builder);
        self
    }

    fn complete_projection_drafts(
        &self,
        drafts: &mut Vec<ModelProjectionDraft>,
    ) -> Result<(), ModelConfigJournalError> {
        if let Some(builder) = &self.projection_builder {
            builder.complete_model_config_projection_drafts(drafts)?;
        }
        Ok(())
    }

    pub fn execute<F>(
        &self,
        command_invocation_id: CommandInvocationId,
        correlation_id: CorrelationId,
        prepare: F,
    ) -> Result<ModelConfigCommandCompletion, ModelConfigJournalError>
    where
        F: FnOnce(MutationJournalId) -> Result<ModelConfigCommandPlan, ModelConfigJournalError>,
    {
        if let Some(existing) =
            self.persistence.model_config_mutation_journal_by_command(command_invocation_id)?
        {
            return self.resume(existing);
        }

        let journal_id = MutationJournalId::new();
        let unowned = UnownedBundle::new(&self.settings, journal_id);
        let mut plan = prepare(journal_id)?;
        if let ModelConfigCommandPlan::Publish { projection_drafts, .. } = &mut plan {
            self.complete_projection_drafts(projection_drafts)?;
        }
        let ModelConfigCommandPlan::Publish {
            prepared_bundle,
            expected_models_revision,
            reserved_models_revision,
            expected_aura_revision,
            reserved_aura_revision,
            credential_slot_creations,
            session_binding_clears,
            metadata_events,
            projection_drafts,
            success_result,
        } = plan
        else {
            let ModelConfigCommandPlan::NoChange { success_result } = plan else {
                unreachable!("model config command plan has only two variants")
            };
            return Ok(ModelConfigCommandCompletion {
                success_result,
                projection_changes: Vec::new(),
            });
        };

        let payload = ModelConfigMutationJournalPayload {
            schema_version: MODEL_CONFIG_MUTATION_JOURNAL_SCHEMA_VERSION,
            command_invocation_id,
            correlation_id,
            prepared_config_bundle: serde_json::to_value(prepared_bundle)?,
            expected_models_revision,
            reserved_models_revision,
            expected_aura_revision,
            reserved_aura_revision,
            credential_children: Vec::new(),
            credential_slot_creations,
            session_binding_clears,
            metadata_events,
            projection_drafts,
            success_result,
        };
        let prepared = when_lease_free(|| {
            self.persistence.prepare_model_config_mutation_journal(journal_id, payload.clone())
        })?;
        let record = match prepared {
            PrepareModelConfigMutationJournalResult::Prepared(record)
            | PrepareModelConfigMutationJournalResult::Existing(record) => *record,
        };
        unowned.owned_by(record.journal_id);
        self.resume(record)
    }

    pub fn execute_with_credential<F>(
        &self,
        credential_journal: &P05CredentialJournal,
        command_invocation_id: CommandInvocationId,
        correlation_id: CorrelationId,
        secret: &SecretBytes,
        prepare: F,
    ) -> Result<ModelConfigCommandCompletion, ModelConfigJournalError>
    where
        F: FnOnce(
            MutationJournalId,
            MutationJournalId,
        ) -> Result<ModelConfigCredentialCommandPlan, ModelConfigJournalError>,
    {
        if let Some(existing) =
            self.persistence.model_config_mutation_journal_by_command(command_invocation_id)?
        {
            return self.resume_with_credential(existing, credential_journal, Some(secret));
        }

        let parent_journal_id = MutationJournalId::new();
        let credential_child_journal_id = MutationJournalId::new();
        let unowned = UnownedBundle::new(&self.settings, parent_journal_id);
        let mut plan = prepare(parent_journal_id, credential_child_journal_id)?;
        self.complete_projection_drafts(&mut plan.projection_drafts)?;
        let payload = ModelConfigMutationJournalPayload {
            schema_version: MODEL_CONFIG_MUTATION_JOURNAL_SCHEMA_VERSION,
            command_invocation_id,
            correlation_id,
            prepared_config_bundle: serde_json::to_value(plan.prepared_bundle)?,
            expected_models_revision: plan.expected_models_revision,
            reserved_models_revision: Some(plan.reserved_models_revision),
            expected_aura_revision: None,
            reserved_aura_revision: None,
            credential_children: vec![ModelConfigCredentialChildRef {
                credential_id: plan.credential_slot_creation.credential_id,
                child_journal_id: credential_child_journal_id,
                phase: ModelConfigCredentialChildPhase::BeforeConfigPublish,
                operation: ModelConfigCredentialChildOperation::Configure,
            }],
            credential_slot_creations: vec![plan.credential_slot_creation],
            session_binding_clears: Vec::new(),
            metadata_events: plan.metadata_events,
            projection_drafts: plan.projection_drafts,
            success_result: plan.success_result,
        };
        let pair = when_lease_free(|| {
            self.persistence.prepare_model_config_with_credential_journals(
                parent_journal_id,
                payload.clone(),
                credential_child_journal_id,
            )
        })?;
        let pair = match pair {
            PrepareModelConfigCredentialJournalPairResult::Prepared(pair)
            | PrepareModelConfigCredentialJournalPairResult::Existing(pair) => pair,
        };
        unowned.owned_by(pair.parent.journal_id);
        self.resume_with_credential(pair.parent, credential_journal, Some(secret))
    }

    pub(crate) fn execute_with_credential_deletes<F>(
        &self,
        credential_journal: &impl CredentialCleanup,
        command_invocation_id: CommandInvocationId,
        correlation_id: CorrelationId,
        prepare: F,
    ) -> Result<ModelConfigCommandCompletion, ModelConfigJournalError>
    where
        F: FnOnce(
            MutationJournalId,
        )
            -> Result<ModelConfigCredentialDeleteCommandPlan, ModelConfigJournalError>,
    {
        if let Some(existing) =
            self.persistence.model_config_mutation_journal_by_command(command_invocation_id)?
        {
            return if existing.payload.credential_child_phase()
                == Some(ModelConfigCredentialChildPhase::AfterConfigPublish)
            {
                self.resume_with_credential_deletes(existing, credential_journal)
            } else {
                self.resume(existing)
            };
        }

        let parent_journal_id = MutationJournalId::new();
        let unowned = UnownedBundle::new(&self.settings, parent_journal_id);
        let mut plan = prepare(parent_journal_id)?;
        self.complete_projection_drafts(&mut plan.projection_drafts)?;
        plan.credential_ids.sort();
        if plan.credential_ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(PersistenceError::InvalidRequest(
                "credential deletion plan contains duplicate CredentialId values".to_owned(),
            )
            .into());
        }
        let credential_children = plan
            .credential_ids
            .iter()
            .copied()
            .map(|credential_id| ModelConfigCredentialChildRef {
                credential_id,
                child_journal_id: MutationJournalId::new(),
                phase: ModelConfigCredentialChildPhase::AfterConfigPublish,
                operation: ModelConfigCredentialChildOperation::Delete,
            })
            .collect::<Vec<_>>();
        let payload = ModelConfigMutationJournalPayload {
            schema_version: MODEL_CONFIG_MUTATION_JOURNAL_SCHEMA_VERSION,
            command_invocation_id,
            correlation_id,
            prepared_config_bundle: serde_json::to_value(plan.prepared_bundle)?,
            expected_models_revision: plan.expected_models_revision,
            reserved_models_revision: Some(plan.reserved_models_revision),
            expected_aura_revision: plan.expected_aura_revision,
            reserved_aura_revision: plan.reserved_aura_revision,
            credential_children,
            credential_slot_creations: Vec::new(),
            session_binding_clears: plan.session_binding_clears,
            metadata_events: plan.metadata_events,
            projection_drafts: plan.projection_drafts,
            success_result: plan.success_result,
        };
        if payload.credential_children.is_empty() {
            let prepared = when_lease_free(|| {
                self.persistence
                    .prepare_model_config_mutation_journal(parent_journal_id, payload.clone())
            })?;
            let record = match prepared {
                PrepareModelConfigMutationJournalResult::Prepared(record)
                | PrepareModelConfigMutationJournalResult::Existing(record) => *record,
            };
            unowned.owned_by(record.journal_id);
            return self.resume(record);
        }
        let prepared = when_lease_free(|| {
            self.persistence.prepare_model_config_with_credential_delete_journals(
                parent_journal_id,
                payload.clone(),
                plan.deletion_reason,
            )
        })?;
        let prepared = match prepared {
            PrepareModelConfigCredentialDeleteJournalsResult::Prepared(prepared)
            | PrepareModelConfigCredentialDeleteJournalsResult::Existing(prepared) => prepared,
        };
        unowned.owned_by(prepared.parent.journal_id);
        self.resume_with_credential_deletes(prepared.parent, credential_journal)
    }

    /// Publishes a deletion, deletes its credentials once it is in effect, and completes it. A
    /// credential Credential Manager still holds then keeps its own delete journal and lease, and
    /// is retried in the background; the deletion does not wait for it under the model-config
    /// lease.
    pub(crate) fn resume_with_credential_deletes(
        &self,
        mut parent: ModelConfigMutationJournalRecord,
        credential_journal: &impl CredentialCleanup,
    ) -> Result<ModelConfigCommandCompletion, ModelConfigJournalError> {
        if parent.payload.credential_child_phase()
            != Some(ModelConfigCredentialChildPhase::AfterConfigPublish)
        {
            return Err(ModelConfigJournalError::CredentialChildRequiresDedicatedCoordinator);
        }
        if parent.state == ModelConfigMutationJournalState::Completed {
            let pending = self.credential_children_open(&parent)?;
            return Ok(ModelConfigCommandCompletion {
                success_result: with_credential_cleanup_pending(
                    parent.payload.success_result,
                    pending,
                ),
                projection_changes: Vec::new(),
            });
        }
        if parent.state == ModelConfigMutationJournalState::Aborted {
            return Err(ModelConfigJournalError::Aborted(parent.journal_id));
        }
        if parent.state == ModelConfigMutationJournalState::ReconciliationRequired {
            return Err(ModelConfigJournalError::ReconciliationRequired(parent.journal_id));
        }
        parent = self.persistence.adopt_model_config_mutation_journal(parent.journal_id)?;
        for child_ref in &parent.payload.credential_children {
            let child = self
                .persistence
                .credential_mutation_journal(child_ref.child_journal_id)?
                .ok_or(PersistenceError::CredentialJournalNotFound)?;
            if !matches!(
                child.state,
                CredentialMutationJournalState::Completed
                    | CredentialMutationJournalState::Aborted
                    | CredentialMutationJournalState::ReconciliationRequired
            ) {
                self.persistence.adopt_credential_mutation_journal(child.journal_id)?;
            }
        }
        let prepared: PreparedConfigBundle =
            serde_json::from_value(parent.payload.prepared_config_bundle.clone())?;
        let mut projection_changes = Vec::new();

        loop {
            match parent.state {
                ModelConfigMutationJournalState::Prepared => {
                    self.settings.resume_prepared_module_bundle(&prepared)?;
                    if let Err(error) = self.settings.publish_prepared_module_bundle(&prepared) {
                        let _ = self.settings.abort_prepared_module_bundle(&prepared);
                        for child_ref in &parent.payload.credential_children {
                            if let Some(child) = self
                                .persistence
                                .credential_mutation_journal(child_ref.child_journal_id)?
                                && child.state == CredentialMutationJournalState::Prepared
                            {
                                let _ = self.persistence.transition_credential_mutation_journal(
                                    child.journal_id,
                                    CredentialMutationJournalState::Prepared,
                                    CredentialMutationJournalState::Aborted,
                                );
                            }
                        }
                        let _ = self.persistence.transition_model_config_mutation_journal(
                            parent.journal_id,
                            ModelConfigMutationJournalState::Prepared,
                            ModelConfigMutationJournalState::Aborted,
                            Some(
                                ErrorCode::new("persistence.model_config_publish_failed")
                                    .expect("static normalized ErrorCode is valid"),
                            ),
                        );
                        return Err(error.into());
                    }
                    parent = self.persistence.transition_model_config_mutation_journal(
                        parent.journal_id,
                        ModelConfigMutationJournalState::Prepared,
                        ModelConfigMutationJournalState::ConfigPublished,
                        None,
                    )?;
                }
                ModelConfigMutationJournalState::ConfigPublished => {
                    let children = self
                        .persistence
                        .begin_model_config_credential_deletes(parent.journal_id)?;
                    for child in children {
                        let _ = credential_journal.delete_published_child(child);
                    }
                    parent = self.persistence.transition_model_config_mutation_journal(
                        parent.journal_id,
                        ModelConfigMutationJournalState::ConfigPublished,
                        ModelConfigMutationJournalState::CredentialMutationCompleted,
                        None,
                    )?;
                }
                ModelConfigMutationJournalState::CredentialMutationCompleted => {
                    let committed = self
                        .persistence
                        .commit_model_config_mutation_metadata(parent.journal_id)?;
                    projection_changes = committed.projection_changes;
                    parent = committed.journal;
                }
                ModelConfigMutationJournalState::MetadataCommitted => {
                    self.settings.resume_prepared_module_bundle(&prepared)?;
                    if let Err(error) = self.settings.activate_published_module_bundle(&prepared) {
                        self.mark_reconciliation_required(&parent)?;
                        return Err(error.into());
                    }
                    parent = self.persistence.transition_model_config_mutation_journal(
                        parent.journal_id,
                        ModelConfigMutationJournalState::MetadataCommitted,
                        ModelConfigMutationJournalState::CleanupPending,
                        None,
                    )?;
                }
                ModelConfigMutationJournalState::CleanupPending => {
                    let mut cleanup_pending = false;
                    let mut reconciliation_required = false;
                    for child_ref in &parent.payload.credential_children {
                        let child = self
                            .persistence
                            .credential_mutation_journal(child_ref.child_journal_id)?
                            .ok_or(PersistenceError::CredentialJournalNotFound)?;
                        let outcome = match credential_journal.delete_published_child(child) {
                            Ok(outcome) => outcome,
                            Err(_) => CredentialDeleteChildOutcome::CleanupPending,
                        };
                        match outcome {
                            CredentialDeleteChildOutcome::Completed => {}
                            CredentialDeleteChildOutcome::CleanupPending => cleanup_pending = true,
                            CredentialDeleteChildOutcome::ReconciliationRequired => {
                                reconciliation_required = true;
                            }
                        }
                    }
                    if reconciliation_required {
                        self.mark_reconciliation_required(&parent)?;
                        return Ok(ModelConfigCommandCompletion {
                            success_result: with_credential_cleanup_pending(
                                parent.payload.success_result,
                                true,
                            ),
                            projection_changes,
                        });
                    }
                    // The settings change is in effect: it completes and gives up the model-config
                    // lease. A credential still there stays with its own delete journal.
                    parent = self.persistence.transition_model_config_mutation_journal(
                        parent.journal_id,
                        ModelConfigMutationJournalState::CleanupPending,
                        ModelConfigMutationJournalState::Completed,
                        None,
                    )?;
                    if cleanup_pending {
                        credential_journal.schedule_retry();
                    }
                    return Ok(ModelConfigCommandCompletion {
                        success_result: with_credential_cleanup_pending(
                            parent.payload.success_result,
                            cleanup_pending,
                        ),
                        projection_changes,
                    });
                }
                ModelConfigMutationJournalState::Completed => {
                    return Ok(ModelConfigCommandCompletion {
                        success_result: with_credential_cleanup_pending(
                            parent.payload.success_result,
                            false,
                        ),
                        projection_changes,
                    });
                }
                ModelConfigMutationJournalState::Aborted => {
                    return Err(ModelConfigJournalError::Aborted(parent.journal_id));
                }
                ModelConfigMutationJournalState::ReconciliationRequired => {
                    return Ok(ModelConfigCommandCompletion {
                        success_result: with_credential_cleanup_pending(
                            parent.payload.success_result,
                            true,
                        ),
                        projection_changes,
                    });
                }
                ModelConfigMutationJournalState::CredentialStaged => {
                    return Err(
                        ModelConfigJournalError::CredentialChildRequiresDedicatedCoordinator,
                    );
                }
            }
        }
    }

    pub fn resume_with_credential(
        &self,
        mut parent: ModelConfigMutationJournalRecord,
        credential_journal: &P05CredentialJournal,
        secret: Option<&SecretBytes>,
    ) -> Result<ModelConfigCommandCompletion, ModelConfigJournalError> {
        let child_id = parent
            .payload
            .credential_children
            .first()
            .map(|child| child.child_journal_id)
            .ok_or(ModelConfigJournalError::CredentialChildRequiresDedicatedCoordinator)?;
        if parent.payload.credential_children.len() != 1
            || parent.payload.credential_child_phase()
                != Some(ModelConfigCredentialChildPhase::BeforeConfigPublish)
        {
            return Err(ModelConfigJournalError::CredentialChildRequiresDedicatedCoordinator);
        }
        if parent.state == ModelConfigMutationJournalState::Completed {
            return Ok(ModelConfigCommandCompletion {
                success_result: parent.payload.success_result,
                projection_changes: Vec::new(),
            });
        }
        if parent.state == ModelConfigMutationJournalState::Aborted {
            return Err(ModelConfigJournalError::Aborted(parent.journal_id));
        }
        if parent.state == ModelConfigMutationJournalState::ReconciliationRequired {
            return Err(ModelConfigJournalError::ReconciliationRequired(parent.journal_id));
        }

        parent = self.persistence.adopt_model_config_mutation_journal(parent.journal_id)?;
        let mut child = self.persistence.adopt_credential_mutation_journal(child_id)?;
        let prepared: PreparedConfigBundle =
            serde_json::from_value(parent.payload.prepared_config_bundle.clone())?;
        let mut projection_changes = Vec::new();

        loop {
            match parent.state {
                ModelConfigMutationJournalState::Prepared => {
                    if let Some(secret) = secret {
                        match credential_journal.stage_prepared_child(child.clone(), secret) {
                            Ok(staged) => child = staged,
                            Err(error) => {
                                let _ = credential_journal
                                    .abort_child_before_final_commit(child.clone());
                                let _ = self.persistence.transition_model_config_mutation_journal(
                                    parent.journal_id,
                                    ModelConfigMutationJournalState::Prepared,
                                    ModelConfigMutationJournalState::Aborted,
                                    Some(
                                        ErrorCode::new("credential.staging_failed")
                                            .expect("static normalized ErrorCode is valid"),
                                    ),
                                );
                                return Err(error.into());
                            }
                        }
                    } else {
                        match credential_journal
                            .recover_child_before_config_publish(child.clone())?
                        {
                            CredentialChildPrePublishRecovery::Staged(staged) => child = *staged,
                            CredentialChildPrePublishRecovery::Aborted => {
                                parent =
                                    self.persistence.transition_model_config_mutation_journal(
                                        parent.journal_id,
                                        ModelConfigMutationJournalState::Prepared,
                                        ModelConfigMutationJournalState::Aborted,
                                        Some(
                                            ErrorCode::new("credential.input_not_recoverable")
                                                .expect("static normalized ErrorCode is valid"),
                                        ),
                                    )?;
                                return Err(ModelConfigJournalError::Aborted(parent.journal_id));
                            }
                        }
                    }
                    parent = self.persistence.transition_model_config_mutation_journal(
                        parent.journal_id,
                        ModelConfigMutationJournalState::Prepared,
                        ModelConfigMutationJournalState::CredentialStaged,
                        None,
                    )?;
                }
                ModelConfigMutationJournalState::CredentialStaged => {
                    self.settings.resume_prepared_module_bundle(&prepared)?;
                    if let Err(error) = self.settings.publish_prepared_module_bundle(&prepared) {
                        let _ = self.settings.abort_prepared_module_bundle(&prepared);
                        let child_abort =
                            credential_journal.abort_child_before_final_commit(child.clone());
                        if child_abort.is_ok() {
                            let _ = self.persistence.transition_model_config_mutation_journal(
                                parent.journal_id,
                                ModelConfigMutationJournalState::CredentialStaged,
                                ModelConfigMutationJournalState::Aborted,
                                Some(
                                    ErrorCode::new("persistence.model_config_publish_failed")
                                        .expect("static normalized ErrorCode is valid"),
                                ),
                            );
                        } else {
                            self.mark_reconciliation_required(&parent)?;
                        }
                        return Err(error.into());
                    }
                    parent = self.persistence.transition_model_config_mutation_journal(
                        parent.journal_id,
                        ModelConfigMutationJournalState::CredentialStaged,
                        ModelConfigMutationJournalState::ConfigPublished,
                        None,
                    )?;
                }
                ModelConfigMutationJournalState::ConfigPublished => {
                    child = match child.state {
                        CredentialMutationJournalState::Staged => {
                            credential_journal.write_staged_child_to_final(child)?
                        }
                        CredentialMutationJournalState::FinalCredentialWritten => child,
                        _ => {
                            self.mark_reconciliation_required(&parent)?;
                            return Err(ModelConfigJournalError::CredentialChildRequiresDedicatedCoordinator);
                        }
                    };
                    parent = self.persistence.transition_model_config_mutation_journal(
                        parent.journal_id,
                        ModelConfigMutationJournalState::ConfigPublished,
                        ModelConfigMutationJournalState::CredentialMutationCompleted,
                        None,
                    )?;
                }
                ModelConfigMutationJournalState::CredentialMutationCompleted => {
                    let committed = self
                        .persistence
                        .commit_model_config_with_credential_metadata(parent.journal_id)?;
                    projection_changes = committed.projection_changes;
                    parent = committed.parent;
                    child = committed.credential_child;
                }
                ModelConfigMutationJournalState::MetadataCommitted => {
                    self.settings.resume_prepared_module_bundle(&prepared)?;
                    if let Err(error) = self.settings.activate_published_module_bundle(&prepared) {
                        self.mark_reconciliation_required(&parent)?;
                        return Err(error.into());
                    }
                    parent = self.persistence.transition_model_config_mutation_journal(
                        parent.journal_id,
                        ModelConfigMutationJournalState::MetadataCommitted,
                        ModelConfigMutationJournalState::CleanupPending,
                        None,
                    )?;
                }
                ModelConfigMutationJournalState::CleanupPending => {
                    if credential_journal
                        .complete_child_after_external_metadata_commit(child.clone())?
                    {
                        parent = self.persistence.transition_model_config_mutation_journal(
                            parent.journal_id,
                            ModelConfigMutationJournalState::CleanupPending,
                            ModelConfigMutationJournalState::Completed,
                            None,
                        )?;
                    } else {
                        return Ok(ModelConfigCommandCompletion {
                            success_result: parent.payload.success_result,
                            projection_changes,
                        });
                    }
                }
                ModelConfigMutationJournalState::Completed => {
                    return Ok(ModelConfigCommandCompletion {
                        success_result: parent.payload.success_result,
                        projection_changes,
                    });
                }
                ModelConfigMutationJournalState::Aborted => {
                    return Err(ModelConfigJournalError::Aborted(parent.journal_id));
                }
                ModelConfigMutationJournalState::ReconciliationRequired => {
                    return Err(ModelConfigJournalError::ReconciliationRequired(parent.journal_id));
                }
            }
        }
    }

    pub fn resume(
        &self,
        record: ModelConfigMutationJournalRecord,
    ) -> Result<ModelConfigCommandCompletion, ModelConfigJournalError> {
        match record.state {
            ModelConfigMutationJournalState::Completed => {
                return Ok(ModelConfigCommandCompletion {
                    success_result: record.payload.success_result,
                    projection_changes: Vec::new(),
                });
            }
            ModelConfigMutationJournalState::Aborted => {
                return Err(ModelConfigJournalError::Aborted(record.journal_id));
            }
            ModelConfigMutationJournalState::ReconciliationRequired => {
                return Err(ModelConfigJournalError::ReconciliationRequired(record.journal_id));
            }
            ModelConfigMutationJournalState::Prepared
            | ModelConfigMutationJournalState::CredentialStaged
            | ModelConfigMutationJournalState::ConfigPublished
            | ModelConfigMutationJournalState::CredentialMutationCompleted
            | ModelConfigMutationJournalState::MetadataCommitted
            | ModelConfigMutationJournalState::CleanupPending => {}
        }
        let mut record = self.persistence.adopt_model_config_mutation_journal(record.journal_id)?;
        if !record.payload.credential_children.is_empty() {
            return Err(ModelConfigJournalError::CredentialChildRequiresDedicatedCoordinator);
        }
        let prepared: PreparedConfigBundle =
            serde_json::from_value(record.payload.prepared_config_bundle.clone())?;
        let mut projection_changes = Vec::new();

        loop {
            match record.state {
                ModelConfigMutationJournalState::Prepared => {
                    self.settings.resume_prepared_module_bundle(&prepared)?;
                    if let Err(error) = self.settings.publish_prepared_module_bundle(&prepared) {
                        let _ = self.settings.abort_prepared_module_bundle(&prepared);
                        let code = ErrorCode::new("persistence.model_config_publish_failed")
                            .expect("static normalized ErrorCode is valid");
                        let _ = self.persistence.transition_model_config_mutation_journal(
                            record.journal_id,
                            ModelConfigMutationJournalState::Prepared,
                            ModelConfigMutationJournalState::Aborted,
                            Some(code),
                        );
                        return Err(error.into());
                    }
                    record = self.persistence.transition_model_config_mutation_journal(
                        record.journal_id,
                        ModelConfigMutationJournalState::Prepared,
                        ModelConfigMutationJournalState::ConfigPublished,
                        None,
                    )?;
                }
                ModelConfigMutationJournalState::ConfigPublished => {
                    let committed = self
                        .persistence
                        .commit_model_config_mutation_metadata(record.journal_id)?;
                    projection_changes = committed.projection_changes.clone();
                    record = committed.journal;
                }
                ModelConfigMutationJournalState::MetadataCommitted => {
                    self.settings.resume_prepared_module_bundle(&prepared)?;
                    if let Err(error) = self.settings.activate_published_module_bundle(&prepared) {
                        self.mark_reconciliation_required(&record)?;
                        return Err(error.into());
                    }
                    record = self.persistence.transition_model_config_mutation_journal(
                        record.journal_id,
                        ModelConfigMutationJournalState::MetadataCommitted,
                        ModelConfigMutationJournalState::CleanupPending,
                        None,
                    )?;
                }
                ModelConfigMutationJournalState::CleanupPending => {
                    record = self.persistence.transition_model_config_mutation_journal(
                        record.journal_id,
                        ModelConfigMutationJournalState::CleanupPending,
                        ModelConfigMutationJournalState::Completed,
                        None,
                    )?;
                }
                ModelConfigMutationJournalState::Completed => {
                    return Ok(ModelConfigCommandCompletion {
                        success_result: record.payload.success_result,
                        projection_changes,
                    });
                }
                ModelConfigMutationJournalState::Aborted => {
                    return Err(ModelConfigJournalError::Aborted(record.journal_id));
                }
                ModelConfigMutationJournalState::ReconciliationRequired => {
                    return Err(ModelConfigJournalError::ReconciliationRequired(record.journal_id));
                }
                ModelConfigMutationJournalState::CredentialStaged
                | ModelConfigMutationJournalState::CredentialMutationCompleted => {
                    return Err(
                        ModelConfigJournalError::CredentialChildRequiresDedicatedCoordinator,
                    );
                }
            }
        }
    }

    fn mark_reconciliation_required(
        &self,
        record: &ModelConfigMutationJournalRecord,
    ) -> Result<(), ModelConfigJournalError> {
        let code = ErrorCode::new("persistence.model_config_reconciliation_required")
            .expect("static normalized ErrorCode is valid");
        self.persistence.transition_model_config_mutation_journal(
            record.journal_id,
            record.state,
            ModelConfigMutationJournalState::ReconciliationRequired,
            Some(code),
        )?;
        Ok(())
    }
}

impl From<ModelConfigMetadataCommit> for ModelConfigCommandCompletion {
    fn from(commit: ModelConfigMetadataCommit) -> Self {
        Self {
            success_result: commit.journal.payload.success_result,
            projection_changes: commit.projection_changes,
        }
    }
}

impl ModelConfigJournalCoordinator {
    /// Whether a credential this change deletes still has an open delete journal.
    fn credential_children_open(
        &self,
        parent: &ModelConfigMutationJournalRecord,
    ) -> Result<bool, ModelConfigJournalError> {
        for child in &parent.payload.credential_children {
            let open = self
                .persistence
                .credential_mutation_journal(child.child_journal_id)?
                .is_some_and(|journal| {
                    !matches!(
                        journal.state,
                        CredentialMutationJournalState::Completed
                            | CredentialMutationJournalState::Aborted
                            | CredentialMutationJournalState::ReconciliationRequired
                    )
                });
            if open {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn with_credential_cleanup_pending(mut result: Value, pending: bool) -> Value {
    if let Some(value) = result
        .as_object_mut()
        .and_then(|result| result.get_mut("value"))
        .and_then(Value::as_object_mut)
    {
        value.insert("credential_cleanup_pending".to_owned(), Value::Bool(pending));
    }
    result
}

#[cfg(test)]
mod tests;
