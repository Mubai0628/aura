mod resolution;
use std::{path::PathBuf, sync::Arc, thread, time::Duration};

use aura_domain_common::{
    CorrelationId, FileIdentity, RendererInstanceId, Revision, SecretNonce, UtcTimestamp,
    WorkspaceId, WorkspaceLifecycleOperationId,
};
use aura_persistence::{PersistenceError, WorkspaceLifecycleRecord};
use aura_workspace_domain::{
    BeginWorkspaceLifecycleInput, BeginWorkspaceLifecycleResult, CatalogFreshness,
    DiscardConfirmation, EditorBufferLifecycleState, EditorLifecycleTransitionAck,
    IssueDiscardConfirmationInput, PrepareEditorLifecycleTransition,
    ResolveWorkspaceLifecycleInput, WorkspaceLifecycleBlocker, WorkspaceLifecycleOperation,
    WorkspaceLifecyclePreflight, WorkspaceLifecyclePrepareDeliveredInput,
    WorkspaceLifecycleResolution, WorkspaceLifecycleState, WorkspaceLifecycleSummary,
    WorkspaceState, WorkspaceSummary,
};

use super::{WorkspaceRuntime, increment};
use crate::{SecurePathResolver, WorkspaceError, domain_identity};

#[derive(Clone)]
pub(super) struct LifecycleRuntimeEntry {
    record: WorkspaceLifecycleRecord,
    expected_workspace_revision: Revision,
    correlation_id: CorrelationId,
    frozen_relocate_target: Option<FrozenRelocateTarget>,
    discard_confirmation: Option<ActiveDiscardConfirmation>,
    prepare_armed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FrozenRelocateTarget {
    selected_root: String,
    canonical_root: String,
    root_identity: FileIdentity,
}

#[derive(Clone)]
struct ActiveDiscardConfirmation {
    confirmation: DiscardConfirmation,
    renderer_instance_id: RendererInstanceId,
}

enum CommitResult {
    Completed(WorkspaceLifecycleSummary),
    Blocked(WorkspaceLifecycleSummary),
}

impl WorkspaceRuntime {
    pub fn begin_lifecycle(
        self: &Arc<Self>,
        input: BeginWorkspaceLifecycleInput,
        correlation_id: CorrelationId,
    ) -> Result<BeginWorkspaceLifecycleResult, WorkspaceError> {
        let mut lifecycles = self.lifecycle_operations()?;
        if lifecycles.contains_key(&input.workspace_id) {
            return Err(WorkspaceError::LifecycleTransitionInProgress);
        }
        let workspace = self.persistence.workspace(input.workspace_id)?;
        if workspace.revision != input.expected_workspace_revision {
            return Err(WorkspaceError::WorkspaceRevisionConflict {
                expected: input.expected_workspace_revision,
                actual: workspace.revision,
            });
        }
        let frozen_relocate_target = self.validate_begin_operation(&workspace, &input.operation)?;
        self.buffers.begin_lifecycle(input.workspace_id)?;

        let operation_id = WorkspaceLifecycleOperationId::new();
        let ack_deadline_at = timestamp_after(self.options.lifecycle_prepare_ack_timeout)?;
        let record = WorkspaceLifecycleRecord {
            summary: WorkspaceLifecycleSummary {
                approval_follow_up: Vec::new(),
                operation_id,
                workspace_id: input.workspace_id,
                renderer_instance_id: input.renderer_instance_id,
                operation: input.operation.kind(),
                lifecycle_revision: Revision::INITIAL,
                state: WorkspaceLifecycleState::Preparing,
                ack_deadline_at: Some(ack_deadline_at),
                preflight: None,
                error_code: None,
            },
            operation: input.operation,
        };
        if let Err(error) = self.persistence.create_workspace_lifecycle_operation(record.clone()) {
            let _ = self.buffers.cancel_lifecycle(input.workspace_id);
            return Err(error.into());
        }
        lifecycles.insert(
            input.workspace_id,
            LifecycleRuntimeEntry {
                record,
                expected_workspace_revision: input.expected_workspace_revision,
                correlation_id,
                frozen_relocate_target,
                discard_confirmation: None,
                prepare_armed: false,
            },
        );
        drop(lifecycles);
        self.notify_projection_change()?;

        Ok(BeginWorkspaceLifecycleResult {
            operation_id,
            lifecycle_revision: Revision::INITIAL,
            state: WorkspaceLifecycleState::Preparing,
            ack_deadline_at,
        })
    }

    pub fn mark_lifecycle_prepare_delivered(
        self: &Arc<Self>,
        input: WorkspaceLifecyclePrepareDeliveredInput,
    ) -> Result<BeginWorkspaceLifecycleResult, WorkspaceError> {
        let mut lifecycles = self.lifecycle_operations()?;
        let workspace_id = find_workspace_for_operation(&lifecycles, input.operation_id)?;
        let entry = lifecycles.get_mut(&workspace_id).ok_or(WorkspaceError::LifecycleNotFound)?;
        validate_operation_revision_and_renderer(
            entry,
            input.expected_lifecycle_revision,
            input.renderer_instance_id,
        )?;
        if entry.record.summary.state != WorkspaceLifecycleState::Preparing {
            return Err(WorkspaceError::LifecycleTransitionInProgress);
        }
        if entry.prepare_armed {
            return begin_result(&entry.record.summary);
        }
        let ack_deadline_at = timestamp_after(self.options.lifecycle_prepare_ack_timeout)?;
        entry.record.summary.ack_deadline_at = Some(ack_deadline_at);
        self.persistence.update_workspace_lifecycle_operation(entry.record.clone())?;
        entry.prepare_armed = true;
        let result = begin_result(&entry.record.summary)?;
        drop(lifecycles);
        self.notify_projection_change()?;

        let weak = Arc::downgrade(self);
        let timeout = self.options.lifecycle_prepare_ack_timeout;
        let operation_id = input.operation_id;
        let revision = input.expected_lifecycle_revision;
        if let Err(error) =
            thread::Builder::new().name(format!("aura-lifecycle-{operation_id}")).spawn(move || {
                thread::sleep(timeout);
                if let Some(runtime) = weak.upgrade() {
                    let _ = runtime.fail_prepare_timeout(operation_id, revision);
                }
            })
        {
            self.renderer_unavailable(input.renderer_instance_id)?;
            return Err(WorkspaceError::Watcher(error.to_string()));
        }
        Ok(result)
    }

    pub fn prepare_lifecycle_transition(
        &self,
        operation_id: WorkspaceLifecycleOperationId,
    ) -> Result<PrepareEditorLifecycleTransition, WorkspaceError> {
        let lifecycles = self.lifecycle_operations()?;
        let entry = lifecycles
            .values()
            .find(|entry| entry.record.summary.operation_id == operation_id)
            .ok_or(WorkspaceError::LifecycleNotFound)?;
        Ok(PrepareEditorLifecycleTransition {
            operation_id,
            lifecycle_revision: entry.record.summary.lifecycle_revision,
            workspace_id: entry.record.summary.workspace_id,
            operation: entry.record.summary.operation,
            deadline_at: entry
                .record
                .summary
                .ack_deadline_at
                .ok_or(WorkspaceError::EditorStateUnconfirmed)?,
        })
    }

    pub fn acknowledge_lifecycle(
        &self,
        ack: EditorLifecycleTransitionAck,
    ) -> Result<WorkspaceLifecycleSummary, WorkspaceError> {
        let mut lifecycles = self.lifecycle_operations()?;
        let workspace_id = find_workspace_for_operation(&lifecycles, ack.operation_id)?;
        let mut entry =
            lifecycles.remove(&workspace_id).ok_or(WorkspaceError::LifecycleNotFound)?;
        if let Err(error) = validate_ack_envelope(&entry, &ack) {
            lifecycles.insert(workspace_id, entry);
            return Err(error);
        }
        if !entry.prepare_armed {
            lifecycles.insert(workspace_id, entry);
            return Err(WorkspaceError::EditorStateUnconfirmed);
        }
        if deadline_elapsed(entry.record.summary.ack_deadline_at) {
            self.transition_terminal(
                &mut entry,
                WorkspaceLifecycleState::Failed,
                "workspace.editor_prepare_ack_timeout",
            )?;
            self.buffers.cancel_lifecycle(workspace_id)?;
            self.notify_projection_change()?;
            return Err(WorkspaceError::EditorPrepareAckTimeout);
        }

        self.buffers.freeze_lifecycle(workspace_id)?;
        let next_revision = increment(entry.record.summary.lifecycle_revision)?;
        let preflight = self.build_preflight(&entry, &ack.buffers, next_revision)?;
        if preflight.blockers.is_empty() {
            match self.commit_lifecycle(&mut entry, preflight)? {
                CommitResult::Completed(summary) => {
                    self.notify_projection_change()?;
                    Ok(summary)
                }
                CommitResult::Blocked(summary) => {
                    lifecycles.insert(workspace_id, entry);
                    self.notify_projection_change()?;
                    Ok(summary)
                }
            }
        } else {
            self.transition(&mut entry, WorkspaceLifecycleState::Blocked, Some(preflight), None)?;
            let summary = entry.record.summary.clone();
            lifecycles.insert(workspace_id, entry);
            self.notify_projection_change()?;
            Ok(summary)
        }
    }
}

impl WorkspaceRuntime {
    fn build_preflight(
        &self,
        entry: &LifecycleRuntimeEntry,
        renderer_buffers: &[EditorBufferLifecycleState],
        lifecycle_revision: Revision,
    ) -> Result<WorkspaceLifecyclePreflight, WorkspaceError> {
        let workspace_id = entry.record.summary.workspace_id;
        let editor = self.buffers.lifecycle_preflight(workspace_id, renderer_buffers)?;
        let persistent = self.persistence.workspace_blockers(workspace_id)?;
        let deferred = self.persistence.pending_deferred_mutations(workspace_id)?;
        let mut blockers = editor.blockers;
        blockers.extend(deferred.into_iter().map(|intent| {
            WorkspaceLifecycleBlocker::DeferredMutationExists {
                session_id: intent.session_id,
                path: intent.path,
            }
        }));
        if persistent.running_objects > 0 {
            blockers.push(WorkspaceLifecycleBlocker::RuntimeObjectsActive {
                count: persistent.running_objects,
            });
        }
        if persistent.runtime_leases > 0 {
            blockers.push(WorkspaceLifecycleBlocker::RuntimeLeasesActive {
                count: persistent.runtime_leases,
            });
        }
        if persistent.mutation_ownership > 0 {
            blockers.push(WorkspaceLifecycleBlocker::MutationOwnershipActive {
                count: persistent.mutation_ownership,
            });
        }
        if persistent.unfinished_journals > 0 {
            blockers.push(WorkspaceLifecycleBlocker::UnfinishedJournals {
                count: persistent.unfinished_journals,
            });
        }
        Ok(WorkspaceLifecyclePreflight {
            workspace_id,
            operation_id: entry.record.summary.operation_id,
            operation: entry.record.summary.operation,
            lifecycle_revision,
            blockers,
            releasable_editor_states: editor.releasable_editor_states,
        })
    }

    fn validate_discard_confirmation(
        &self,
        entry: &LifecycleRuntimeEntry,
        input: &ResolveWorkspaceLifecycleInput,
    ) -> Result<(), WorkspaceError> {
        let supplied = input
            .discard_confirmation_nonce
            .as_ref()
            .ok_or(WorkspaceError::DiscardConfirmationRequired)?;
        let active = entry
            .discard_confirmation
            .as_ref()
            .ok_or(WorkspaceError::DiscardConfirmationRequired)?;
        if active.renderer_instance_id != input.renderer_instance_id
            || active.confirmation.operation_id != input.operation_id
            || active.confirmation.lifecycle_revision != input.expected_lifecycle_revision
            || active.confirmation.workspace_id != entry.record.summary.workspace_id
        {
            return Err(WorkspaceError::DiscardConfirmationInvalid);
        }
        if UtcTimestamp::now() > active.confirmation.expires_at {
            return Err(WorkspaceError::DiscardConfirmationExpired);
        }
        if !secret_nonce_eq(
            supplied.expose().as_bytes(),
            active.confirmation.confirmation_nonce.expose().as_bytes(),
        ) {
            return Err(WorkspaceError::DiscardConfirmationInvalid);
        }
        if self.buffers.discard_bindings(entry.record.summary.workspace_id)?
            != active.confirmation.affected_buffers
        {
            return Err(WorkspaceError::DiscardConfirmationStale);
        }
        Ok(())
    }

    fn fail_prepare_timeout(
        &self,
        operation_id: WorkspaceLifecycleOperationId,
        expected_revision: Revision,
    ) -> Result<(), WorkspaceError> {
        let mut lifecycles = self.lifecycle_operations()?;
        let Ok(workspace_id) = find_workspace_for_operation(&lifecycles, operation_id) else {
            return Ok(());
        };
        let mut entry =
            lifecycles.remove(&workspace_id).ok_or(WorkspaceError::LifecycleNotFound)?;
        if entry.record.summary.state != WorkspaceLifecycleState::Preparing
            || entry.record.summary.lifecycle_revision != expected_revision
        {
            lifecycles.insert(workspace_id, entry);
            return Ok(());
        }
        self.transition_terminal(
            &mut entry,
            WorkspaceLifecycleState::Failed,
            "workspace.editor_prepare_ack_timeout",
        )?;
        self.buffers.cancel_lifecycle(workspace_id)?;
        self.notify_projection_change()?;
        Ok(())
    }

    fn transition(
        &self,
        entry: &mut LifecycleRuntimeEntry,
        state: WorkspaceLifecycleState,
        preflight: Option<WorkspaceLifecyclePreflight>,
        error_code: Option<String>,
    ) -> Result<(), WorkspaceError> {
        let next = increment(entry.record.summary.lifecycle_revision)?;
        entry.record.summary.lifecycle_revision = next;
        entry.record.summary.state = state;
        if state != WorkspaceLifecycleState::Preparing {
            entry.record.summary.ack_deadline_at = None;
        }
        entry.record.summary.preflight = preflight.map(|value| retag_preflight(value, next));
        entry.record.summary.error_code = error_code;
        entry.discard_confirmation = None;
        self.persistence.update_workspace_lifecycle_operation(entry.record.clone())?;
        Ok(())
    }

    fn transition_terminal(
        &self,
        entry: &mut LifecycleRuntimeEntry,
        state: WorkspaceLifecycleState,
        error_code: &'static str,
    ) -> Result<(), WorkspaceError> {
        debug_assert!(state.is_terminal());
        self.transition(entry, state, None, Some(error_code.to_owned()))
    }

    fn lifecycle_operations(
        &self,
    ) -> Result<
        std::sync::MutexGuard<'_, std::collections::HashMap<WorkspaceId, LifecycleRuntimeEntry>>,
        WorkspaceError,
    > {
        self.lifecycles
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace lifecycle operations"))
    }
}

fn validate_ack_envelope(
    entry: &LifecycleRuntimeEntry,
    ack: &EditorLifecycleTransitionAck,
) -> Result<(), WorkspaceError> {
    if entry.record.summary.state != WorkspaceLifecycleState::Preparing {
        return Err(WorkspaceError::LifecycleTransitionInProgress);
    }
    if entry.record.summary.workspace_id != ack.workspace_id {
        return Err(WorkspaceError::LifecycleNotFound);
    }
    validate_operation_revision_and_renderer(
        entry,
        ack.lifecycle_revision,
        ack.renderer_instance_id,
    )
}

fn validate_operation_revision_and_renderer(
    entry: &LifecycleRuntimeEntry,
    revision: Revision,
    renderer_instance_id: RendererInstanceId,
) -> Result<(), WorkspaceError> {
    if entry.record.summary.lifecycle_revision != revision {
        return Err(WorkspaceError::LifecycleRevisionConflict {
            expected: entry.record.summary.lifecycle_revision,
            actual: revision,
        });
    }
    if entry.record.summary.renderer_instance_id != renderer_instance_id {
        return Err(WorkspaceError::LifecycleRendererMismatch {
            expected: entry.record.summary.renderer_instance_id,
            actual: renderer_instance_id,
        });
    }
    Ok(())
}

fn find_workspace_for_operation(
    lifecycles: &std::collections::HashMap<WorkspaceId, LifecycleRuntimeEntry>,
    operation_id: WorkspaceLifecycleOperationId,
) -> Result<WorkspaceId, WorkspaceError> {
    lifecycles
        .iter()
        .find(|(_, entry)| entry.record.summary.operation_id == operation_id)
        .map(|(workspace_id, _)| *workspace_id)
        .ok_or(WorkspaceError::LifecycleNotFound)
}

fn timestamp_after(duration: Duration) -> Result<UtcTimestamp, WorkspaceError> {
    let delta = time::Duration::try_from(duration)
        .map_err(|error| WorkspaceError::Watcher(error.to_string()))?;
    Ok(UtcTimestamp::from_value(UtcTimestamp::now().value() + delta))
}

fn deadline_elapsed(deadline: Option<UtcTimestamp>) -> bool {
    deadline.is_none_or(|deadline| UtcTimestamp::now() > deadline)
}

fn retag_preflight(
    mut preflight: WorkspaceLifecyclePreflight,
    revision: Revision,
) -> WorkspaceLifecyclePreflight {
    preflight.lifecycle_revision = revision;
    preflight
}

fn begin_result(
    summary: &WorkspaceLifecycleSummary,
) -> Result<BeginWorkspaceLifecycleResult, WorkspaceError> {
    Ok(BeginWorkspaceLifecycleResult {
        operation_id: summary.operation_id,
        lifecycle_revision: summary.lifecycle_revision,
        state: summary.state,
        ack_deadline_at: summary.ack_deadline_at.ok_or(WorkspaceError::EditorStateUnconfirmed)?,
    })
}

fn secret_nonce_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter().zip(right).fold(0_u8, |difference, (left, right)| difference | (left ^ right)) == 0
}

impl WorkspaceRuntime {
    fn commit_lifecycle(
        &self,
        entry: &mut LifecycleRuntimeEntry,
        preflight: WorkspaceLifecyclePreflight,
    ) -> Result<CommitResult, WorkspaceError> {
        let workspace_id = entry.record.summary.workspace_id;
        let current = self.buffers.current_lifecycle_states(workspace_id)?;
        let revision = increment(entry.record.summary.lifecycle_revision)?;
        let final_preflight = self.build_preflight(entry, &current, revision)?;
        if !final_preflight.blockers.is_empty() {
            self.transition(entry, WorkspaceLifecycleState::Blocked, Some(final_preflight), None)?;
            return Ok(CommitResult::Blocked(entry.record.summary.clone()));
        }
        if let Err(error) = self.revalidate_frozen_operation(entry) {
            self.transition_terminal(
                entry,
                WorkspaceLifecycleState::Failed,
                "workspace.lifecycle_target_changed",
            )?;
            self.buffers.cancel_lifecycle(workspace_id)?;
            let _ = error;
            return Ok(CommitResult::Completed(entry.record.summary.clone()));
        }
        self.transition(
            entry,
            WorkspaceLifecycleState::Committing,
            Some(retag_preflight(preflight, revision)),
            None,
        )?;

        let original_workspace = self.persistence.workspace(workspace_id)?;
        if original_workspace.revision != entry.expected_workspace_revision {
            return self.fail_commit(
                entry,
                &original_workspace,
                "workspace.lifecycle_workspace_revision_changed",
            );
        }
        self.remove_online_runtime_state(workspace_id)?;

        let completed_revision = increment(entry.record.summary.lifecycle_revision)?;
        let mut completed = entry.record.clone();
        completed.summary.lifecycle_revision = completed_revision;
        completed.summary.state = WorkspaceLifecycleState::Completed;
        completed.summary.ack_deadline_at = None;
        completed.summary.error_code = None;
        if let Some(preflight) = completed.summary.preflight.as_mut() {
            preflight.lifecycle_revision = completed_revision;
        }

        let persistence_result = match &entry.record.operation {
            WorkspaceLifecycleOperation::Archive => {
                let mut workspace = original_workspace.clone();
                workspace.state = WorkspaceState::Archived;
                workspace.catalog_freshness = CatalogFreshness::Unavailable;
                workspace.revision = increment(workspace.revision)?;
                workspace.authorization_epoch = increment(workspace.authorization_epoch)?;
                self.persistence
                    .commit_workspace_lifecycle(
                        workspace,
                        completed.clone(),
                        "workspace.archived",
                        entry.correlation_id,
                        true,
                    )
                    .map(|_| None)
            }
            WorkspaceLifecycleOperation::Relocate { .. } => {
                let target = entry
                    .frozen_relocate_target
                    .clone()
                    .ok_or(WorkspaceError::LifecycleTargetChanged)?;
                let mut workspace = original_workspace.clone();
                workspace.selected_root = target.selected_root;
                workspace.canonical_root = target.canonical_root;
                workspace.root_identity = Some(target.root_identity);
                workspace.state = WorkspaceState::Online;
                workspace.catalog_freshness = CatalogFreshness::Unavailable;
                workspace.revision = increment(workspace.revision)?;
                workspace.authorization_epoch = increment(workspace.authorization_epoch)?;
                self.persistence
                    .commit_workspace_lifecycle(
                        workspace.clone(),
                        completed.clone(),
                        "workspace.relocated",
                        entry.correlation_id,
                        true,
                    )
                    .map(|_| Some(workspace))
            }
            WorkspaceLifecycleOperation::PermanentDelete { .. } => self
                .persistence
                .permanently_delete_workspace(workspace_id, completed.clone())
                .map(|_| None),
        };
        let relocated_workspace = match persistence_result {
            Ok(workspace) => workspace,
            Err(_) => {
                return self.fail_commit(
                    entry,
                    &original_workspace,
                    "workspace.lifecycle_commit_failed",
                );
            }
        };

        entry.record = completed;
        self.buffers.complete_lifecycle_cleanup(workspace_id)?;
        if matches!(entry.record.operation, WorkspaceLifecycleOperation::Archive) {
            self.clear_archived_index_in_background(vec![workspace_id]);
        }
        if let Some(workspace) = relocated_workspace {
            self.install_online_workspace(workspace, entry.correlation_id)?;
        }
        Ok(CommitResult::Completed(entry.record.summary.clone()))
    }

    fn fail_commit(
        &self,
        entry: &mut LifecycleRuntimeEntry,
        original_workspace: &WorkspaceSummary,
        error_code: &'static str,
    ) -> Result<CommitResult, WorkspaceError> {
        let _ = self.reinstall_after_failed_commit(original_workspace);
        self.transition_terminal(entry, WorkspaceLifecycleState::Failed, error_code)?;
        self.buffers.cancel_lifecycle(entry.record.summary.workspace_id)?;
        Ok(CommitResult::Completed(entry.record.summary.clone()))
    }

    fn reinstall_after_failed_commit(
        &self,
        workspace: &WorkspaceSummary,
    ) -> Result<(), WorkspaceError> {
        if workspace.state == WorkspaceState::Online {
            self.install_online_workspace(workspace.clone(), CorrelationId::new())?;
        }
        Ok(())
    }

    fn validate_begin_operation(
        &self,
        workspace: &WorkspaceSummary,
        operation: &WorkspaceLifecycleOperation,
    ) -> Result<Option<FrozenRelocateTarget>, WorkspaceError> {
        match operation {
            WorkspaceLifecycleOperation::Archive => {
                if workspace.state == WorkspaceState::Archived {
                    return Err(WorkspaceError::Archived);
                }
                Ok(None)
            }
            WorkspaceLifecycleOperation::Relocate {
                selected_target_root,
                confirmed_same_logical_project,
            } => {
                let selected = PathBuf::from(selected_target_root);
                let root = SecurePathResolver::inspect_selected_root(&selected)?;
                let identity = domain_identity(&root.identity);
                if workspace.root_identity.as_ref() != Some(&identity)
                    && !confirmed_same_logical_project
                {
                    return Err(WorkspaceError::RelocateConfirmationRequired);
                }
                let canonical_root = root.final_path.display().to_string();
                if let Some(existing) = self.persistence.find_workspace_by_root(
                    &canonical_root,
                    Some(&identity.volume_serial_number),
                    Some(&identity.file_id),
                )? && existing.workspace_id != workspace.workspace_id
                {
                    return Err(WorkspaceError::Persistence(
                        PersistenceError::WorkspaceRootConflict,
                    ));
                }
                Ok(Some(FrozenRelocateTarget {
                    selected_root: selected.display().to_string(),
                    canonical_root,
                    root_identity: identity,
                }))
            }
            WorkspaceLifecycleOperation::PermanentDelete { .. } => {
                if workspace.state != WorkspaceState::Archived {
                    return Err(WorkspaceError::Busy {
                        reason: "permanent delete requires an archived Workspace".to_owned(),
                    });
                }
                Ok(None)
            }
        }
    }

    fn revalidate_frozen_operation(
        &self,
        entry: &LifecycleRuntimeEntry,
    ) -> Result<(), WorkspaceError> {
        let WorkspaceLifecycleOperation::Relocate { .. } = &entry.record.operation else {
            return Ok(());
        };
        let frozen =
            entry.frozen_relocate_target.as_ref().ok_or(WorkspaceError::LifecycleTargetChanged)?;
        let selected = PathBuf::from(&frozen.selected_root);
        let inspected = SecurePathResolver::inspect_selected_root(&selected)?;
        let current = FrozenRelocateTarget {
            selected_root: selected.display().to_string(),
            canonical_root: inspected.final_path.display().to_string(),
            root_identity: domain_identity(&inspected.identity),
        };
        if current != *frozen {
            return Err(WorkspaceError::LifecycleTargetChanged);
        }
        if let Some(existing) = self.persistence.find_workspace_by_root(
            &current.canonical_root,
            Some(&current.root_identity.volume_serial_number),
            Some(&current.root_identity.file_id),
        )? && existing.workspace_id != entry.record.summary.workspace_id
        {
            return Err(WorkspaceError::Persistence(PersistenceError::WorkspaceRootConflict));
        }
        Ok(())
    }
}

impl WorkspaceRuntime {
    pub fn issue_discard_confirmation(
        &self,
        input: IssueDiscardConfirmationInput,
    ) -> Result<DiscardConfirmation, WorkspaceError> {
        let mut lifecycles = self.lifecycle_operations()?;
        let workspace_id = find_workspace_for_operation(&lifecycles, input.operation_id)?;
        let entry = lifecycles.get_mut(&workspace_id).ok_or(WorkspaceError::LifecycleNotFound)?;
        validate_operation_revision_and_renderer(
            entry,
            input.expected_lifecycle_revision,
            input.renderer_instance_id,
        )?;
        if entry.record.summary.state != WorkspaceLifecycleState::Blocked {
            return Err(WorkspaceError::LifecycleTransitionInProgress);
        }
        let affected_buffers = self.buffers.discard_bindings(workspace_id)?;
        if affected_buffers.is_empty() {
            return Err(WorkspaceError::DiscardConfirmationRequired);
        }

        let mut random = [0_u8; 32];
        getrandom::fill(&mut random)
            .map_err(|error| WorkspaceError::Randomness(error.to_string()))?;
        let nonce = SecretNonce::from_bytes(random);
        let next_revision = increment(entry.record.summary.lifecycle_revision)?;
        entry.record.summary.lifecycle_revision = next_revision;
        entry.record.summary.error_code = None;
        if let Some(preflight) = entry.record.summary.preflight.as_mut() {
            preflight.lifecycle_revision = next_revision;
        }
        self.persistence.update_workspace_lifecycle_operation(entry.record.clone())?;

        let confirmation = DiscardConfirmation {
            confirmation_nonce: nonce,
            operation_id: input.operation_id,
            lifecycle_revision: next_revision,
            workspace_id,
            affected_buffers,
            expires_at: timestamp_after(self.options.discard_confirmation_ttl)?,
        };
        entry.discard_confirmation = Some(ActiveDiscardConfirmation {
            confirmation: confirmation.clone(),
            renderer_instance_id: input.renderer_instance_id,
        });
        self.notify_projection_change()?;
        Ok(confirmation)
    }

    pub fn renderer_unavailable(
        &self,
        renderer_instance_id: RendererInstanceId,
    ) -> Result<Vec<WorkspaceLifecycleSummary>, WorkspaceError> {
        let mut lifecycles = self.lifecycle_operations()?;
        let workspace_ids = lifecycles
            .iter()
            .filter(|(_, entry)| entry.record.summary.renderer_instance_id == renderer_instance_id)
            .map(|(workspace_id, _)| *workspace_id)
            .collect::<Vec<_>>();
        let mut terminal = Vec::with_capacity(workspace_ids.len());
        for workspace_id in workspace_ids {
            let mut entry =
                lifecycles.remove(&workspace_id).ok_or(WorkspaceError::LifecycleNotFound)?;
            let (state, code) = if entry.record.summary.state == WorkspaceLifecycleState::Preparing
            {
                (WorkspaceLifecycleState::Failed, "workspace.editor_state_unconfirmed")
            } else {
                (WorkspaceLifecycleState::Cancelled, "workspace.renderer_unavailable")
            };
            self.transition_terminal(&mut entry, state, code)?;
            self.buffers.cancel_lifecycle(workspace_id)?;
            terminal.push(entry.record.summary);
        }
        if !terminal.is_empty() {
            self.notify_projection_change()?;
        }
        Ok(terminal)
    }

    pub(super) fn ensure_no_active_lifecycle(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<(), WorkspaceError> {
        if self.lifecycle_operations()?.contains_key(&workspace_id) {
            Err(WorkspaceError::LifecycleTransitionInProgress)
        } else {
            Ok(())
        }
    }

    fn reflight_and_commit(
        &self,
        entry: &mut LifecycleRuntimeEntry,
        lifecycles: &mut std::sync::MutexGuard<
            '_,
            std::collections::HashMap<WorkspaceId, LifecycleRuntimeEntry>,
        >,
    ) -> Result<WorkspaceLifecycleSummary, WorkspaceError> {
        let workspace_id = entry.record.summary.workspace_id;
        let current = self.buffers.current_lifecycle_states(workspace_id)?;
        let revision = increment(entry.record.summary.lifecycle_revision)?;
        let preflight = self.build_preflight(entry, &current, revision)?;
        if !preflight.blockers.is_empty() {
            self.transition(entry, WorkspaceLifecycleState::Blocked, Some(preflight), None)?;
            let summary = entry.record.summary.clone();
            lifecycles.insert(workspace_id, entry.clone());
            self.notify_projection_change()?;
            return Ok(summary);
        }
        match self.commit_lifecycle(entry, preflight)? {
            CommitResult::Completed(summary) => {
                self.notify_projection_change()?;
                Ok(summary)
            }
            CommitResult::Blocked(summary) => {
                lifecycles.insert(workspace_id, entry.clone());
                self.notify_projection_change()?;
                Ok(summary)
            }
        }
    }
}
