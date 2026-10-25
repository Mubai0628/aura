use std::str::FromStr;
mod attachment_grants;
pub(crate) mod session_authority;
mod transition;

use aura_artifact_contract::ArtifactScope;
use aura_domain_common::{
    ApprovalDecisionId, ApprovalRequestId, ContentHash, CorrelationId, ExternalGrantId,
    FileIdentity, HostPreparationOperationId, NetworkGrantId, NetworkRequestId, PermissionGrantId,
    PermissionSnapshotId, QuarantineRecordId, SandboxAclJournalId, SandboxExecutionId,
    SecurityOperationId, SessionId, TrustRecordId, TurnTaskId, UtcTimestamp, WorkspaceId,
    WorkspacePath,
};
use aura_permission_domain::{
    ApprovalRequest, ApprovalRequestDraft, ApprovalResolution, ApprovalView,
    ExternalGrantLifecycle, ExternalGrantRecord, ExternalGrantTargetKind, ExternalGrantTerm,
    ExternalProcessLaunchPlan, ExternalTrustRecord, HostPreparationAction,
    HostPreparationBrokerPlan, HostPreparationOperationState, HostPreparationOperationView,
    HostPreparationOwnership, HostPreparationStatus, NetworkEnforcementKind, NetworkGrant,
    NetworkInspectionLevel, NetworkRequestAuditRecord, NetworkRequestAuditState,
    PendingApprovalProjection, PermissionGrant, QuarantineRecordDetails, QuarantineReleaseBinding,
    QuarantineState, SandboxAclJournalEntry, SandboxAclJournalState, SandboxExecutionRecord,
    SandboxExecutionState, SandboxFilesystemAccess, SecurityProjectionPayload,
    SecurityRuntimeProjection, SecuritySource, SessionPermissionProjection, TurnDenyRecord,
};
use aura_security_contract::TrustState;
use aura_security_contract::{
    ApprovalDecisionKind, ApprovalState, CanonicalPath, PermissionGrantLifetime, PermissionMode,
    PermissionSnapshot, ResolvedCapability,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::coordinator::{insert_event, next_projection_revision};
use crate::{
    CommittedProjection, EventActor, PersistenceCoordinator, PersistenceError, PersistentEventDraft,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SecurityStartupReport {
    pub cancelled_approvals: u64,
    pub sandbox_executions_marked_unknown: u64,
    pub network_grants_revoked: u64,
    pub artifact_overwrites_interrupted: u64,
    pub artifact_overwrites_needing_reconcile: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArtifactOverwriteJournalBinding {
    pub operation_id: SecurityOperationId,
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub external_grant_id: ExternalGrantId,
    pub relative_path: WorkspacePath,
    pub final_destination: CanonicalPath,
    pub backup_file_name: String,
    pub expected_target_identity: FileIdentity,
    pub expected_content_hash: ContentHash,
    pub replacement_content_hash: ContentHash,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactOverwriteJournalState {
    Prepared,
    Applying,
    Published,
    Completed,
    Aborted,
    Interrupted,
    NeedsReconcile,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactOverwriteJournalRecord {
    pub binding: ArtifactOverwriteJournalBinding,
    pub state: ArtifactOverwriteJournalState,
    pub error_code: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ApprovalCommit {
    pub resolution: ApprovalResolution,
    pub projection: CommittedProjection<SecurityProjectionPayload>,
}

#[derive(Clone, Debug)]
pub struct HostPreparationJournalRecord {
    pub operation: HostPreparationOperationView,
    pub broker_plan: HostPreparationBrokerPlan,
    pub approval: Option<ApprovalView>,
}

#[derive(Clone, Debug)]
pub struct HostPreparationStoredState {
    pub status: HostPreparationStatus,
    pub ownership: HostPreparationOwnership,
}

/// Whether one prepared Host Preparation plan has passed its own deadline.
///
/// Answered from two indexed reads and nothing else, so a caller can decide whether a
/// settlement is needed *before* paying for a host probe or taking the single writer.
/// The answer is a pre-check, not the decision: the settlement transaction re-reads the
/// operation and refuses unless it is still prepared and still expired, so a plan that
/// was completed, cancelled or replaced in between cannot be settled by this fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostPreparationExpiry {
    pub operation_id: HostPreparationOperationId,
    pub expires_at: UtcTimestamp,
}

#[derive(Clone, Debug)]
pub struct HostPreparationOperationTransition {
    pub operation_id: HostPreparationOperationId,
    pub expected_state: HostPreparationOperationState,
    pub expected_operation_revision: u64,
    pub expected_host_preparation_revision: u64,
    pub next_state: HostPreparationOperationState,
    pub ownership_after: Option<HostPreparationOwnership>,
    pub error_code: Option<String>,
    pub runtime: SecurityRuntimeProjection,
    pub correlation_id: CorrelationId,
}

pub(crate) struct SecurityMutation<T> {
    value: T,
    projections: Vec<CommittedProjection<SecurityProjectionPayload>>,
}

/// One session's mode change, as the transaction body needs it.
///
/// It exists so the synchronous entry ([`PersistenceCoordinator::set_session_permission_mode`]
/// and the blocking commit) and the awaitable entry
/// ([`PersistenceCoordinator::change_session_permission_mode`]) really run **one** body:
/// a second copy of the statement sequence would be the place where a CAS, an
/// invalidation or a projection silently diverges between the two callers.
#[derive(Clone, Copy)]
struct SessionPermissionChange {
    session_id: SessionId,
    expected_revision: u64,
    mode: PermissionMode,
    correlation_id: CorrelationId,
}

/// The one mode-change transaction: CAS on the revision, then the mode, the
/// invalidations, the transition record, the event and the two projections.
fn transact_session_permission_change(
    connection: &mut Connection,
    change: SessionPermissionChange,
) -> Result<SecurityMutation<SessionPermissionProjection>, PersistenceError> {
    let SessionPermissionChange { session_id, expected_revision, mode, correlation_id } = change;
    let transaction = connection.transaction()?;
    let current = load_session_permission_projection(&transaction, session_id)?;
    if current.revision != expected_revision {
        return Err(PersistenceError::PermissionRevisionConflict);
    }
    let revision = expected_revision.checked_add(1).ok_or_else(|| {
        PersistenceError::InvalidRequest("permission revision exhausted".to_owned())
    })?;
    let now = UtcTimestamp::now();
    transaction.execute(
        "UPDATE session_permission_states
         SET mode=?1, revision=?2, updated_at=?3 WHERE session_id=?4",
        params![
            permission_mode_name(mode),
            to_i64(revision, "permission revision")?,
            now.to_string(),
            session_id.to_string()
        ],
    )?;
    let invalidated =
        invalidate_pending_for_session(&transaction, session_id, "permission_mode_changed", now)?;
    let workspace_id = session_workspace_id(&transaction, session_id)?;
    transition::record(
        &transaction,
        session_id,
        workspace_id,
        revision,
        aura_permission_domain::PermissionModeTransition::Settled,
        correlation_id,
    )?;
    let payload = load_session_permission_projection(&transaction, session_id)?;
    let mut event = PersistentEventDraft::system(
        "SessionPermissionModeChanged",
        correlation_id,
        json!({
            "session_id": session_id,
            "mode": mode,
            "revision": revision,
            "invalidated_approval_count": invalidated,
        }),
    );
    event.workspace_id = Some(workspace_id);
    event.session_id = Some(session_id);
    event.actor = EventActor::User;
    insert_event(&transaction, &event)?;
    let session_projection = commit_session_permission_projection(&transaction, payload.clone())?;
    let pending_projection = commit_pending_approval_projection(&transaction)?;
    transaction.commit()?;
    Ok(SecurityMutation {
        value: payload,
        projections: vec![session_projection, pending_projection],
    })
}

/// The identity a session's mode-change write is recorded under.
///
/// Derived from the session, never stored, so a caller that gave up waiting can look the
/// write's terminal state up instead of resending it.
fn permission_write_identity(session_id: SessionId) -> String {
    format!("permission-mode-change:{session_id}")
}

impl<T> SecurityMutation<T> {
    pub(crate) fn into_projections(self) -> Vec<CommittedProjection<SecurityProjectionPayload>> {
        self.projections
    }
}

impl PersistenceCoordinator {
    pub fn reconcile_security_after_core_restart(
        &self,
        correlation_id: CorrelationId,
    ) -> Result<SecurityStartupReport, PersistenceError> {
        let mutation = self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let now = UtcTimestamp::now();
            let mut cancelled = 0_u64;
            for mut request in load_pending_approvals(&transaction)? {
                request.state = ApprovalState::Cancelled;
                request.request_revision =
                    request.request_revision.checked_add(1).ok_or_else(|| {
                        PersistenceError::InvalidRequest("approval revision exhausted".to_owned())
                    })?;
                request.resolved_at = Some(now);
                request.invalidation_reason = Some("core_restarted".to_owned());
                transaction.execute(
                    "UPDATE approval_requests
                     SET request_json=?1, request_revision=?2, state='cancelled',
                         resolved_at=?3, invalidation_reason='core_restarted'
                     WHERE approval_request_id=?4 AND state='pending'",
                    params![
                        serde_json::to_string(&request)?,
                        to_i64(request.request_revision, "approval revision")?,
                        now.to_string(),
                        request.id.to_string()
                    ],
                )?;
                cancelled = cancelled.saturating_add(1);
            }
            let unknown = transaction.execute(
                "UPDATE sandbox_executions
                 SET state='state_unknown', updated_at=?1, terminal_at=?1
                 WHERE state NOT IN (
                    'exited', 'cancelled', 'timed_out', 'policy_violation',
                    'cleanup_failed', 'state_unknown'
                 )",
                [now.to_string()],
            )?;
            let revoked = transaction.execute(
                "UPDATE network_grants SET revoked_at=?1 WHERE revoked_at IS NULL",
                [now.to_string()],
            )?;
            let interrupted_releases = transaction.execute(
                "UPDATE quarantine_release_journal
                 SET state='interrupted', updated_at=?1, error_code='core_restarted'
                 WHERE state IN ('authorized', 'applying', 'published')",
                [now.to_string()],
            )?;
            let interrupted_artifact_overwrites = transaction.execute(
                "UPDATE artifact_overwrite_journal
                 SET state='interrupted', updated_at=?1, error_code='core_restarted_before_apply'
                 WHERE state='prepared'",
                [now.to_string()],
            )?;
            let artifact_overwrites_needing_reconcile = transaction.execute(
                "UPDATE artifact_overwrite_journal
                 SET state='needs_reconcile', updated_at=?1, error_code='core_restarted_external_state_ambiguous'
                 WHERE state IN ('applying', 'published')",
                [now.to_string()],
            )?;
            let event = PersistentEventDraft::system(
                "security.core_restart_reconciled",
                correlation_id,
                json!({
                    "cancelled_approvals": cancelled,
                    "sandbox_executions_marked_unknown": unknown,
                    "network_grants_revoked": revoked,
                    "quarantine_releases_interrupted": interrupted_releases,
                    "artifact_overwrites_interrupted": interrupted_artifact_overwrites,
                    "artifact_overwrites_needing_reconcile": artifact_overwrites_needing_reconcile,
                }),
            );
            insert_event(&transaction, &event)?;
            let projection = commit_pending_approval_projection(&transaction)?;
            transaction.commit()?;
            Ok(SecurityMutation {
                value: SecurityStartupReport {
                    cancelled_approvals: cancelled,
                    sandbox_executions_marked_unknown: u64::try_from(unknown).unwrap_or(u64::MAX),
                    network_grants_revoked: u64::try_from(revoked).unwrap_or(u64::MAX),
                    artifact_overwrites_interrupted: u64::try_from(interrupted_artifact_overwrites)
                        .unwrap_or(u64::MAX),
                    artifact_overwrites_needing_reconcile: u64::try_from(
                        artifact_overwrites_needing_reconcile,
                    )
                    .unwrap_or(u64::MAX),
                },
                projections: vec![projection],
            })
        })?;
        self.finish_security_mutation(mutation)
    }

    pub fn host_preparation_state(
        &self,
    ) -> Result<Option<HostPreparationStoredState>, PersistenceError> {
        self.with_reader(load_host_preparation_state)
    }

    /// The plan that is prepared right now, if any, with its own deadline.
    ///
    /// This is the light half of what an ordinary status read needs: it answers "is
    /// there anything to settle" without a host probe, without a runtime projection and
    /// without queueing a write behind the single persistence writer. A status route
    /// that settles only what this fact says is expired therefore costs two reads in
    /// the ordinary case, which is what keeps it from being dragged down by unrelated
    /// write pressure.
    pub fn prepared_host_preparation_expiry(
        &self,
    ) -> Result<Option<HostPreparationExpiry>, PersistenceError> {
        self.with_reader(|connection| {
            let active = load_active_host_preparation_operation(connection)?;
            Ok(active
                .filter(|operation| operation.state == HostPreparationOperationState::Prepared)
                .map(|operation| HostPreparationExpiry {
                    operation_id: operation.operation_id,
                    expires_at: operation.expires_at,
                }))
        })
    }

    pub fn host_preparation_operation(
        &self,
        operation_id: HostPreparationOperationId,
    ) -> Result<HostPreparationJournalRecord, PersistenceError> {
        self.with_reader(|connection| load_host_preparation_journal(connection, operation_id))
    }

    pub fn record_host_preparation_probe(
        &self,
        runtime: SecurityRuntimeProjection,
        correlation_id: CorrelationId,
    ) -> Result<HostPreparationStoredState, PersistenceError> {
        let mutation = self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let current = load_host_preparation_state(&transaction)?;
            let ownership = current
                .as_ref()
                .map_or_else(HostPreparationOwnership::default, |state| state.ownership.clone());
            let revision = current
                .as_ref()
                .map_or(0, |state| state.status.revision)
                .checked_add(1)
                .ok_or_else(|| {
                    PersistenceError::InvalidRequest(
                        "host preparation revision exhausted".to_owned(),
                    )
                })?;
            let status = HostPreparationStatus {
                summary: runtime.host_preparation.clone(),
                revision,
                active_operation: load_active_host_preparation_operation(&transaction)?,
            };
            upsert_host_preparation_state(&transaction, &status, &ownership)?;
            insert_event(
                &transaction,
                &PersistentEventDraft::system(
                    "security.host_preparation.probed",
                    correlation_id,
                    json!({ "revision": revision, "summary": status.summary }),
                ),
            )?;
            let projection = commit_security_projection(
                &transaction,
                "security_runtime",
                "global",
                SecurityProjectionPayload::SecurityRuntime(runtime),
            )?;
            transaction.commit()?;
            Ok(SecurityMutation {
                value: HostPreparationStoredState { status, ownership },
                projections: vec![projection],
            })
        })?;
        self.finish_security_mutation(mutation)
    }

    /// Atomically expires one prepared Host Preparation plan and publishes the
    /// matching operational state, event, and projection update.
    ///
    /// `expected_operation_id` names the plan the caller believed was due. The
    /// transaction re-reads the active operation and refuses when it is not that plan, so
    /// a settlement can never act on an observation that has been overtaken by a newer
    /// plan — the row alone would otherwise be enough to settle whatever happens to be
    /// prepared at the moment the writer gets to it.
    pub fn expire_prepared_host_preparation(
        &self,
        now: UtcTimestamp,
        runtime: SecurityRuntimeProjection,
        expected_operation_id: Option<HostPreparationOperationId>,
        correlation_id: CorrelationId,
    ) -> Result<Option<HostPreparationOperationId>, PersistenceError> {
        let mutation = self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let Some(current) = load_host_preparation_state(&transaction)? else {
                return Ok(None);
            };
            let Some(mut operation) = load_active_host_preparation_operation(&transaction)? else {
                return Ok(None);
            };
            if operation.state != HostPreparationOperationState::Prepared
                || operation.expires_at >= now
                || expected_operation_id.is_some_and(|expected| expected != operation.operation_id)
            {
                return Ok(None);
            }

            let operation_id = operation.operation_id;
            operation.state = HostPreparationOperationState::Failed;
            operation.operation_revision =
                operation.operation_revision.checked_add(1).ok_or_else(|| {
                    PersistenceError::InvalidRequest(
                        "host preparation operation revision exhausted".to_owned(),
                    )
                })?;
            operation.updated_at = now;
            operation.error_code = Some("sandbox.host_preparation_plan_expired".to_owned());
            let changed = transaction.execute(
                "UPDATE host_preparation_journal
                 SET state='failed', operation_json=?1, updated_at=?2, error_code=?3
                 WHERE host_preparation_operation_id=?4 AND state='prepared'",
                params![
                    serde_json::to_string(&operation)?,
                    now.to_string(),
                    operation.error_code,
                    operation_id.to_string(),
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::HostPreparationStateConflict);
            }
            let revision = current.status.revision.checked_add(1).ok_or_else(|| {
                PersistenceError::InvalidRequest("host preparation revision exhausted".to_owned())
            })?;
            let status = HostPreparationStatus {
                summary: runtime.host_preparation.clone(),
                revision,
                active_operation: None,
            };
            upsert_host_preparation_state(&transaction, &status, &current.ownership)?;
            insert_event(
                &transaction,
                &PersistentEventDraft::system(
                    "security.host_preparation.operation_transitioned",
                    correlation_id,
                    json!({
                        "operation_id": operation_id,
                        "from": HostPreparationOperationState::Prepared,
                        "to": HostPreparationOperationState::Failed,
                        "operation_revision": operation.operation_revision,
                        "error_code": operation.error_code,
                    }),
                ),
            )?;
            let projection = commit_security_projection(
                &transaction,
                "security_runtime",
                "global",
                SecurityProjectionPayload::SecurityRuntime(runtime),
            )?;
            transaction.commit()?;
            Ok(Some(SecurityMutation { value: operation_id, projections: vec![projection] }))
        })?;
        match mutation {
            Some(mutation) => self.finish_security_mutation(mutation).map(Some),
            None => Ok(None),
        }
    }

    pub fn begin_host_preparation_operation(
        &self,
        expected_revision: u64,
        mut operation: HostPreparationOperationView,
        broker_plan: HostPreparationBrokerPlan,
        approval: ApprovalView,
        runtime: SecurityRuntimeProjection,
        correlation_id: CorrelationId,
    ) -> Result<HostPreparationStoredState, PersistenceError> {
        let mutation = self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let current = load_host_preparation_state(&transaction)?
                .ok_or(PersistenceError::HostPreparationRevisionConflict)?;
            if current.status.revision != expected_revision {
                return Err(PersistenceError::HostPreparationRevisionConflict);
            }
            if load_active_host_preparation_operation(&transaction)?.is_some() {
                return Err(PersistenceError::HostPreparationStateConflict);
            }
            if operation.operation_id != broker_plan.operation_id
                || operation.action != broker_plan.action
                || operation.state != HostPreparationOperationState::Prepared
                || operation.operation_revision != 1
                || operation.host_preparation_revision != expected_revision
                || operation.host_preparation_revision != broker_plan.host_preparation_revision
                || operation.runtime_instance_id != broker_plan.runtime_instance_id
                || operation.release_descriptor != broker_plan.release_descriptor
                || operation.plan_hash != broker_plan.plan_hash
                || operation.expires_at != broker_plan.expires_at
                || operation.add_all_application_packages_ace
                    != broker_plan.add_all_application_packages_ace
                || operation.add_all_restricted_application_packages_ace
                    != broker_plan.add_all_restricted_application_packages_ace
                || operation.update_null_device_security != broker_plan.update_null_device_security
                || operation.error_code.is_some()
                || approval.request_revision != 1
                || approval.created_at != operation.created_at
                || approval.allowed_decisions
                    != [ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny]
            {
                return Err(PersistenceError::InvalidRequest(
                    "host preparation operation does not match its broker plan".to_owned(),
                ));
            }
            operation.updated_at = operation.created_at;
            transaction.execute(
                "INSERT INTO host_preparation_journal(
                    host_preparation_operation_id, action, state, operation_json,
                    broker_plan_json, plan_hash_json, created_at, updated_at, error_code,
                    approval_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, NULL, ?8)",
                params![
                    operation.operation_id.to_string(),
                    host_preparation_action_name(operation.action),
                    host_preparation_operation_state_name(operation.state),
                    serde_json::to_string(&operation)?,
                    serde_json::to_string(&broker_plan)?,
                    serde_json::to_string(&broker_plan.plan_hash)?,
                    operation.created_at.to_string(),
                    serde_json::to_string(&approval)?,
                ],
            )?;
            let revision = expected_revision.checked_add(1).ok_or_else(|| {
                PersistenceError::InvalidRequest("host preparation revision exhausted".to_owned())
            })?;
            let status = HostPreparationStatus {
                summary: runtime.host_preparation.clone(),
                revision,
                active_operation: Some(operation.clone()),
            };
            upsert_host_preparation_state(&transaction, &status, &current.ownership)?;
            insert_event(
                &transaction,
                &PersistentEventDraft::system(
                    "security.host_preparation.operation_prepared",
                    correlation_id,
                    json!({
                        "operation_id": operation.operation_id,
                        "action": operation.action,
                        "plan_hash": broker_plan.plan_hash,
                    }),
                ),
            )?;
            let projection = commit_security_projection(
                &transaction,
                "security_runtime",
                "global",
                SecurityProjectionPayload::SecurityRuntime(runtime),
            )?;
            transaction.commit()?;
            Ok(SecurityMutation {
                value: HostPreparationStoredState { status, ownership: current.ownership },
                projections: vec![projection],
            })
        })?;
        self.finish_security_mutation(mutation)
    }

    pub fn transition_host_preparation_operation(
        &self,
        transition: HostPreparationOperationTransition,
    ) -> Result<HostPreparationStoredState, PersistenceError> {
        let HostPreparationOperationTransition {
            operation_id,
            expected_state,
            expected_operation_revision,
            expected_host_preparation_revision,
            next_state,
            ownership_after,
            error_code,
            runtime,
            correlation_id,
        } = transition;
        let mutation = self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let mut journal = load_host_preparation_journal(&transaction, operation_id)?;
            let current = load_host_preparation_state(&transaction)?
                .ok_or(PersistenceError::HostPreparationStateConflict)?;
            if journal.operation.state != expected_state
                || journal.operation.operation_revision != expected_operation_revision
                || current.status.revision != expected_host_preparation_revision
                || !valid_host_preparation_transition(expected_state, next_state)
            {
                return Err(PersistenceError::HostPreparationStateConflict);
            }
            let now = UtcTimestamp::now();
            journal.operation.state = next_state;
            journal.operation.operation_revision =
                journal.operation.operation_revision.checked_add(1).ok_or_else(|| {
                    PersistenceError::InvalidRequest(
                        "host preparation operation revision exhausted".to_owned(),
                    )
                })?;
            journal.operation.updated_at = now;
            journal.operation.error_code = error_code.clone();
            let changed = transaction.execute(
                "UPDATE host_preparation_journal
                 SET state=?1, operation_json=?2, updated_at=?3, error_code=?4
                 WHERE host_preparation_operation_id=?5 AND state=?6",
                params![
                    host_preparation_operation_state_name(next_state),
                    serde_json::to_string(&journal.operation)?,
                    now.to_string(),
                    error_code,
                    operation_id.to_string(),
                    host_preparation_operation_state_name(expected_state),
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::HostPreparationStateConflict);
            }
            let approval_id = journal.approval.as_ref().map(|approval| approval.id);
            let approval_decision = match (expected_state, next_state) {
                (
                    HostPreparationOperationState::Prepared,
                    HostPreparationOperationState::Denied,
                ) => Some(ApprovalDecisionKind::Deny),
                (
                    HostPreparationOperationState::Prepared,
                    HostPreparationOperationState::AwaitingElevation
                    | HostPreparationOperationState::Applying,
                ) => Some(ApprovalDecisionKind::AllowOnce),
                _ => None,
            };
            let ownership = ownership_after.unwrap_or(current.ownership);
            let revision = current.status.revision.checked_add(1).ok_or_else(|| {
                PersistenceError::InvalidRequest("host preparation revision exhausted".to_owned())
            })?;
            let active_operation = if host_preparation_state_is_terminal(next_state) {
                None
            } else {
                Some(journal.operation.clone())
            };
            let status = HostPreparationStatus {
                summary: runtime.host_preparation.clone(),
                revision,
                active_operation,
            };
            upsert_host_preparation_state(&transaction, &status, &ownership)?;
            insert_event(
                &transaction,
                &PersistentEventDraft::system(
                    "security.host_preparation.operation_transitioned",
                    correlation_id,
                    json!({
                        "operation_id": operation_id,
                        "from": expected_state,
                        "to": next_state,
                        "operation_revision": journal.operation.operation_revision,
                        "approval_id": approval_id,
                        "approval_decision": approval_decision,
                        "error_code": journal.operation.error_code,
                    }),
                ),
            )?;
            let projection = commit_security_projection(
                &transaction,
                "security_runtime",
                "global",
                SecurityProjectionPayload::SecurityRuntime(runtime),
            )?;
            transaction.commit()?;
            Ok(SecurityMutation {
                value: HostPreparationStoredState { status, ownership },
                projections: vec![projection],
            })
        })?;
        self.finish_security_mutation(mutation)
    }

    pub fn pending_host_preparation_operations(
        &self,
    ) -> Result<Vec<HostPreparationJournalRecord>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT host_preparation_operation_id
                 FROM host_preparation_journal
                 WHERE state NOT IN ('completed', 'rolled_back', 'denied', 'failed')
                 ORDER BY created_at, host_preparation_operation_id",
            )?;
            let ids = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids.into_iter()
                .map(|value| {
                    load_host_preparation_journal(
                        connection,
                        HostPreparationOperationId::from_str(&value)?,
                    )
                })
                .collect()
        })
    }

    pub fn ensure_session_permission_state(
        &self,
        session_id: SessionId,
        workspace_id: WorkspaceId,
        default_mode: PermissionMode,
        correlation_id: CorrelationId,
    ) -> Result<SessionPermissionProjection, PersistenceError> {
        let mutation = self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let mutation = session_authority::ensure(
                &transaction,
                session_id,
                workspace_id,
                default_mode,
                correlation_id,
            )?;
            transaction.commit()?;
            Ok(mutation)
        })?;
        self.finish_security_mutation(mutation)
    }

    pub fn session_permission_state(
        &self,
        session_id: SessionId,
    ) -> Result<SessionPermissionProjection, PersistenceError> {
        self.with_reader(|connection| load_session_permission_projection(connection, session_id))
    }

    pub fn set_session_permission_mode(
        &self,
        session_id: SessionId,
        expected_revision: u64,
        mode: PermissionMode,
        correlation_id: CorrelationId,
    ) -> Result<SessionPermissionProjection, PersistenceError> {
        let _launch = self.try_permission_change_lease(session_id)?;
        self.commit_session_permission_mode(session_id, expected_revision, mode, correlation_id)
    }

    pub async fn change_session_permission_mode(
        &self,
        session_id: SessionId,
        expected_revision: u64,
        mode: PermissionMode,
        correlation_id: CorrelationId,
    ) -> Result<SessionPermissionProjection, PersistenceError> {
        let _launch = self.permission_change_lease(session_id).await;
        self.commit_session_permission_mode_awaitable(
            session_id,
            expected_revision,
            mode,
            correlation_id,
        )
        .await
    }

    /// The async mode change's commit: the same transaction, submitted to the one writer
    /// instead of run on the calling thread.
    ///
    /// WHY THE CALLER STOPS WAITING SYNCHRONOUSLY
    /// The commit's first step is [`PersistenceCoordinator::lock_permission_update`], which
    /// parks its thread in `Condvar::wait` while a same-session authority section is live,
    /// and the step after it waits for the writer to dispatch the transaction. Running that
    /// from the request reactor stops the executor that also owes the timer, the heartbeat
    /// and another session's request: on one worker a 100 ms timer did not run inside a
    /// 700 ms observation window and fired 815 ms after the release. Waiting for the
    /// authority is therefore done asynchronously
    /// ([`PersistenceCoordinator::lock_permission_update_awaitable`]) and the transaction -
    /// including the revision CAS and the whole mode-change body - is submitted to the
    /// existing single writer with [`PersistenceCoordinator::dispatch_awaitable`], so the
    /// caller never parks a thread it does not own.
    ///
    /// WHAT IS KEPT EXACTLY
    /// * the same gate, taken for the same span: the authority write is held from before
    ///   the transaction is submitted until after its projections are published, so the
    ///   mode change still cannot commit inside its session's live authority section, and
    ///   the authority reader is never released early to look non-blocking;
    /// * the same start/mode atomicity: the caller still holds the session's start change
    ///   lease for the whole commit, so a launch cannot interleave with it;
    /// * the same revision CAS, the same invalidations, the same recorded transition and
    ///   the same projections as the synchronous entry - both call the one
    ///   [`transact_session_permission_change`] body;
    /// * the same per-session fault attribution: a faulted gate is still this session's
    ///   answer, returned before anything is submitted, and an unwind inside the job still
    ///   confines the fault to this session.
    ///
    /// WHY THIS IS NOT A ROUTE AROUND THE BOUNDED OWNER
    /// No thread is spawned, no second store or connection is opened, and no blocking
    /// permit is held while waiting. The transaction runs on the writer every other write
    /// in this process already goes through, with the same lane, the same queue bound and
    /// the same deadline: an interactive `WorkClass::Sync` job, refused at admission with
    /// `WriterBusy` ("not executed") if the lane is full and answered with
    /// `WriterUnresponsive` (admitted, no answer yet - never reported as "did not run") if
    /// the writer does not answer inside the budget.
    async fn commit_session_permission_mode_awaitable(
        &self,
        session_id: SessionId,
        expected_revision: u64,
        mode: PermissionMode,
        correlation_id: CorrelationId,
    ) -> Result<SessionPermissionProjection, PersistenceError> {
        // Held across the submit, the commit and the publication, exactly as the
        // synchronous path holds it: this is the exclusion, not a decoration.
        let _barrier = self.lock_permission_update_awaitable(session_id).await?;
        let decision =
            SessionPermissionChange { session_id, expected_revision, mode, correlation_id };
        let mutation = self
            .dispatch_awaitable(permission_write_identity(session_id), move |connection| {
                transact_session_permission_change(connection, decision)
            })?;
        let mutation = mutation.await.map_err(|_| PersistenceError::WriterResponseClosed)??;
        self.finish_security_mutation_awaitable(mutation).await
    }

    fn commit_session_permission_mode(
        &self,
        session_id: SessionId,
        expected_revision: u64,
        mode: PermissionMode,
        correlation_id: CorrelationId,
    ) -> Result<SessionPermissionProjection, PersistenceError> {
        let _barrier = self.lock_permission_update(session_id)?;
        let decision =
            SessionPermissionChange { session_id, expected_revision, mode, correlation_id };
        let mutation = self
            .dispatch(move |connection| transact_session_permission_change(connection, decision))?;
        self.finish_security_mutation(mutation)
    }

    pub fn create_permission_snapshot(
        &self,
        session_id: SessionId,
        turn_task_id: Option<TurnTaskId>,
        correlation_id: CorrelationId,
    ) -> Result<PermissionSnapshot, PersistenceError> {
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let state = load_session_permission_projection(&transaction, session_id)?;
            let workspace_id = session_workspace_id(&transaction, session_id)?;
            let snapshot = PermissionSnapshot {
                id: PermissionSnapshotId::new(),
                session_id,
                workspace_id,
                mode: state.mode,
                session_permission_revision: state.revision,
                active_grant_ids: active_grant_ids(
                    &transaction,
                    workspace_id,
                    session_id,
                    turn_task_id,
                )?,
                created_at: UtcTimestamp::now(),
            };
            transaction.execute(
                "INSERT INTO permission_snapshots(
                    permission_snapshot_id, session_id, workspace_id, mode,
                    session_permission_revision, active_grant_ids_json, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    snapshot.id.to_string(),
                    session_id.to_string(),
                    workspace_id.to_string(),
                    permission_mode_name(snapshot.mode),
                    to_i64(snapshot.session_permission_revision, "permission revision")?,
                    serde_json::to_string(&snapshot.active_grant_ids)?,
                    snapshot.created_at.to_string()
                ],
            )?;
            let mut event = PersistentEventDraft::system(
                "PermissionSnapshotCreated",
                correlation_id,
                json!({
                    "permission_snapshot_id": snapshot.id,
                    "session_id": session_id,
                    "workspace_id": workspace_id,
                    "mode": snapshot.mode,
                    "active_grant_count": snapshot.active_grant_ids.len(),
                }),
            );
            event.workspace_id = Some(workspace_id);
            event.session_id = Some(session_id);
            event.turn_task_id = turn_task_id;
            insert_event(&transaction, &event)?;
            transaction.commit()?;
            Ok(snapshot)
        })
    }

    pub fn permission_snapshot(
        &self,
        snapshot_id: PermissionSnapshotId,
    ) -> Result<PermissionSnapshot, PersistenceError> {
        self.with_reader(|connection| {
            connection
                .query_row(
                    "SELECT session_id, workspace_id, mode, session_permission_revision,
                            active_grant_ids_json, created_at
                     FROM permission_snapshots WHERE permission_snapshot_id=?1",
                    [snapshot_id.to_string()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, i64>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                        ))
                    },
                )
                .optional()?
                .map(|row| parse_snapshot(snapshot_id, row))
                .transpose()?
                .ok_or(PersistenceError::PermissionSnapshotNotFound)
        })
    }

    pub fn list_permission_grants(
        &self,
        workspace_id: WorkspaceId,
        session_id: Option<SessionId>,
    ) -> Result<Vec<PermissionGrant>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT grant_json FROM permission_grants
                 WHERE workspace_id=?1
                   AND (?2 IS NULL OR session_id IS NULL OR session_id=?2)
                 ORDER BY created_at, permission_grant_id",
            )?;
            statement
                .query_map(
                    params![workspace_id.to_string(), session_id.map(|id| id.to_string())],
                    |row| row.get::<_, String>(0),
                )?
                .map(|row| {
                    let value = row?;
                    serde_json::from_str(&value).map_err(PersistenceError::from)
                })
                .collect()
        })
    }

    pub fn revoke_permission_grant(
        &self,
        grant_id: PermissionGrantId,
        correlation_id: CorrelationId,
    ) -> Result<PermissionGrant, PersistenceError> {
        let mutation = self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let mut grant = load_grant(&transaction, grant_id)?
                .ok_or(PersistenceError::PermissionGrantNotFound)?;
            if grant.revoked_at.is_none() {
                grant.revoked_at = Some(UtcTimestamp::now());
                update_grant(&transaction, &grant)?;
                transaction.execute(
                    "UPDATE external_grants SET
                        lifecycle='revoked', revoked_at=?2, suspended_at=NULL,
                        updated_at=?2, revalidation_revision=revalidation_revision+1
                     WHERE permission_grant_id=?1 AND lifecycle!='revoked'",
                    params![grant_id.to_string(), grant.revoked_at.map(|value| value.to_string())],
                )?;
            }
            let mut event = PersistentEventDraft::system(
                "PermissionGrantRevoked",
                correlation_id,
                json!({ "permission_grant_id": grant_id }),
            );
            event.workspace_id = Some(grant.workspace_id);
            event.session_id = grant.session_id;
            event.actor = EventActor::User;
            insert_event(&transaction, &event)?;
            let mut projections = Vec::new();
            if let Some(session_id) = grant.session_id
                && let Ok(payload) = load_session_permission_projection(&transaction, session_id)
            {
                projections.push(commit_session_permission_projection(&transaction, payload)?);
            }
            transaction.commit()?;
            Ok(SecurityMutation { value: grant, projections })
        })?;
        self.finish_security_mutation(mutation)
    }

    pub fn permission_grant(
        &self,
        grant_id: PermissionGrantId,
    ) -> Result<PermissionGrant, PersistenceError> {
        self.with_reader(|connection| {
            load_grant(connection, grant_id)?.ok_or(PersistenceError::PermissionGrantNotFound)
        })
    }

    /// Mint or reuse a directory grant and bind it to one approval decision.
    ///
    /// The same approval and root identity always return the grant already bound,
    /// including when that grant was revoked afterwards. A different approval may
    /// reuse a still-active grant or mint a new one after revocation.
    pub fn issue_bound_directory_grant(
        &self,
        approval_id: ApprovalRequestId,
        operation_fingerprint: String,
        candidate: ExternalGrantRecord,
        correlation_id: CorrelationId,
    ) -> Result<ExternalGrantRecord, PersistenceError> {
        validate_external_grant(&candidate)?;
        let identity_json = serde_json::to_string(&candidate.root_identity)?;
        self.dispatch(move |connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if let Some(bound) =
                directory_decision_grant(&transaction, &approval_id, &identity_json)?
            {
                let record = load_external_grant(&transaction, bound)?
                    .ok_or(PersistenceError::ExternalGrantNotFound)?;
                transaction.commit()?;
                return Ok(record);
            }
            if let Some(existing) = reusable_directory_grant(&transaction, &candidate)? {
                insert_directory_decision(
                    &transaction,
                    &approval_id,
                    &identity_json,
                    existing.id,
                    &operation_fingerprint,
                    candidate.session_id,
                )?;
                transaction.commit()?;
                return Ok(existing);
            }
            insert_external_grant(&transaction, &candidate, correlation_id)?;
            insert_directory_decision(
                &transaction,
                &approval_id,
                &identity_json,
                candidate.id,
                &operation_fingerprint,
                candidate.session_id,
            )?;
            transaction.commit()?;
            Ok(candidate)
        })
    }

    /// Grants minted or reused for one frozen operation, so a later revocation is visible
    /// to the call that approval released.
    pub fn directory_grants_for_operation(
        &self,
        operation_fingerprint: &str,
    ) -> Result<Vec<ExternalGrantId>, PersistenceError> {
        let fingerprint = operation_fingerprint.to_owned();
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT external_grant_id FROM external_grant_decisions
                 WHERE operation_fingerprint=?1 ORDER BY created_at, external_grant_id",
            )?;
            let ids = statement
                .query_map([fingerprint], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids.into_iter()
                .map(|value| ExternalGrantId::from_str(&value).map_err(PersistenceError::from))
                .collect()
        })
    }

    pub fn external_grant(
        &self,
        grant_id: ExternalGrantId,
    ) -> Result<ExternalGrantRecord, PersistenceError> {
        self.with_reader(|connection| {
            load_external_grant(connection, grant_id)?
                .ok_or(PersistenceError::ExternalGrantNotFound)
        })
    }

    pub fn list_session_external_grants(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<ExternalGrantRecord>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT external_grant_id FROM external_grants
                 WHERE session_id=?1 ORDER BY created_at, external_grant_id",
            )?;
            let ids = statement
                .query_map([session_id.to_string()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids.into_iter()
                .map(|value| {
                    let id = ExternalGrantId::from_str(&value)?;
                    load_external_grant(connection, id)?
                        .ok_or(PersistenceError::ExternalGrantNotFound)
                })
                .collect()
        })
    }

    /// Workspace-term grants for this workspace, including those approved in another session.
    pub fn list_workspace_external_grants(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<ExternalGrantRecord>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT external_grant_id FROM external_grants
                 WHERE workspace_id=?1 AND scope_lifetime='workspace'
                 ORDER BY created_at, external_grant_id",
            )?;
            let ids = statement
                .query_map([workspace_id.to_string()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids.into_iter()
                .map(|value| {
                    let id = ExternalGrantId::from_str(&value)?;
                    load_external_grant(connection, id)?
                        .ok_or(PersistenceError::ExternalGrantNotFound)
                })
                .collect()
        })
    }

    pub fn workspace_of_session(
        &self,
        session_id: SessionId,
    ) -> Result<WorkspaceId, PersistenceError> {
        self.with_reader(|connection| session_workspace_id(connection, session_id))
    }

    pub fn active_external_grant_for_permission(
        &self,
        session_id: SessionId,
        permission_grant_id: PermissionGrantId,
    ) -> Result<Option<ExternalGrantRecord>, PersistenceError> {
        self.with_reader(|connection| {
            let id = connection
                .query_row(
                    "SELECT external_grant_id FROM external_grants
                     WHERE session_id=?1 AND permission_grant_id=?2 AND lifecycle='active'
                     ORDER BY created_at DESC LIMIT 1",
                    params![session_id.to_string(), permission_grant_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            id.map(|value| {
                let id = ExternalGrantId::from_str(&value)?;
                load_external_grant(connection, id)?.ok_or(PersistenceError::ExternalGrantNotFound)
            })
            .transpose()
        })
    }

    pub fn replace_external_grant_after_revalidation(
        &self,
        record: ExternalGrantRecord,
        expected_revalidation_revision: u64,
        correlation_id: CorrelationId,
    ) -> Result<ExternalGrantRecord, PersistenceError> {
        validate_external_grant(&record)?;
        if record.revalidation_revision != expected_revalidation_revision.saturating_add(1) {
            return Err(PersistenceError::ExternalGrantStateConflict);
        }
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let existing = load_external_grant(&transaction, record.id)?
                .ok_or(PersistenceError::ExternalGrantNotFound)?;
            if existing.session_id != record.session_id
                || existing.workspace_id != record.workspace_id
                || existing.revalidation_revision != expected_revalidation_revision
                || existing.lifecycle == ExternalGrantLifecycle::Revoked
            {
                return Err(PersistenceError::ExternalGrantStateConflict);
            }
            if transaction.execute(
                "UPDATE external_grants SET
                    selected_root=?2, canonical_root=?3, root_identity_json=?4,
                    root_is_directory=?5, capabilities_json=?6, recursive_scope=?7,
                    lifecycle=?8, revalidation_revision=?9, updated_at=?10,
                    suspended_at=?11, revoked_at=?12
                 WHERE external_grant_id=?1 AND revalidation_revision=?13",
                params![
                    record.id.to_string(),
                    record.selected_root.display,
                    record.canonical_root.display,
                    serde_json::to_string(&record.root_identity)?,
                    record.target_kind == ExternalGrantTargetKind::Directory,
                    serde_json::to_string(&record.capabilities)?,
                    record.recursive_scope,
                    external_grant_lifecycle_name(record.lifecycle),
                    to_i64(record.revalidation_revision, "external grant revision")?,
                    record.updated_at.to_string(),
                    record.suspended_at.map(|value| value.to_string()),
                    record.revoked_at.map(|value| value.to_string()),
                    to_i64(expected_revalidation_revision, "external grant revision")?,
                ],
            )? != 1
            {
                return Err(PersistenceError::ExternalGrantStateConflict);
            }
            let mut event = PersistentEventDraft::system(
                "ExternalGrantRevalidated",
                correlation_id,
                json!({
                    "external_grant_id": record.id,
                    "session_id": record.session_id,
                    "lifecycle": record.lifecycle,
                    "revalidation_revision": record.revalidation_revision,
                }),
            );
            event.workspace_id = Some(record.workspace_id);
            event.session_id = Some(record.session_id);
            insert_event(&transaction, &event)?;
            transaction.commit()?;
            Ok(record)
        })
    }

    pub fn revoke_external_grant(
        &self,
        grant_id: ExternalGrantId,
        session_id: SessionId,
        correlation_id: CorrelationId,
    ) -> Result<ExternalGrantRecord, PersistenceError> {
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let mut record = load_external_grant(&transaction, grant_id)?
                .ok_or(PersistenceError::ExternalGrantNotFound)?;
            if record.session_id != session_id {
                let same_workspace = record.term == ExternalGrantTerm::Workspace
                    && session_workspace_id(&transaction, session_id)? == record.workspace_id;
                if !same_workspace {
                    return Err(PersistenceError::ExternalGrantSessionMismatch);
                }
            }
            if record.lifecycle != ExternalGrantLifecycle::Revoked {
                let now = UtcTimestamp::now();
                record.lifecycle = ExternalGrantLifecycle::Revoked;
                record.revalidation_revision =
                    record.revalidation_revision.checked_add(1).ok_or_else(|| {
                        PersistenceError::InvalidRequest(
                            "external grant revision exhausted".to_owned(),
                        )
                    })?;
                record.updated_at = now;
                record.suspended_at = None;
                record.revoked_at = Some(now);
                transaction.execute(
                    "UPDATE external_grants SET lifecycle='revoked',
                        revalidation_revision=?2, updated_at=?3,
                        suspended_at=NULL, revoked_at=?3
                     WHERE external_grant_id=?1",
                    params![
                        record.id.to_string(),
                        to_i64(record.revalidation_revision, "external grant revision")?,
                        now.to_string(),
                    ],
                )?;
                let mut event = PersistentEventDraft::system(
                    "ExternalGrantRevoked",
                    correlation_id,
                    json!({ "external_grant_id": record.id, "session_id": session_id }),
                );
                event.workspace_id = Some(record.workspace_id);
                event.session_id = Some(session_id);
                event.actor = EventActor::User;
                insert_event(&transaction, &event)?;
            }
            transaction.commit()?;
            Ok(record)
        })
    }

    pub fn create_approval_request(
        &self,
        draft: ApprovalRequestDraft,
        correlation_id: CorrelationId,
    ) -> Result<ApprovalRequest, PersistenceError> {
        let mutation = self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            transaction
                .query_row(
                    "SELECT 1 FROM permission_snapshots WHERE permission_snapshot_id=?1",
                    [draft.permission_snapshot_id.to_string()],
                    |_| Ok(()),
                )
                .optional()?
                .ok_or(PersistenceError::PermissionSnapshotNotFound)?;
            if draft.operation_ids.is_empty()
                || draft.operation_ids.len() != draft.operation_fingerprints.len()
            {
                return Err(PersistenceError::InvalidRequest(
                    "approval operations and fingerprints must be non-empty and aligned".to_owned(),
                ));
            }
            let now = UtcTimestamp::now();
            let request = ApprovalRequest {
                id: ApprovalRequestId::new(),
                identity: draft.identity,
                operation_ids: draft.operation_ids,
                operation_fingerprints: draft.operation_fingerprints,
                title: draft.title,
                model_purpose: draft.model_purpose,
                actual_operation_summary: draft.actual_operation_summary,
                source_summary: draft.source_summary,
                source: draft.source,
                source_fingerprint: draft.source_fingerprint,
                capabilities: draft.capabilities,
                scopes: draft.scopes,
                risk_flags: draft.risk_flags,
                allowed_decisions: draft.allowed_decisions,
                permission_snapshot_id: draft.permission_snapshot_id,
                release_descriptor: draft.release_descriptor,
                request_revision: 1,
                state: ApprovalState::Pending,
                created_at: now,
                resolved_at: None,
                invalidation_reason: None,
            };
            insert_approval(&transaction, &request)?;
            let mut event = PersistentEventDraft::system(
                "ApprovalRequested",
                correlation_id,
                json!({
                    "approval_request_id": request.id,
                    "operation_count": request.operation_ids.len(),
                    "risk_flags": request.risk_flags,
                }),
            );
            event.workspace_id = request.identity.workspace_id;
            event.session_id = request.identity.session_id;
            event.turn_task_id = request.identity.turn_task_id;
            event.agent_run_id = request.identity.agent_run_id;
            insert_event(&transaction, &event)?;
            let projection = commit_pending_approval_projection(&transaction)?;
            transaction.commit()?;
            Ok(SecurityMutation { value: request, projections: vec![projection] })
        })?;
        self.finish_security_mutation(mutation)
    }

    pub fn pending_approvals(&self) -> Result<Vec<ApprovalView>, PersistenceError> {
        self.with_reader(|connection| {
            Ok(load_pending_approvals(connection)?.iter().map(ApprovalView::from).collect())
        })
    }

    pub fn approval_details(
        &self,
        approval_id: ApprovalRequestId,
    ) -> Result<ApprovalRequest, PersistenceError> {
        self.with_reader(|connection| {
            load_approval(connection, approval_id)?.ok_or(PersistenceError::ApprovalNotFound)
        })
    }

    pub fn resolve_approval(
        &self,
        approval_id: ApprovalRequestId,
        expected_request_revision: u64,
        decision: ApprovalDecisionKind,
        correlation_id: CorrelationId,
    ) -> Result<ApprovalCommit, PersistenceError> {
        let mutation = self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let mut request = load_approval(&transaction, approval_id)?
                .ok_or(PersistenceError::ApprovalNotFound)?;
            if request.state != ApprovalState::Pending {
                return Err(PersistenceError::ApprovalAlreadyResolved);
            }
            if request.request_revision != expected_request_revision {
                return Err(PersistenceError::ApprovalStale);
            }
            if !request.allowed_decisions.contains(&decision) {
                return Err(PersistenceError::ApprovalDecisionNotAllowed);
            }

            let resolved_at = UtcTimestamp::now();
            let decision_id = ApprovalDecisionId::new();
            let mut grant_ids = Vec::new();
            match decision.grant_lifetime() {
                Some(lifetime) => {
                    let workspace_id = request.identity.workspace_id.ok_or_else(|| {
                        PersistenceError::InvalidRequest(
                            "approved operations require a workspace identity".to_owned(),
                        )
                    })?;
                    let grants =
                        grants_for_approval(&request, lifetime, workspace_id, resolved_at)?;
                    for grant in grants {
                        insert_grant(&transaction, &grant, Some(decision_id))?;
                        grant_ids.push(grant.id);
                    }
                    request.state = ApprovalState::Approved;
                }
                None => {
                    request.state = ApprovalState::Denied;
                    if let Some(turn_task_id) = request.identity.turn_task_id {
                        for operation_fingerprint in &request.operation_fingerprints {
                            insert_turn_deny(
                                &transaction,
                                &TurnDenyRecord {
                                    turn_task_id,
                                    source: request.source.clone(),
                                    operation_fingerprint: operation_fingerprint.clone(),
                                    denied_at: resolved_at,
                                },
                            )?;
                        }
                    }
                }
            }

            request.request_revision =
                request.request_revision.checked_add(1).ok_or_else(|| {
                    PersistenceError::InvalidRequest("approval revision exhausted".to_owned())
                })?;
            request.resolved_at = Some(resolved_at);
            update_approval(&transaction, &request)?;
            transaction.execute(
                "INSERT INTO approval_decisions(
                    approval_decision_id, approval_request_id, decision_kind,
                    decided_at, permission_grant_ids_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    decision_id.to_string(),
                    approval_id.to_string(),
                    approval_decision_name(decision),
                    resolved_at.to_string(),
                    serde_json::to_string(&grant_ids)?,
                ],
            )?;

            let resolution = ApprovalResolution {
                approval_id,
                decision_id,
                state: request.state,
                decision,
                grant_ids,
                resolved_at,
            };
            let mut event = PersistentEventDraft::system(
                "ApprovalResolved",
                correlation_id,
                json!({
                    "approval_id": approval_id,
                    "decision_id": decision_id,
                    "decision": decision,
                    "state": request.state,
                    "grant_ids": resolution.grant_ids,
                }),
            );
            event.workspace_id = request.identity.workspace_id;
            event.session_id = request.identity.session_id;
            event.turn_task_id = request.identity.turn_task_id;
            event.agent_run_id = request.identity.agent_run_id;
            event.actor = EventActor::User;
            insert_event(&transaction, &event)?;

            let mut projections = vec![commit_pending_approval_projection(&transaction)?];
            if let Some(session_id) = request.identity.session_id {
                let payload = load_session_permission_projection(&transaction, session_id)?;
                projections.push(commit_session_permission_projection(&transaction, payload)?);
            }
            transaction.commit()?;
            Ok(SecurityMutation { value: resolution, projections })
        })?;
        let resolution = mutation.value.clone();
        let projection = mutation.projections.first().cloned().ok_or_else(|| {
            PersistenceError::InvalidRequest(
                "approval resolution produced no projection".to_owned(),
            )
        })?;
        for committed in mutation.projections {
            self.publish_and_mark_security(committed)?;
        }
        Ok(ApprovalCommit { resolution, projection })
    }

    pub fn upsert_external_trust(
        &self,
        source: SecuritySource,
        state: TrustState,
        effective_fingerprint: Option<ContentHash>,
        correlation_id: CorrelationId,
    ) -> Result<ExternalTrustRecord, PersistenceError> {
        let source_key = source.stable_key();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let existing = load_trust_by_source(&transaction, &source)?;
            let now = UtcTimestamp::now();
            let record = ExternalTrustRecord {
                id: existing.as_ref().map_or_else(TrustRecordId::new, |value| value.id),
                source,
                state,
                effective_fingerprint,
                updated_at: now,
            };
            transaction.execute(
                "INSERT INTO external_trust_records(
                    trust_record_id, record_json, source_key, source_json,
                    trust_state_json, effective_fingerprint_json, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(source_key) DO UPDATE SET
                    record_json=excluded.record_json,
                    source_json=excluded.source_json,
                    trust_state_json=excluded.trust_state_json,
                    effective_fingerprint_json=excluded.effective_fingerprint_json,
                    updated_at=excluded.updated_at",
                params![
                    record.id.to_string(),
                    serde_json::to_string(&record)?,
                    source_key,
                    serde_json::to_string(&record.source)?,
                    serde_json::to_string(&record.state)?,
                    record.effective_fingerprint.as_ref().map(serde_json::to_string).transpose()?,
                    now.to_string(),
                ],
            )?;
            let event = PersistentEventDraft::system(
                "ExternalTrustChanged",
                correlation_id,
                json!({
                    "trust_record_id": record.id,
                    "source": record.source,
                    "state": record.state,
                    "effective_fingerprint": record.effective_fingerprint,
                }),
            );
            insert_event(&transaction, &event)?;
            transaction.commit()?;
            Ok(record)
        })
    }

    pub fn external_trust(
        &self,
        source: &SecuritySource,
    ) -> Result<Option<ExternalTrustRecord>, PersistenceError> {
        self.with_reader(|connection| load_trust_by_source(connection, source))
    }

    pub fn invalidate_external_trust_if_changed(
        &self,
        source: SecuritySource,
        effective_fingerprint: ContentHash,
        correlation_id: CorrelationId,
    ) -> Result<ExternalTrustRecord, PersistenceError> {
        let current = self.external_trust(&source)?.ok_or(PersistenceError::TrustRecordNotFound)?;
        let unchanged = match &current.state {
            TrustState::SystemTrusted => true,
            TrustState::Trusted { fingerprint, .. } => fingerprint == &effective_fingerprint,
            TrustState::Untrusted | TrustState::Revoked => false,
        };
        if unchanged {
            return Ok(current);
        }
        self.upsert_external_trust(
            source,
            TrustState::Untrusted,
            Some(effective_fingerprint),
            correlation_id,
        )
    }

    pub fn create_sandbox_execution(
        &self,
        record: SandboxExecutionRecord,
        correlation_id: CorrelationId,
    ) -> Result<SandboxExecutionRecord, PersistenceError> {
        if record.state != SandboxExecutionState::Planned {
            return Err(PersistenceError::InvalidRequest(
                "new sandbox execution must be planned".to_owned(),
            ));
        }
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            transaction.execute(
                "INSERT INTO sandbox_executions(
                    sandbox_execution_id, identity_json, permission_snapshot_id,
                    mode, plan_hash_json, plan_json, state, process_id, exit_code,
                    created_at, updated_at, terminal_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    record.plan.execution_id.to_string(),
                    serde_json::to_string(&record.plan.identity)?,
                    record.plan.permission_snapshot_id.to_string(),
                    sandbox_mode_name(record.plan.mode),
                    serde_json::to_string(&record.plan.plan_hash)?,
                    serde_json::to_string(&record.plan)?,
                    sandbox_state_name(record.state),
                    record.process_id,
                    record.exit_code,
                    record.created_at.to_string(),
                    record.updated_at.to_string(),
                    record.terminal_at.map(|value| value.to_string()),
                ],
            )?;
            let mut event = PersistentEventDraft::system(
                "SandboxExecutionPlanned",
                correlation_id,
                json!({
                    "sandbox_execution_id": record.plan.execution_id,
                    "mode": record.plan.mode,
                    "network_access": record.plan.network_access,
                    "network_enforcement": record.plan.network_enforcement,
                    "network_inspection_level": NetworkInspectionLevel::for_enforcement(
                        record.plan.network_enforcement,
                    ),
                    "plan_hash": record.plan.plan_hash,
                }),
            );
            event.workspace_id = record.plan.identity.workspace_id;
            event.session_id = record.plan.identity.session_id;
            event.turn_task_id = record.plan.identity.turn_task_id;
            event.agent_run_id = record.plan.identity.agent_run_id;
            insert_event(&transaction, &event)?;
            transaction.commit()?;
            Ok(record)
        })
    }

    pub fn record_external_process_execution_audit(
        &self,
        plan: &ExternalProcessLaunchPlan,
        state: SandboxExecutionState,
        process_id: Option<u32>,
        exit_code: Option<u32>,
        error_code: Option<String>,
        correlation_id: CorrelationId,
    ) -> Result<(), PersistenceError> {
        if plan.network_enforcement == NetworkEnforcementKind::CoreControlPlane {
            return Err(PersistenceError::InvalidRequest(
                "external process execution cannot claim Core control-plane enforcement".to_owned(),
            ));
        }
        let plan = plan.clone();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let mut event = PersistentEventDraft::system(
                "ExternalProcessExecutionAudited",
                correlation_id,
                json!({
                    "sandbox_execution_id": plan.execution_id,
                    "source": plan.source,
                    "source_fingerprint": plan.source_fingerprint,
                    "isolation": plan.isolation,
                    "network_access": plan.network_access,
                    "network_enforcement": plan.network_enforcement,
                    "network_inspection_level": NetworkInspectionLevel::for_enforcement(
                        plan.network_enforcement,
                    ),
                    "permission_snapshot_id": plan.permission_snapshot_id,
                    "plan_hash": plan.plan_hash,
                    "state": state,
                    "process_id": process_id,
                    "exit_code": exit_code,
                    "error_code": error_code,
                }),
            );
            event.workspace_id = plan.identity.workspace_id;
            event.session_id = plan.identity.session_id;
            event.turn_task_id = plan.identity.turn_task_id;
            event.agent_run_id = plan.identity.agent_run_id;
            insert_event(&transaction, &event)?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn transition_sandbox_execution(
        &self,
        execution_id: SandboxExecutionId,
        expected_state: SandboxExecutionState,
        next_state: SandboxExecutionState,
        process_id: Option<u32>,
        exit_code: Option<u32>,
        correlation_id: CorrelationId,
    ) -> Result<SandboxExecutionRecord, PersistenceError> {
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let mut record = load_sandbox_execution(&transaction, execution_id)?
                .ok_or(PersistenceError::SandboxExecutionNotFound)?;
            if record.state != expected_state {
                return Err(PersistenceError::SandboxStateConflict);
            }
            let now = UtcTimestamp::now();
            record.state = next_state;
            record.process_id = process_id.or(record.process_id);
            record.exit_code = exit_code.or(record.exit_code);
            record.updated_at = now;
            if sandbox_state_is_terminal(next_state) {
                record.terminal_at = Some(now);
            }
            transaction.execute(
                "UPDATE sandbox_executions
                 SET state=?1, process_id=?2, exit_code=?3, updated_at=?4, terminal_at=?5
                 WHERE sandbox_execution_id=?6",
                params![
                    sandbox_state_name(record.state),
                    record.process_id,
                    record.exit_code,
                    record.updated_at.to_string(),
                    record.terminal_at.map(|value| value.to_string()),
                    execution_id.to_string(),
                ],
            )?;
            let mut event = PersistentEventDraft::system(
                "SandboxExecutionStateChanged",
                correlation_id,
                json!({
                    "sandbox_execution_id": execution_id,
                    "previous_state": expected_state,
                    "state": next_state,
                    "process_id": record.process_id,
                    "exit_code": record.exit_code,
                    "network_enforcement": record.plan.network_enforcement,
                    "network_inspection_level": NetworkInspectionLevel::for_enforcement(
                        record.plan.network_enforcement,
                    ),
                }),
            );
            event.workspace_id = record.plan.identity.workspace_id;
            event.session_id = record.plan.identity.session_id;
            event.turn_task_id = record.plan.identity.turn_task_id;
            event.agent_run_id = record.plan.identity.agent_run_id;
            insert_event(&transaction, &event)?;
            transaction.commit()?;
            Ok(record)
        })
    }

    pub fn prepare_sandbox_acl_journal(
        &self,
        entry: SandboxAclJournalEntry,
        correlation_id: CorrelationId,
    ) -> Result<SandboxAclJournalEntry, PersistenceError> {
        if entry.state != SandboxAclJournalState::Prepared {
            return Err(PersistenceError::InvalidRequest(
                "new ACL journal entry must be prepared".to_owned(),
            ));
        }
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            transaction.execute(
                "INSERT INTO sandbox_acl_journal(
                    sandbox_acl_journal_id, sandbox_execution_id, target_identity_json,
                    target_path_json,
                    target_path_display, original_dacl_sddl, original_dacl_hash,
                    appcontainer_profile_name, appcontainer_sid, access_mask, recursive,
                    state, created_at, updated_at, error_code
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    entry.id.to_string(),
                    entry.execution_id.to_string(),
                    serde_json::to_string(&entry.target_identity)?,
                    serde_json::to_string(&entry.target_path)?,
                    entry.target_path.display,
                    entry.original_dacl_sddl,
                    serde_json::to_string(&entry.original_dacl_hash)?,
                    entry.appcontainer_profile_name,
                    entry.appcontainer_sid,
                    sandbox_access_mask(entry.access),
                    entry.recursive,
                    acl_journal_state_name(entry.state),
                    entry.created_at.to_string(),
                    entry.updated_at.to_string(),
                    entry.error_code,
                ],
            )?;
            let event = PersistentEventDraft::system(
                "SandboxAclJournalPrepared",
                correlation_id,
                json!({
                    "sandbox_acl_journal_id": entry.id,
                    "sandbox_execution_id": entry.execution_id,
                    "target_path": entry.target_path.display,
                    "access": entry.access,
                }),
            );
            insert_event(&transaction, &event)?;
            transaction.commit()?;
            Ok(entry)
        })
    }

    pub fn update_sandbox_acl_journal(
        &self,
        journal_id: SandboxAclJournalId,
        expected_state: SandboxAclJournalState,
        next_state: SandboxAclJournalState,
        error_code: Option<String>,
        correlation_id: CorrelationId,
    ) -> Result<SandboxAclJournalEntry, PersistenceError> {
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let mut entry = load_acl_journal(&transaction, journal_id)?
                .ok_or(PersistenceError::SandboxJournalNotFound)?;
            if entry.state != expected_state {
                return Err(PersistenceError::SandboxStateConflict);
            }
            entry.state = next_state;
            entry.updated_at = UtcTimestamp::now();
            entry.error_code = error_code;
            transaction.execute(
                "UPDATE sandbox_acl_journal
                 SET state=?1, updated_at=?2, error_code=?3
                 WHERE sandbox_acl_journal_id=?4",
                params![
                    acl_journal_state_name(entry.state),
                    entry.updated_at.to_string(),
                    entry.error_code,
                    journal_id.to_string(),
                ],
            )?;
            let event = PersistentEventDraft::system(
                "SandboxAclJournalStateChanged",
                correlation_id,
                json!({
                    "sandbox_acl_journal_id": journal_id,
                    "sandbox_execution_id": entry.execution_id,
                    "state": next_state,
                    "error_code": entry.error_code,
                }),
            );
            insert_event(&transaction, &event)?;
            transaction.commit()?;
            Ok(entry)
        })
    }

    pub fn pending_sandbox_acl_cleanup(
        &self,
    ) -> Result<Vec<SandboxAclJournalEntry>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT sandbox_acl_journal_id FROM sandbox_acl_journal
                 WHERE state NOT IN ('cleaned') ORDER BY created_at, sandbox_acl_journal_id",
            )?;
            let ids = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids.into_iter()
                .map(|id| {
                    let id = SandboxAclJournalId::from_str(&id)?;
                    load_acl_journal(connection, id)?
                        .ok_or(PersistenceError::SandboxJournalNotFound)
                })
                .collect()
        })
    }

    pub fn create_network_grant(
        &self,
        grant: NetworkGrant,
        approval_decision_id: ApprovalDecisionId,
        correlation_id: CorrelationId,
    ) -> Result<NetworkGrant, PersistenceError> {
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let approved = transaction
                .query_row(
                    "SELECT requests.state
                     FROM approval_decisions AS decisions
                     JOIN approval_requests AS requests
                       ON requests.approval_request_id=decisions.approval_request_id
                     WHERE decisions.approval_decision_id=?1",
                    [approval_decision_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            if approved.as_deref() != Some("approved") {
                return Err(PersistenceError::ApprovalDecisionNotAllowed);
            }
            insert_network_grant(&transaction, &grant, approval_decision_id)?;
            let event = PersistentEventDraft::system(
                "NetworkGrantCreated",
                correlation_id,
                json!({
                    "network_grant_id": grant.id,
                    "identity_scope": grant.identity_scope,
                    "lifetime": grant.lifetime,
                    "origins": grant.origins,
                    "destination_classes": grant.destination_classes,
                    "methods": grant.methods,
                    "allowed_data_classes": grant.allowed_data_classes,
                }),
            );
            insert_event(&transaction, &event)?;
            transaction.commit()?;
            Ok(grant)
        })
    }

    pub fn network_grant(
        &self,
        grant_id: NetworkGrantId,
    ) -> Result<Option<NetworkGrant>, PersistenceError> {
        self.with_reader(|connection| load_network_grant(connection, grant_id))
    }

    pub fn revoke_network_grant(
        &self,
        grant_id: NetworkGrantId,
        correlation_id: CorrelationId,
    ) -> Result<NetworkGrant, PersistenceError> {
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let mut grant = load_network_grant(&transaction, grant_id)?
                .ok_or(PersistenceError::NetworkGrantNotFound)?;
            if grant.revoked_at.is_none() {
                grant.revoked_at = Some(UtcTimestamp::now());
                transaction.execute(
                    "UPDATE network_grants SET grant_json=?1, revoked_at=?2
                     WHERE network_grant_id=?3",
                    params![
                        serde_json::to_string(&grant)?,
                        grant.revoked_at.map(|value| value.to_string()),
                        grant_id.to_string(),
                    ],
                )?;
            }
            let event = PersistentEventDraft::system(
                "NetworkGrantRevoked",
                correlation_id,
                json!({ "network_grant_id": grant_id }),
            );
            insert_event(&transaction, &event)?;
            transaction.commit()?;
            Ok(grant)
        })
    }

    pub fn start_network_request_audit(
        &self,
        record: NetworkRequestAuditRecord,
        correlation_id: CorrelationId,
    ) -> Result<NetworkRequestAuditRecord, PersistenceError> {
        if record.state != NetworkRequestAuditState::Started || record.finished_at.is_some() {
            return Err(PersistenceError::InvalidRequest(
                "new network audit must be started and unfinished".to_owned(),
            ));
        }
        if record.network_enforcement != NetworkEnforcementKind::CoreControlPlane
            || record.inspection_level != NetworkInspectionLevel::StructuredHttp
        {
            return Err(PersistenceError::InvalidRequest(
                "Core HTTP audit must be StructuredHttp and originate from the Core control plane"
                    .to_owned(),
            ));
        }
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            insert_network_audit(&transaction, &record)?;
            let mut event = PersistentEventDraft::system(
                "NetworkRequestStarted",
                correlation_id,
                json!({
                    "network_request_id": record.request_id,
                    "caller": record.caller,
                    "network_enforcement": record.network_enforcement,
                    "inspection_level": record.inspection_level,
                    "method": record.method,
                    "origin": record.origin,
                    "destination_classes": record.destination_classes,
                    "data_classes": record.data_classes,
                    "upload_bytes": record.upload_bytes,
                }),
            );
            event.workspace_id = record.identity.workspace_id;
            event.session_id = record.identity.session_id;
            event.turn_task_id = record.identity.turn_task_id;
            event.agent_run_id = record.identity.agent_run_id;
            insert_event(&transaction, &event)?;
            transaction.commit()?;
            Ok(record)
        })
    }

    pub fn finish_network_request_audit(
        &self,
        request_id: NetworkRequestId,
        state: NetworkRequestAuditState,
        download_bytes: u64,
        error_code: Option<String>,
        correlation_id: CorrelationId,
    ) -> Result<NetworkRequestAuditRecord, PersistenceError> {
        if matches!(state, NetworkRequestAuditState::Started | NetworkRequestAuditState::Authorized)
        {
            return Err(PersistenceError::InvalidRequest(
                "network audit terminal transition requires a terminal state".to_owned(),
            ));
        }
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let mut record = load_network_audit(&transaction, request_id)?
                .ok_or(PersistenceError::NetworkAuditNotFound)?;
            if record.finished_at.is_some() {
                return Err(PersistenceError::NetworkAuditAlreadyFinished);
            }
            record.state = state;
            record.download_bytes = download_bytes;
            record.error_code = error_code;
            record.finished_at = Some(UtcTimestamp::now());
            update_network_audit(&transaction, &record)?;
            let mut event = PersistentEventDraft::system(
                if state == NetworkRequestAuditState::Blocked {
                    "NetworkRequestBlocked"
                } else {
                    "NetworkRequestFinished"
                },
                correlation_id,
                json!({
                    "network_request_id": request_id,
                    "state": state,
                    "upload_bytes": record.upload_bytes,
                    "download_bytes": download_bytes,
                    "error_code": record.error_code,
                }),
            );
            event.workspace_id = record.identity.workspace_id;
            event.session_id = record.identity.session_id;
            event.turn_task_id = record.identity.turn_task_id;
            event.agent_run_id = record.identity.agent_run_id;
            insert_event(&transaction, &event)?;
            transaction.commit()?;
            Ok(record)
        })
    }

    pub fn create_quarantine_record(
        &self,
        mut details: QuarantineRecordDetails,
        correlation_id: CorrelationId,
    ) -> Result<QuarantineRecordDetails, PersistenceError> {
        if details.record.state != QuarantineState::Quarantined {
            return Err(PersistenceError::InvalidRequest(
                "new quarantine record must be quarantined".to_owned(),
            ));
        }
        let artifact = details.artifact.as_ref().ok_or_else(|| {
            PersistenceError::InvalidRequest(
                "new quarantine record requires an Artifact Store reference".to_owned(),
            )
        })?;
        if artifact.scope != ArtifactScope::Quarantine
            || artifact.content_hash != details.record.content_hash
            || artifact.byte_length != details.record.byte_length
        {
            return Err(PersistenceError::InvalidRequest(
                "quarantine record does not match its Artifact Store object".to_owned(),
            ));
        }
        let artifact_id = artifact.artifact_id;
        details.record.source_url = sanitized_source_url(&details.record.source_url);
        details.record.content_disposition =
            details.record.content_disposition.as_ref().map(|_| "<redacted>".to_owned());
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            transaction.execute(
                "INSERT INTO quarantine_records(
                    quarantine_record_id, record_json, original_name, source_url,
                    media_type, content_disposition, detected_kinds_json,
                    content_hash_json, byte_length, relative_object_path, state,
                    created_at, resolved_at, artifact_id
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    details.record.id.to_string(),
                    serde_json::to_string(&details)?,
                    details.record.original_name,
                    details.record.source_url,
                    details.record.media_type,
                    details.record.content_disposition,
                    serde_json::to_string(&details.record.detected_kinds)?,
                    serde_json::to_string(&details.record.content_hash)?,
                    to_i64(details.record.byte_length, "quarantine byte length")?,
                    format!("artifact:{artifact_id}"),
                    quarantine_state_name(details.record.state),
                    details.record.created_at.to_string(),
                    details.record.resolved_at.map(|value| value.to_string()),
                    artifact_id.to_string(),
                ],
            )?;
            let event = PersistentEventDraft::system(
                "ExecutableQuarantined",
                correlation_id,
                json!({
                    "quarantine_id": details.record.id,
                    "original_name": details.record.original_name,
                    "detected_kinds": details.record.detected_kinds,
                    "content_hash": details.record.content_hash,
                    "byte_length": details.record.byte_length,
                }),
            );
            insert_event(&transaction, &event)?;
            transaction.commit()?;
            Ok(details)
        })
    }

    pub fn list_quarantine(&self) -> Result<Vec<QuarantineRecordDetails>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT record_json FROM quarantine_records
                 WHERE state='quarantined' ORDER BY created_at, quarantine_record_id",
            )?;
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .map(|row| {
                    let value = row?;
                    serde_json::from_str(&value).map_err(PersistenceError::from)
                })
                .collect()
        })
    }

    pub fn quarantine_details(
        &self,
        quarantine_id: QuarantineRecordId,
    ) -> Result<QuarantineRecordDetails, PersistenceError> {
        self.with_reader(|connection| {
            load_quarantine(connection, quarantine_id)?.ok_or(PersistenceError::QuarantineNotFound)
        })
    }

    pub fn begin_artifact_overwrite(
        &self,
        binding: ArtifactOverwriteJournalBinding,
        correlation_id: CorrelationId,
    ) -> Result<ArtifactOverwriteJournalRecord, PersistenceError> {
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            if session_workspace_id(&transaction, binding.session_id)? != binding.workspace_id {
                return Err(PersistenceError::ExternalGrantSessionMismatch);
            }
            let grant = load_external_grant(&transaction, binding.external_grant_id)?
                .ok_or(PersistenceError::ExternalGrantNotFound)?;
            if grant.session_id != binding.session_id
                || grant.workspace_id != binding.workspace_id
                || grant.lifecycle != ExternalGrantLifecycle::Active
            {
                return Err(PersistenceError::ExternalGrantStateConflict);
            }
            let now = UtcTimestamp::now();
            transaction.execute(
                "INSERT INTO artifact_overwrite_journal(
                    security_operation_id, workspace_id, session_id, external_grant_id,
                    binding_json, state, created_at, updated_at, error_code
                 ) VALUES (?1, ?2, ?3, ?4, ?5, 'prepared', ?6, ?6, NULL)",
                params![
                    binding.operation_id.to_string(),
                    binding.workspace_id.to_string(),
                    binding.session_id.to_string(),
                    binding.external_grant_id.to_string(),
                    serde_json::to_string(&binding)?,
                    now.to_string(),
                ],
            )?;
            let mut event = PersistentEventDraft::system(
                "ArtifactOverwritePrepared",
                correlation_id,
                json!({
                    "operation_id": binding.operation_id,
                    "external_grant_id": binding.external_grant_id,
                    "relative_path": binding.relative_path,
                    "replacement_content_hash": binding.replacement_content_hash,
                }),
            );
            event.workspace_id = Some(binding.workspace_id);
            event.session_id = Some(binding.session_id);
            insert_event(&transaction, &event)?;
            transaction.commit()?;
            Ok(ArtifactOverwriteJournalRecord {
                binding,
                state: ArtifactOverwriteJournalState::Prepared,
                error_code: None,
            })
        })
    }

    pub fn transition_artifact_overwrite(
        &self,
        operation_id: SecurityOperationId,
        next: ArtifactOverwriteJournalState,
        error_code: Option<String>,
        correlation_id: CorrelationId,
    ) -> Result<ArtifactOverwriteJournalRecord, PersistenceError> {
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let (next_name, predicate) = match next {
                ArtifactOverwriteJournalState::Applying => ("applying", "state='prepared'"),
                ArtifactOverwriteJournalState::Published => ("published", "state='applying'"),
                ArtifactOverwriteJournalState::Completed => ("completed", "state='published'"),
                ArtifactOverwriteJournalState::Aborted => {
                    ("aborted", "state IN ('prepared', 'applying')")
                }
                ArtifactOverwriteJournalState::NeedsReconcile => {
                    ("needs_reconcile", "state IN ('applying', 'published')")
                }
                ArtifactOverwriteJournalState::Prepared
                | ArtifactOverwriteJournalState::Interrupted => {
                    return Err(PersistenceError::InvalidRequest(
                        "artifact overwrite transition is Core-recovery owned".to_owned(),
                    ));
                }
            };
            let sql = format!(
                "UPDATE artifact_overwrite_journal
                 SET state=?2, updated_at=?3, error_code=?4
                 WHERE security_operation_id=?1 AND {predicate}"
            );
            if transaction.execute(
                &sql,
                params![
                    operation_id.to_string(),
                    next_name,
                    UtcTimestamp::now().to_string(),
                    error_code,
                ],
            )? != 1
            {
                return Err(PersistenceError::InvalidRequest(
                    "artifact overwrite Journal transition was stale".to_owned(),
                ));
            }
            let record =
                load_artifact_overwrite_journal(&transaction, operation_id)?.ok_or_else(|| {
                    PersistenceError::InvalidRequest(
                        "artifact overwrite Journal disappeared".to_owned(),
                    )
                })?;
            let mut event = PersistentEventDraft::system(
                "ArtifactOverwriteStateChanged",
                correlation_id,
                json!({ "operation_id": operation_id, "state": next_name }),
            );
            event.workspace_id = Some(record.binding.workspace_id);
            event.session_id = Some(record.binding.session_id);
            insert_event(&transaction, &event)?;
            transaction.commit()?;
            Ok(record)
        })
    }

    pub fn artifact_overwrite_journal(
        &self,
        operation_id: SecurityOperationId,
    ) -> Result<ArtifactOverwriteJournalRecord, PersistenceError> {
        self.with_reader(|connection| {
            load_artifact_overwrite_journal(connection, operation_id)?.ok_or_else(|| {
                PersistenceError::InvalidRequest("artifact overwrite Journal not found".to_owned())
            })
        })
    }

    pub fn authorize_quarantine_release(
        &self,
        binding: &QuarantineReleaseBinding,
        approval_id: ApprovalRequestId,
    ) -> Result<QuarantineRecordDetails, PersistenceError> {
        self.with_reader(|connection| {
            let details = load_quarantine(connection, binding.quarantine_id)?
                .ok_or(PersistenceError::QuarantineNotFound)?;
            if details.record.state != QuarantineState::Quarantined
                || details.record.content_hash != binding.content_hash
            {
                return Err(PersistenceError::QuarantineReleaseNotApproved);
            }
            let approval = load_approval(connection, approval_id)?
                .ok_or(PersistenceError::ApprovalNotFound)?;
            let fingerprint = binding.operation_fingerprint()?;
            if approval.state != ApprovalState::Approved
                || !approval.capabilities.contains(&ResolvedCapability::DownloadWindowsExecutable)
                || !approval.operation_fingerprints.contains(&fingerprint)
            {
                return Err(PersistenceError::QuarantineReleaseNotApproved);
            }
            Ok(details)
        })
    }

    pub fn consume_quarantine_release_authorization(
        &self,
        binding: QuarantineReleaseBinding,
        approval_id: ApprovalRequestId,
        correlation_id: CorrelationId,
    ) -> Result<QuarantineRecordDetails, PersistenceError> {
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let details = load_quarantine(&transaction, binding.quarantine_id)?
                .ok_or(PersistenceError::QuarantineNotFound)?;
            let fingerprint = binding.operation_fingerprint()?;
            let approval = load_approval(&transaction, approval_id)?
                .ok_or(PersistenceError::ApprovalNotFound)?;
            if details.record.state != QuarantineState::Quarantined
                || details.record.content_hash != binding.content_hash
                || approval.state != ApprovalState::Approved
                || approval.identity.workspace_id != Some(binding.workspace_id)
                || approval.identity.session_id != Some(binding.session_id)
                || !approval.capabilities.contains(&ResolvedCapability::DownloadWindowsExecutable)
                || !approval.operation_fingerprints.contains(&fingerprint)
            {
                return Err(PersistenceError::QuarantineReleaseNotApproved);
            }
            let grant_ids = transaction
                .query_row(
                    "SELECT permission_grant_ids_json FROM approval_decisions
                     WHERE approval_request_id=?1",
                    [approval_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .ok_or(PersistenceError::QuarantineReleaseNotApproved)?;
            let grant_ids: Vec<PermissionGrantId> = serde_json::from_str(&grant_ids)?;
            let now = UtcTimestamp::now();
            let mut matching = None;
            for grant_id in grant_ids {
                let Some(grant) = load_grant(&transaction, grant_id)? else {
                    continue;
                };
                if grant.lifetime == PermissionGrantLifetime::Once
                    && grant.workspace_id == binding.workspace_id
                    && grant.session_id == Some(binding.session_id)
                    && grant.operation_fingerprint.as_ref() == Some(&fingerprint)
                    && grant.revoked_at.is_none()
                    && grant.consumed_at.is_none()
                    && grant.expires_at.is_none_or(|expiry| expiry > now)
                {
                    matching = Some(grant);
                    break;
                }
            }
            let mut grant = matching.ok_or(PersistenceError::QuarantineReleaseNotApproved)?;
            grant.consumed_at = Some(now);
            update_grant(&transaction, &grant)?;
            let inserted = transaction.execute(
                "INSERT INTO quarantine_release_journal(
                    quarantine_record_id, approval_request_id, binding_json,
                    state, created_at, updated_at, error_code
                 ) VALUES (?1, ?2, ?3, 'authorized', ?4, ?4, NULL)
                 ON CONFLICT(quarantine_record_id) DO UPDATE SET
                    approval_request_id=excluded.approval_request_id,
                    binding_json=excluded.binding_json,
                    state='authorized', created_at=excluded.created_at,
                    updated_at=excluded.updated_at, error_code=NULL
                 WHERE quarantine_release_journal.state IN ('completed', 'failed', 'interrupted')",
                params![
                    binding.quarantine_id.to_string(),
                    approval_id.to_string(),
                    serde_json::to_string(&binding)?,
                    now.to_string(),
                ],
            )?;
            if inserted != 1 {
                return Err(PersistenceError::InvalidRequest(
                    "quarantine release already has an active Journal entry".to_owned(),
                ));
            }
            let mut event = PersistentEventDraft::system(
                "QuarantineReleaseAuthorizationConsumed",
                correlation_id,
                json!({
                    "quarantine_id": binding.quarantine_id,
                    "approval_id": approval_id,
                    "permission_grant_id": grant.id,
                    "operation_fingerprint": fingerprint,
                }),
            );
            event.workspace_id = Some(binding.workspace_id);
            event.session_id = Some(binding.session_id);
            event.actor = EventActor::User;
            insert_event(&transaction, &event)?;
            transaction.commit()?;
            Ok(details)
        })
    }

    pub fn mark_quarantine_release_applying(
        &self,
        quarantine_id: QuarantineRecordId,
    ) -> Result<(), PersistenceError> {
        self.dispatch(move |connection| {
            if connection.execute(
                "UPDATE quarantine_release_journal
                 SET state='applying', updated_at=?2
                 WHERE quarantine_record_id=?1 AND state='authorized'",
                params![quarantine_id.to_string(), UtcTimestamp::now().to_string()],
            )? != 1
            {
                return Err(PersistenceError::InvalidRequest(
                    "quarantine release Journal was not authorized".to_owned(),
                ));
            }
            Ok(())
        })
    }

    pub fn quarantine_release_journal_state(
        &self,
        quarantine_id: QuarantineRecordId,
    ) -> Result<Option<String>, PersistenceError> {
        self.with_reader(|connection| {
            connection
                .query_row(
                    "SELECT state FROM quarantine_release_journal WHERE quarantine_record_id=?1",
                    [quarantine_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(PersistenceError::from)
        })
    }

    pub fn mark_quarantine_release_published(
        &self,
        quarantine_id: QuarantineRecordId,
    ) -> Result<(), PersistenceError> {
        self.dispatch(move |connection| {
            if connection.execute(
                "UPDATE quarantine_release_journal
                 SET state='published', updated_at=?2
                 WHERE quarantine_record_id=?1 AND state='applying'",
                params![quarantine_id.to_string(), UtcTimestamp::now().to_string()],
            )? != 1
            {
                return Err(PersistenceError::InvalidRequest(
                    "quarantine release Journal was not applying".to_owned(),
                ));
            }
            Ok(())
        })
    }

    pub fn interrupt_quarantine_release(
        &self,
        quarantine_id: QuarantineRecordId,
        error_code: String,
    ) -> Result<(), PersistenceError> {
        self.dispatch(move |connection| {
            connection.execute(
                "UPDATE quarantine_release_journal
                 SET state='interrupted', updated_at=?2, error_code=?3
                 WHERE quarantine_record_id=?1 AND state IN ('applying', 'published')",
                params![quarantine_id.to_string(), UtcTimestamp::now().to_string(), error_code],
            )?;
            Ok(())
        })
    }

    pub fn fail_quarantine_release(
        &self,
        quarantine_id: QuarantineRecordId,
        error_code: String,
    ) -> Result<(), PersistenceError> {
        self.dispatch(move |connection| {
            connection.execute(
                "UPDATE quarantine_release_journal
                 SET state='failed', updated_at=?2, error_code=?3
                 WHERE quarantine_record_id=?1 AND state IN ('authorized', 'applying')",
                params![quarantine_id.to_string(), UtcTimestamp::now().to_string(), error_code],
            )?;
            Ok(())
        })
    }

    pub fn resolve_quarantine(
        &self,
        quarantine_id: QuarantineRecordId,
        state: QuarantineState,
        correlation_id: CorrelationId,
    ) -> Result<QuarantineRecordDetails, PersistenceError> {
        if state == QuarantineState::Quarantined {
            return Err(PersistenceError::InvalidRequest(
                "quarantine resolution requires released or discarded".to_owned(),
            ));
        }
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let mut details = load_quarantine(&transaction, quarantine_id)?
                .ok_or(PersistenceError::QuarantineNotFound)?;
            if details.record.state != QuarantineState::Quarantined {
                return Err(PersistenceError::InvalidRequest(
                    "quarantine record is already resolved".to_owned(),
                ));
            }
            details.record.state = state;
            details.record.resolved_at = Some(UtcTimestamp::now());
            transaction.execute(
                "UPDATE quarantine_records
                 SET record_json=?1, state=?2, resolved_at=?3
                 WHERE quarantine_record_id=?4",
                params![
                    serde_json::to_string(&details)?,
                    quarantine_state_name(state),
                    details.record.resolved_at.map(|value| value.to_string()),
                    quarantine_id.to_string(),
                ],
            )?;
            if state == QuarantineState::Released
                && transaction.execute(
                    "UPDATE quarantine_release_journal
                     SET state='completed', updated_at=?2, error_code=NULL
                     WHERE quarantine_record_id=?1 AND state='published'",
                    params![quarantine_id.to_string(), UtcTimestamp::now().to_string()],
                )? != 1
            {
                return Err(PersistenceError::InvalidRequest(
                    "quarantine release Journal was not published".to_owned(),
                ));
            }
            let event = PersistentEventDraft::system(
                if state == QuarantineState::Released {
                    "QuarantineReleased"
                } else {
                    "QuarantineDiscarded"
                },
                correlation_id,
                json!({
                    "quarantine_id": quarantine_id,
                    "state": state,
                    "content_hash": details.record.content_hash,
                }),
            );
            insert_event(&transaction, &event)?;
            transaction.commit()?;
            Ok(details)
        })
    }

    pub(crate) fn finish_security_mutation<T>(
        &self,
        mutation: SecurityMutation<T>,
    ) -> Result<T, PersistenceError> {
        for projection in mutation.projections {
            self.publish_and_mark_security(projection)?;
        }
        Ok(mutation.value)
    }

    fn publish_and_mark_security(
        &self,
        projection: CommittedProjection<SecurityProjectionPayload>,
    ) -> Result<(), PersistenceError> {
        self.publish_security(projection.clone())?;
        let outbox_id = projection.outbox_id;
        self.dispatch(move |connection| {
            connection.execute(
                "UPDATE projection_outbox SET published_at=?1 WHERE outbox_id=?2",
                params![UtcTimestamp::now().to_string(), outbox_id],
            )?;
            Ok(())
        })
    }

    /// The same finish, with each outbox acknowledgement awaited instead of waited for.
    ///
    /// The steps and their order are [`Self::publish_and_mark_security`]'s unchanged:
    /// hand the projection to the ordinary subscriber path, then mark its outbox row
    /// published. Only the transport of the acknowledgement differs - one `WorkClass::Sync`
    /// job on the same writer, whose result the caller awaits - so an async mode change can
    /// keep holding this session's authority write across publication without parking a
    /// runtime thread on the storage layer.
    async fn finish_security_mutation_awaitable<T>(
        &self,
        mutation: SecurityMutation<T>,
    ) -> Result<T, PersistenceError> {
        for projection in mutation.projections {
            self.publish_security(projection.clone())?;
            let outbox_id = projection.outbox_id;
            let receipt = self.dispatch_awaitable(
                format!("security-outbox:{outbox_id}"),
                move |connection| {
                    connection.execute(
                        "UPDATE projection_outbox SET published_at=?1 WHERE outbox_id=?2",
                        params![UtcTimestamp::now().to_string(), outbox_id],
                    )?;
                    Ok(())
                },
            )?;
            receipt.await.map_err(|_| PersistenceError::WriterResponseClosed)??;
        }
        Ok(mutation.value)
    }
}

fn session_workspace_id(
    connection: &Connection,
    session_id: SessionId,
) -> Result<WorkspaceId, PersistenceError> {
    connection
        .query_row(
            "SELECT workspace_id FROM sessions WHERE session_id=?1",
            [session_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|value| WorkspaceId::from_str(&value).map_err(PersistenceError::from))
        .transpose()?
        .ok_or(PersistenceError::PermissionStateNotFound)
}

fn load_session_permission_projection(
    connection: &Connection,
    session_id: SessionId,
) -> Result<SessionPermissionProjection, PersistenceError> {
    let (mode, revision) = connection
        .query_row(
            "SELECT mode, revision FROM session_permission_states WHERE session_id=?1",
            [session_id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?
        .ok_or(PersistenceError::PermissionStateNotFound)?;
    let workspace_id = session_workspace_id(connection, session_id)?;
    let active_grant_count = connection.query_row(
        "SELECT COUNT(*) FROM permission_grants
         WHERE workspace_id=?1
           AND (session_id IS NULL OR session_id=?2)
           AND revoked_at IS NULL
           AND consumed_at IS NULL
           AND (expires_at IS NULL OR expires_at>?3)",
        params![workspace_id.to_string(), session_id.to_string(), UtcTimestamp::now().to_string(),],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(SessionPermissionProjection {
        session_id,
        mode: parse_permission_mode(&mode)?,
        revision: from_i64(revision, "permission revision")?,
        transition: transition::load(
            connection,
            session_id,
            from_i64(revision, "permission revision")?,
        )?,
        active_grant_count: u32::try_from(active_grant_count).map_err(|_| {
            PersistenceError::InvalidRequest("active grant count exceeds u32".to_owned())
        })?,
    })
}

fn validate_external_grant(record: &ExternalGrantRecord) -> Result<(), PersistenceError> {
    if record.revalidation_revision == 0
        || record.selected_root.display.is_empty()
        || record.canonical_root.display.is_empty()
        || record.selected_root.utf16
            != record.selected_root.display.encode_utf16().collect::<Vec<_>>()
        || record.canonical_root.utf16
            != record.canonical_root.display.encode_utf16().collect::<Vec<_>>()
        || record.root_identity.volume_serial_number.is_empty()
        || record.root_identity.file_id.is_empty()
        || record.capabilities.is_empty()
        || (record.target_kind == ExternalGrantTargetKind::File && record.recursive_scope)
    {
        return Err(PersistenceError::InvalidRequest(
            "external grant verification facts are invalid".to_owned(),
        ));
    }
    let mut capabilities = record.capabilities.clone();
    capabilities.sort();
    capabilities.dedup();
    if capabilities != record.capabilities {
        return Err(PersistenceError::InvalidRequest(
            "external grant capabilities must be sorted and unique".to_owned(),
        ));
    }
    let lifecycle_is_valid = match record.lifecycle {
        ExternalGrantLifecycle::Active => {
            record.suspended_at.is_none() && record.revoked_at.is_none()
        }
        ExternalGrantLifecycle::Suspended => {
            record.suspended_at.is_some() && record.revoked_at.is_none()
        }
        ExternalGrantLifecycle::Revoked => record.revoked_at.is_some(),
    };
    if !lifecycle_is_valid {
        return Err(PersistenceError::InvalidRequest(
            "external grant lifecycle timestamps are invalid".to_owned(),
        ));
    }
    let term_is_valid = match record.term {
        ExternalGrantTerm::Turn => record.turn_task_id.is_some(),
        ExternalGrantTerm::Session | ExternalGrantTerm::Workspace => record.turn_task_id.is_none(),
    };
    if !term_is_valid {
        return Err(PersistenceError::InvalidRequest(
            "external grant term does not match its turn binding".to_owned(),
        ));
    }
    Ok(())
}

fn external_grant_term_name(term: ExternalGrantTerm) -> &'static str {
    match term {
        ExternalGrantTerm::Session => "session",
        ExternalGrantTerm::Workspace => "workspace",
        ExternalGrantTerm::Turn => "turn",
    }
}

fn parse_external_grant_term(value: &str) -> Result<ExternalGrantTerm, PersistenceError> {
    match value {
        "session" => Ok(ExternalGrantTerm::Session),
        "workspace" => Ok(ExternalGrantTerm::Workspace),
        "turn" => Ok(ExternalGrantTerm::Turn),
        _ => Err(PersistenceError::InvalidRequest(format!("unknown external grant term {value}"))),
    }
}

fn insert_external_grant(
    transaction: &Transaction<'_>,
    record: &ExternalGrantRecord,
    correlation_id: CorrelationId,
) -> Result<(), PersistenceError> {
    let session_workspace = session_workspace_id(transaction, record.session_id)?;
    if session_workspace != record.workspace_id {
        return Err(PersistenceError::ExternalGrantSessionMismatch);
    }
    if let Some(permission_grant_id) = record.permission_grant_id {
        let grant = load_grant(transaction, permission_grant_id)?
            .ok_or(PersistenceError::PermissionGrantNotFound)?;
        if grant.workspace_id != record.workspace_id
            || grant.lifetime != PermissionGrantLifetime::Workspace
            || grant.revoked_at.is_some()
            || grant.consumed_at.is_some()
            || grant.expires_at.is_some_and(|expiry| expiry <= UtcTimestamp::now())
        {
            return Err(PersistenceError::ExternalGrantStateConflict);
        }
    }
    let workspace_authorization_epoch = if record.permission_grant_id.is_some() {
        Some(transaction.query_row(
            "SELECT authorization_epoch FROM workspaces WHERE workspace_id=?1",
            [record.workspace_id.to_string()],
            |row| row.get::<_, i64>(0),
        )?)
    } else {
        None
    };
    transaction.execute(
        "INSERT INTO external_grants(
            external_grant_id, session_id, workspace_id, permission_grant_id,
            selected_root, canonical_root, root_identity_json, root_is_directory,
            capabilities_json, recursive_scope, lifecycle, revalidation_revision,
            created_at, updated_at, suspended_at, revoked_at,
            workspace_authorization_epoch, expires_at, turn_task_id, scope_lifetime
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        params![
            record.id.to_string(),
            record.session_id.to_string(),
            record.workspace_id.to_string(),
            record.permission_grant_id.map(|value| value.to_string()),
            record.selected_root.display.clone(),
            record.canonical_root.display.clone(),
            serde_json::to_string(&record.root_identity)?,
            record.target_kind == ExternalGrantTargetKind::Directory,
            serde_json::to_string(&record.capabilities)?,
            record.recursive_scope,
            external_grant_lifecycle_name(record.lifecycle),
            to_i64(record.revalidation_revision, "external grant revision")?,
            record.created_at.to_string(),
            record.updated_at.to_string(),
            record.suspended_at.map(|value| value.to_string()),
            record.revoked_at.map(|value| value.to_string()),
            workspace_authorization_epoch,
            record.expires_at.map(|value| value.to_string()),
            record.turn_task_id.map(|value| value.to_string()),
            external_grant_term_name(record.term),
        ],
    )?;
    let mut event = PersistentEventDraft::system(
        "ExternalGrantIssued",
        correlation_id,
        json!({
            "external_grant_id": record.id,
            "session_id": record.session_id,
            "workspace_id": record.workspace_id,
            "permission_grant_id": record.permission_grant_id,
            "capabilities": record.capabilities,
            "recursive_scope": record.recursive_scope,
            "term": external_grant_term_name(record.term),
            "expires_at": record.expires_at,
        }),
    );
    event.workspace_id = Some(record.workspace_id);
    event.session_id = Some(record.session_id);
    event.actor = EventActor::User;
    insert_event(transaction, &event)?;
    Ok(())
}

fn directory_decision_grant(
    connection: &Connection,
    approval_id: &ApprovalRequestId,
    identity_json: &str,
) -> Result<Option<ExternalGrantId>, PersistenceError> {
    let id = connection
        .query_row(
            "SELECT external_grant_id FROM external_grant_decisions
             WHERE approval_id=?1 AND root_identity_json=?2",
            params![approval_id.to_string(), identity_json],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    match id {
        None => Ok(None),
        Some(value) => Ok(Some(ExternalGrantId::from_str(&value)?)),
    }
}

fn insert_directory_decision(
    transaction: &Transaction<'_>,
    approval_id: &ApprovalRequestId,
    identity_json: &str,
    grant_id: ExternalGrantId,
    operation_fingerprint: &str,
    session_id: SessionId,
) -> Result<(), PersistenceError> {
    transaction.execute(
        "INSERT INTO external_grant_decisions(
            approval_id, root_identity_json, external_grant_id,
            operation_fingerprint, session_id, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            approval_id.to_string(),
            identity_json,
            grant_id.to_string(),
            operation_fingerprint,
            session_id.to_string(),
            UtcTimestamp::now().to_string(),
        ],
    )?;
    Ok(())
}

fn reusable_directory_grant(
    connection: &Connection,
    candidate: &ExternalGrantRecord,
) -> Result<Option<ExternalGrantRecord>, PersistenceError> {
    let mut statement = connection.prepare(
        "SELECT external_grant_id FROM external_grants
         WHERE workspace_id=?1 AND lifecycle='active'
           AND (session_id=?2 OR scope_lifetime='workspace')
         ORDER BY created_at, external_grant_id",
    )?;
    let ids = statement
        .query_map(
            params![candidate.workspace_id.to_string(), candidate.session_id.to_string()],
            |row| row.get::<_, String>(0),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let now = UtcTimestamp::now();
    for value in ids {
        let id = ExternalGrantId::from_str(&value)?;
        let Some(record) = load_external_grant(connection, id)? else { continue };
        let live = record.lifecycle == ExternalGrantLifecycle::Active
            && record.expires_at.is_none_or(|expiry| expiry > now);
        let same = record.target_kind == ExternalGrantTargetKind::Directory
            && record.root_identity == candidate.root_identity
            && record.recursive_scope == candidate.recursive_scope
            && record.capabilities == candidate.capabilities
            && record.term == candidate.term
            && record.turn_task_id == candidate.turn_task_id
            && record.workspace_id == candidate.workspace_id;
        if live && same {
            return Ok(Some(record));
        }
    }
    Ok(None)
}

fn load_external_grant(
    connection: &Connection,
    grant_id: ExternalGrantId,
) -> Result<Option<ExternalGrantRecord>, PersistenceError> {
    let row = connection
        .query_row(
            "SELECT session_id, workspace_id, permission_grant_id,
                    selected_root, canonical_root, root_identity_json,
                    root_is_directory, capabilities_json, recursive_scope,
                    lifecycle, revalidation_revision, created_at, updated_at,
                    suspended_at, revoked_at, expires_at, turn_task_id, scope_lifetime
             FROM external_grants WHERE external_grant_id=?1",
            [grant_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, bool>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, bool>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, Option<String>>(14)?,
                    row.get::<_, Option<String>>(15)?,
                    row.get::<_, Option<String>>(16)?,
                    row.get::<_, String>(17)?,
                ))
            },
        )
        .optional()?;
    row.map(
        |(
            session_id,
            workspace_id,
            permission_grant_id,
            selected_root,
            canonical_root,
            root_identity_json,
            root_is_directory,
            capabilities_json,
            recursive_scope,
            lifecycle,
            revalidation_revision,
            created_at,
            updated_at,
            suspended_at,
            revoked_at,
            expires_at,
            turn_task_id,
            scope_lifetime,
        )| {
            Ok(ExternalGrantRecord {
                id: grant_id,
                session_id: SessionId::from_str(&session_id)?,
                workspace_id: WorkspaceId::from_str(&workspace_id)?,
                permission_grant_id: permission_grant_id
                    .map(|value| PermissionGrantId::from_str(&value))
                    .transpose()?,
                selected_root: canonical_path_from_persisted(selected_root),
                canonical_root: canonical_path_from_persisted(canonical_root),
                root_identity: serde_json::from_str(&root_identity_json)?,
                target_kind: if root_is_directory {
                    ExternalGrantTargetKind::Directory
                } else {
                    ExternalGrantTargetKind::File
                },
                capabilities: serde_json::from_str(&capabilities_json)?,
                recursive_scope,
                lifecycle: parse_external_grant_lifecycle(&lifecycle)?,
                revalidation_revision: from_i64(revalidation_revision, "external grant revision")?,
                created_at: parse_timestamp(&created_at)?,
                updated_at: parse_timestamp(&updated_at)?,
                suspended_at: suspended_at.map(|value| parse_timestamp(&value)).transpose()?,
                revoked_at: revoked_at.map(|value| parse_timestamp(&value)).transpose()?,
                expires_at: expires_at.map(|value| parse_timestamp(&value)).transpose()?,
                turn_task_id: turn_task_id.map(|value| TurnTaskId::from_str(&value)).transpose()?,
                term: parse_external_grant_term(&scope_lifetime)?,
            })
        },
    )
    .transpose()
}

fn load_artifact_overwrite_journal(
    connection: &Connection,
    operation_id: SecurityOperationId,
) -> Result<Option<ArtifactOverwriteJournalRecord>, PersistenceError> {
    connection
        .query_row(
            "SELECT binding_json, state, error_code
             FROM artifact_overwrite_journal WHERE security_operation_id=?1",
            [operation_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?
        .map(|(binding, state, error_code)| {
            Ok(ArtifactOverwriteJournalRecord {
                binding: serde_json::from_str(&binding)?,
                state: parse_artifact_overwrite_state(&state)?,
                error_code,
            })
        })
        .transpose()
}

fn parse_artifact_overwrite_state(
    value: &str,
) -> Result<ArtifactOverwriteJournalState, PersistenceError> {
    match value {
        "prepared" => Ok(ArtifactOverwriteJournalState::Prepared),
        "applying" => Ok(ArtifactOverwriteJournalState::Applying),
        "published" => Ok(ArtifactOverwriteJournalState::Published),
        "completed" => Ok(ArtifactOverwriteJournalState::Completed),
        "aborted" => Ok(ArtifactOverwriteJournalState::Aborted),
        "interrupted" => Ok(ArtifactOverwriteJournalState::Interrupted),
        "needs_reconcile" => Ok(ArtifactOverwriteJournalState::NeedsReconcile),
        _ => Err(PersistenceError::InvalidRequest(
            "persisted artifact overwrite Journal state is invalid".to_owned(),
        )),
    }
}

fn canonical_path_from_persisted(display: String) -> CanonicalPath {
    CanonicalPath { utf16: display.encode_utf16().collect(), display }
}

const fn external_grant_lifecycle_name(lifecycle: ExternalGrantLifecycle) -> &'static str {
    match lifecycle {
        ExternalGrantLifecycle::Active => "active",
        ExternalGrantLifecycle::Suspended => "suspended",
        ExternalGrantLifecycle::Revoked => "revoked",
    }
}

fn parse_external_grant_lifecycle(value: &str) -> Result<ExternalGrantLifecycle, PersistenceError> {
    match value {
        "active" => Ok(ExternalGrantLifecycle::Active),
        "suspended" => Ok(ExternalGrantLifecycle::Suspended),
        "revoked" => Ok(ExternalGrantLifecycle::Revoked),
        _ => Err(PersistenceError::InvalidRequest(format!(
            "invalid external grant lifecycle: {value}"
        ))),
    }
}

fn active_grant_ids(
    connection: &Connection,
    workspace_id: WorkspaceId,
    session_id: SessionId,
    turn_task_id: Option<TurnTaskId>,
) -> Result<Vec<PermissionGrantId>, PersistenceError> {
    let mut statement = connection.prepare(
        "SELECT permission_grant_id FROM permission_grants
         WHERE workspace_id=?1
           AND (session_id IS NULL OR session_id=?2)
           AND (turn_task_id IS NULL OR turn_task_id=?3)
           AND revoked_at IS NULL
           AND consumed_at IS NULL
           AND (expires_at IS NULL OR expires_at>?4)
         ORDER BY created_at, permission_grant_id",
    )?;
    statement
        .query_map(
            params![
                workspace_id.to_string(),
                session_id.to_string(),
                turn_task_id.map(|id| id.to_string()),
                UtcTimestamp::now().to_string(),
            ],
            |row| row.get::<_, String>(0),
        )?
        .map(|row| {
            let value = row?;
            PermissionGrantId::from_str(&value).map_err(PersistenceError::from)
        })
        .collect()
}

fn parse_snapshot(
    id: PermissionSnapshotId,
    row: (String, String, String, i64, String, String),
) -> Result<PermissionSnapshot, PersistenceError> {
    Ok(PermissionSnapshot {
        id,
        session_id: SessionId::from_str(&row.0)?,
        workspace_id: WorkspaceId::from_str(&row.1)?,
        mode: parse_permission_mode(&row.2)?,
        session_permission_revision: from_i64(row.3, "permission revision")?,
        active_grant_ids: serde_json::from_str(&row.4)?,
        created_at: parse_timestamp(&row.5)?,
    })
}

fn insert_approval(
    transaction: &Transaction<'_>,
    request: &ApprovalRequest,
) -> Result<(), PersistenceError> {
    transaction.execute(
        "INSERT INTO approval_requests(
            approval_request_id, request_json, identity_json, title, model_purpose,
            actual_operation_summary, source_summary, source_json,
            source_fingerprint_json, capabilities_json, scopes_json, risk_flags_json,
            allowed_decisions_json, permission_snapshot_id, request_revision, state,
            created_at, resolved_at, invalidation_reason
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
            ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19
         )",
        params![
            request.id.to_string(),
            serde_json::to_string(request)?,
            serde_json::to_string(&request.identity)?,
            request.title,
            request.model_purpose,
            request.actual_operation_summary,
            request.source_summary,
            serde_json::to_string(&request.source)?,
            request.source_fingerprint.as_ref().map(serde_json::to_string).transpose()?,
            serde_json::to_string(&request.capabilities)?,
            serde_json::to_string(&request.scopes)?,
            serde_json::to_string(&request.risk_flags)?,
            serde_json::to_string(&request.allowed_decisions)?,
            request.permission_snapshot_id.to_string(),
            to_i64(request.request_revision, "approval revision")?,
            approval_state_name(request.state),
            request.created_at.to_string(),
            request.resolved_at.map(|value| value.to_string()),
            request.invalidation_reason,
        ],
    )?;
    for (index, (operation_id, fingerprint)) in
        request.operation_ids.iter().zip(&request.operation_fingerprints).enumerate()
    {
        transaction.execute(
            "INSERT INTO approval_request_operations(
                approval_request_id, operation_index, security_operation_id,
                operation_fingerprint_json
             ) VALUES (?1, ?2, ?3, ?4)",
            params![
                request.id.to_string(),
                i64::try_from(index).map_err(|_| PersistenceError::InvalidRequest(
                    "too many approval operations".to_owned(),
                ))?,
                operation_id.to_string(),
                serde_json::to_string(fingerprint)?,
            ],
        )?;
    }
    Ok(())
}

fn load_approval(
    connection: &Connection,
    approval_id: ApprovalRequestId,
) -> Result<Option<ApprovalRequest>, PersistenceError> {
    connection
        .query_row(
            "SELECT request_json FROM approval_requests WHERE approval_request_id=?1",
            [approval_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|value| serde_json::from_str(&value).map_err(PersistenceError::from))
        .transpose()
}

fn load_pending_approvals(
    connection: &Connection,
) -> Result<Vec<ApprovalRequest>, PersistenceError> {
    let mut statement = connection.prepare(
        "SELECT request_json FROM approval_requests
         WHERE state='pending' ORDER BY created_at, approval_request_id",
    )?;
    statement
        .query_map([], |row| row.get::<_, String>(0))?
        .map(|row| {
            let value = row?;
            serde_json::from_str(&value).map_err(PersistenceError::from)
        })
        .collect()
}

fn load_trust_by_source(
    connection: &Connection,
    source: &SecuritySource,
) -> Result<Option<ExternalTrustRecord>, PersistenceError> {
    connection
        .query_row(
            "SELECT record_json FROM external_trust_records WHERE source_key=?1",
            [source.stable_key()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|value| serde_json::from_str(&value).map_err(PersistenceError::from))
        .transpose()
}

fn load_sandbox_execution(
    connection: &Connection,
    execution_id: SandboxExecutionId,
) -> Result<Option<SandboxExecutionRecord>, PersistenceError> {
    connection
        .query_row(
            "SELECT plan_json, state, process_id, exit_code, created_at, updated_at, terminal_at
             FROM sandbox_executions WHERE sandbox_execution_id=?1",
            [execution_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<u32>>(2)?,
                    row.get::<_, Option<u32>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            },
        )
        .optional()?
        .map(|row| {
            Ok(SandboxExecutionRecord {
                plan: serde_json::from_str(&row.0)?,
                state: parse_sandbox_state(&row.1)?,
                process_id: row.2,
                exit_code: row.3,
                created_at: parse_timestamp(&row.4)?,
                updated_at: parse_timestamp(&row.5)?,
                terminal_at: row.6.as_deref().map(parse_timestamp).transpose()?,
            })
        })
        .transpose()
}

fn load_acl_journal(
    connection: &Connection,
    journal_id: SandboxAclJournalId,
) -> Result<Option<SandboxAclJournalEntry>, PersistenceError> {
    connection
        .query_row(
            "SELECT sandbox_execution_id, target_identity_json, target_path_json,
                    original_dacl_sddl, original_dacl_hash, appcontainer_profile_name,
                    appcontainer_sid, access_mask, recursive, state, created_at, updated_at,
                    error_code
             FROM sandbox_acl_journal WHERE sandbox_acl_journal_id=?1",
            [journal_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, bool>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, Option<String>>(12)?,
                ))
            },
        )
        .optional()?
        .map(|row| {
            Ok(SandboxAclJournalEntry {
                id: journal_id,
                execution_id: SandboxExecutionId::from_str(&row.0)?,
                target_identity: serde_json::from_str(&row.1)?,
                target_path: serde_json::from_str(&row.2)?,
                original_dacl_sddl: row.3,
                original_dacl_hash: serde_json::from_str(&row.4)?,
                appcontainer_profile_name: row.5,
                appcontainer_sid: row.6,
                access: parse_sandbox_access(row.7)?,
                recursive: row.8,
                state: parse_acl_journal_state(&row.9)?,
                created_at: parse_timestamp(&row.10)?,
                updated_at: parse_timestamp(&row.11)?,
                error_code: row.12,
            })
        })
        .transpose()
}

fn insert_network_grant(
    transaction: &Transaction<'_>,
    grant: &NetworkGrant,
    approval_decision_id: ApprovalDecisionId,
) -> Result<(), PersistenceError> {
    transaction.execute(
        "INSERT INTO network_grants(
            network_grant_id, grant_json, identity_scope, identity_scope_id, lifetime,
            origins_json, destination_classes_json, methods_json,
            allowed_data_classes_json, max_upload_bytes, max_download_bytes,
            source_fingerprint_json, expires_at, revoked_at, approval_decision_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            grant.id.to_string(),
            serde_json::to_string(grant)?,
            network_identity_scope_name(grant.identity_scope),
            grant.identity_scope_id,
            grant_lifetime_name(grant.lifetime),
            serde_json::to_string(&grant.origins)?,
            serde_json::to_string(&grant.destination_classes)?,
            serde_json::to_string(&grant.methods)?,
            serde_json::to_string(&grant.allowed_data_classes)?,
            grant
                .max_upload_bytes
                .map(|value| to_i64(value, "network upload limit"))
                .transpose()?,
            grant
                .max_download_bytes
                .map(|value| to_i64(value, "network download limit"))
                .transpose()?,
            grant.source_fingerprint.as_ref().map(serde_json::to_string).transpose()?,
            grant.expires_at.map(|value| value.to_string()),
            grant.revoked_at.map(|value| value.to_string()),
            approval_decision_id.to_string(),
        ],
    )?;
    Ok(())
}

fn load_network_grant(
    connection: &Connection,
    grant_id: NetworkGrantId,
) -> Result<Option<NetworkGrant>, PersistenceError> {
    connection
        .query_row(
            "SELECT grant_json FROM network_grants WHERE network_grant_id=?1",
            [grant_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|value| serde_json::from_str(&value).map_err(PersistenceError::from))
        .transpose()
}

fn insert_network_audit(
    transaction: &Transaction<'_>,
    record: &NetworkRequestAuditRecord,
) -> Result<(), PersistenceError> {
    transaction.execute(
        "INSERT INTO network_request_audit(
            network_request_id, identity_json, caller, inspection_level, method, normalized_origin,
            destination_classes_json, data_classes_json, upload_bytes, download_bytes,
            state, error_code, started_at, finished_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            record.request_id.to_string(),
            serde_json::to_string(&record.identity)?,
            network_caller_name(record.caller),
            network_inspection_level_name(record.inspection_level),
            record.method,
            record.origin.as_ref().map(serde_json::to_string).transpose()?,
            serde_json::to_string(&record.destination_classes)?,
            serde_json::to_string(&record.data_classes)?,
            to_i64(record.upload_bytes, "network upload bytes")?,
            to_i64(record.download_bytes, "network download bytes")?,
            network_audit_state_name(record.state),
            record.error_code,
            record.started_at.to_string(),
            record.finished_at.map(|value| value.to_string()),
        ],
    )?;
    Ok(())
}

fn load_network_audit(
    connection: &Connection,
    request_id: NetworkRequestId,
) -> Result<Option<NetworkRequestAuditRecord>, PersistenceError> {
    connection
        .query_row(
            "SELECT identity_json, caller, inspection_level, method, normalized_origin,
                    destination_classes_json, data_classes_json, upload_bytes,
                    download_bytes, state, error_code, started_at, finished_at
             FROM network_request_audit WHERE network_request_id=?1",
            [request_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, Option<String>>(12)?,
                ))
            },
        )
        .optional()?
        .map(|row| {
            Ok(NetworkRequestAuditRecord {
                request_id,
                identity: serde_json::from_str(&row.0)?,
                caller: parse_network_caller(&row.1)?,
                network_enforcement: NetworkEnforcementKind::CoreControlPlane,
                inspection_level: parse_network_inspection_level(&row.2)?,
                method: row.3,
                origin: row.4.as_deref().map(serde_json::from_str).transpose()?,
                destination_classes: serde_json::from_str(&row.5)?,
                data_classes: serde_json::from_str(&row.6)?,
                upload_bytes: from_i64(row.7, "network upload bytes")?,
                download_bytes: from_i64(row.8, "network download bytes")?,
                state: parse_network_audit_state(&row.9)?,
                error_code: row.10,
                started_at: parse_timestamp(&row.11)?,
                finished_at: row.12.as_deref().map(parse_timestamp).transpose()?,
            })
        })
        .transpose()
}

fn update_network_audit(
    transaction: &Transaction<'_>,
    record: &NetworkRequestAuditRecord,
) -> Result<(), PersistenceError> {
    let changed = transaction.execute(
        "UPDATE network_request_audit
         SET download_bytes=?1, state=?2, error_code=?3, finished_at=?4
         WHERE network_request_id=?5",
        params![
            to_i64(record.download_bytes, "network download bytes")?,
            network_audit_state_name(record.state),
            record.error_code,
            record.finished_at.map(|value| value.to_string()),
            record.request_id.to_string(),
        ],
    )?;
    if changed != 1 {
        return Err(PersistenceError::NetworkAuditNotFound);
    }
    Ok(())
}

fn load_quarantine(
    connection: &Connection,
    quarantine_id: QuarantineRecordId,
) -> Result<Option<QuarantineRecordDetails>, PersistenceError> {
    connection
        .query_row(
            "SELECT record_json FROM quarantine_records WHERE quarantine_record_id=?1",
            [quarantine_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|value| serde_json::from_str(&value).map_err(PersistenceError::from))
        .transpose()
}

fn update_approval(
    transaction: &Transaction<'_>,
    request: &ApprovalRequest,
) -> Result<(), PersistenceError> {
    let changed = transaction.execute(
        "UPDATE approval_requests
         SET request_json=?1, request_revision=?2, state=?3,
             resolved_at=?4, invalidation_reason=?5
         WHERE approval_request_id=?6",
        params![
            serde_json::to_string(request)?,
            to_i64(request.request_revision, "approval revision")?,
            approval_state_name(request.state),
            request.resolved_at.map(|value| value.to_string()),
            request.invalidation_reason.as_deref(),
            request.id.to_string(),
        ],
    )?;
    if changed != 1 {
        return Err(PersistenceError::ApprovalNotFound);
    }
    Ok(())
}

fn invalidate_pending_for_session(
    transaction: &Transaction<'_>,
    session_id: SessionId,
    reason: &str,
    now: UtcTimestamp,
) -> Result<u64, PersistenceError> {
    let mut statement = transaction.prepare(
        "SELECT request_json FROM approval_requests
         WHERE state='pending'
           AND json_extract(identity_json, '$.session_id')=?1",
    )?;
    let encoded = statement
        .query_map([session_id.to_string()], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    let mut count = 0_u64;
    for value in encoded {
        let mut request: ApprovalRequest = serde_json::from_str(&value)?;
        request.state = ApprovalState::Invalidated;
        request.request_revision = request.request_revision.checked_add(1).ok_or_else(|| {
            PersistenceError::InvalidRequest("approval revision exhausted".to_owned())
        })?;
        request.resolved_at = Some(now);
        request.invalidation_reason = Some(reason.to_owned());
        update_approval(transaction, &request)?;
        count = count.saturating_add(1);
    }
    Ok(count)
}

fn grants_for_approval(
    request: &ApprovalRequest,
    lifetime: PermissionGrantLifetime,
    workspace_id: WorkspaceId,
    now: UtcTimestamp,
) -> Result<Vec<PermissionGrant>, PersistenceError> {
    let (session_id, turn_task_id) = match lifetime {
        PermissionGrantLifetime::Once => {
            (request.identity.session_id, request.identity.turn_task_id)
        }
        PermissionGrantLifetime::TurnTask => (
            Some(request.identity.session_id.ok_or_else(|| {
                PersistenceError::InvalidRequest("turn grant requires a session".to_owned())
            })?),
            Some(request.identity.turn_task_id.ok_or_else(|| {
                PersistenceError::InvalidRequest("turn grant requires a turn task".to_owned())
            })?),
        ),
        PermissionGrantLifetime::Session => (
            Some(request.identity.session_id.ok_or_else(|| {
                PersistenceError::InvalidRequest("session grant requires a session".to_owned())
            })?),
            None,
        ),
        PermissionGrantLifetime::Workspace => (None, None),
    };
    let fingerprints: Vec<Option<_>> = if lifetime == PermissionGrantLifetime::Once {
        request.operation_fingerprints.iter().cloned().map(Some).collect()
    } else {
        vec![None]
    };
    Ok(fingerprints
        .into_iter()
        .map(|operation_fingerprint| PermissionGrant {
            id: PermissionGrantId::new(),
            lifetime,
            workspace_id,
            session_id,
            turn_task_id,
            source: request.source.clone(),
            source_fingerprint: request.source_fingerprint.clone(),
            capabilities: request.capabilities.clone(),
            scopes: request.scopes.clone(),
            approved_risk_flags: request.risk_flags.clone(),
            operation_fingerprint,
            created_at: now,
            expires_at: None,
            revoked_at: None,
            consumed_at: None,
        })
        .collect())
}

fn insert_grant(
    transaction: &Transaction<'_>,
    grant: &PermissionGrant,
    approval_decision_id: Option<ApprovalDecisionId>,
) -> Result<(), PersistenceError> {
    transaction.execute(
        "INSERT INTO permission_grants(
            permission_grant_id, grant_json, lifetime, workspace_id, session_id,
            turn_task_id, source_json, source_fingerprint_json, capabilities_json,
            scopes_json, approved_risk_flags_json, operation_fingerprint_json,
            created_at, expires_at, revoked_at, consumed_at, approval_decision_id
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
            ?11, ?12, ?13, ?14, ?15, ?16, ?17
         )",
        params![
            grant.id.to_string(),
            serde_json::to_string(grant)?,
            grant_lifetime_name(grant.lifetime),
            grant.workspace_id.to_string(),
            grant.session_id.map(|id| id.to_string()),
            grant.turn_task_id.map(|id| id.to_string()),
            serde_json::to_string(&grant.source)?,
            grant.source_fingerprint.as_ref().map(serde_json::to_string).transpose()?,
            serde_json::to_string(&grant.capabilities)?,
            serde_json::to_string(&grant.scopes)?,
            serde_json::to_string(&grant.approved_risk_flags)?,
            grant.operation_fingerprint.as_ref().map(serde_json::to_string).transpose()?,
            grant.created_at.to_string(),
            grant.expires_at.map(|value| value.to_string()),
            grant.revoked_at.map(|value| value.to_string()),
            grant.consumed_at.map(|value| value.to_string()),
            approval_decision_id.map(|id| id.to_string()),
        ],
    )?;
    Ok(())
}

fn load_grant(
    connection: &Connection,
    grant_id: PermissionGrantId,
) -> Result<Option<PermissionGrant>, PersistenceError> {
    connection
        .query_row(
            "SELECT grant_json FROM permission_grants WHERE permission_grant_id=?1",
            [grant_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|value| serde_json::from_str(&value).map_err(PersistenceError::from))
        .transpose()
}

fn update_grant(
    transaction: &Transaction<'_>,
    grant: &PermissionGrant,
) -> Result<(), PersistenceError> {
    let changed = transaction.execute(
        "UPDATE permission_grants
         SET grant_json=?1, expires_at=?2, revoked_at=?3, consumed_at=?4
         WHERE permission_grant_id=?5",
        params![
            serde_json::to_string(grant)?,
            grant.expires_at.map(|value| value.to_string()),
            grant.revoked_at.map(|value| value.to_string()),
            grant.consumed_at.map(|value| value.to_string()),
            grant.id.to_string(),
        ],
    )?;
    if changed != 1 {
        return Err(PersistenceError::PermissionGrantNotFound);
    }
    Ok(())
}

fn insert_turn_deny(
    transaction: &Transaction<'_>,
    record: &TurnDenyRecord,
) -> Result<(), PersistenceError> {
    transaction.execute(
        "INSERT INTO turn_deny_records(
            turn_task_id, source_key, source_json, operation_fingerprint_json, denied_at
         ) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(turn_task_id, source_key, operation_fingerprint_json)
         DO UPDATE SET denied_at=excluded.denied_at",
        params![
            record.turn_task_id.to_string(),
            record.source.stable_key(),
            serde_json::to_string(&record.source)?,
            serde_json::to_string(&record.operation_fingerprint)?,
            record.denied_at.to_string(),
        ],
    )?;
    Ok(())
}

fn commit_session_permission_projection(
    transaction: &Transaction<'_>,
    payload: SessionPermissionProjection,
) -> Result<CommittedProjection<SecurityProjectionPayload>, PersistenceError> {
    let key = payload.session_id.to_string();
    commit_security_projection(
        transaction,
        "security_session_permission",
        &key,
        SecurityProjectionPayload::SessionPermission(payload),
    )
}

fn commit_pending_approval_projection(
    transaction: &Transaction<'_>,
) -> Result<CommittedProjection<SecurityProjectionPayload>, PersistenceError> {
    let source_revision =
        next_projection_revision(transaction, "security_pending_approval", "global")?;
    let payload = SecurityProjectionPayload::PendingApproval(PendingApprovalProjection {
        approvals: load_pending_approvals(transaction)?.iter().map(ApprovalView::from).collect(),
        revision: source_revision,
    });
    insert_security_projection_outbox(
        transaction,
        "security_pending_approval",
        "global",
        source_revision,
        payload,
    )
}

fn commit_security_projection(
    transaction: &Transaction<'_>,
    kind: &str,
    key: &str,
    payload: SecurityProjectionPayload,
) -> Result<CommittedProjection<SecurityProjectionPayload>, PersistenceError> {
    let source_revision = next_projection_revision(transaction, kind, key)?;
    insert_security_projection_outbox(transaction, kind, key, source_revision, payload)
}

fn insert_security_projection_outbox(
    transaction: &Transaction<'_>,
    kind: &str,
    key: &str,
    source_revision: u64,
    payload: SecurityProjectionPayload,
) -> Result<CommittedProjection<SecurityProjectionPayload>, PersistenceError> {
    transaction.execute(
        "INSERT INTO projection_outbox(
            projection_kind, projection_key, revision, payload_json, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            kind,
            key,
            to_i64(source_revision, "security projection revision")?,
            serde_json::to_string(&payload)?,
            UtcTimestamp::now().to_string(),
        ],
    )?;
    Ok(CommittedProjection { outbox_id: transaction.last_insert_rowid(), source_revision, payload })
}

fn load_host_preparation_state(
    connection: &Connection,
) -> Result<Option<HostPreparationStoredState>, PersistenceError> {
    let stored = connection
        .query_row(
            "SELECT summary_json, ownership_json, revision
             FROM host_preparation_states WHERE state_key='global'",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?)),
        )
        .optional()?;
    let Some((summary_json, ownership_json, revision)) = stored else {
        return Ok(None);
    };
    Ok(Some(HostPreparationStoredState {
        status: HostPreparationStatus {
            summary: serde_json::from_str(&summary_json)?,
            revision: from_i64(revision, "host preparation revision")?,
            active_operation: load_active_host_preparation_operation(connection)?,
        },
        ownership: serde_json::from_str(&ownership_json)?,
    }))
}

fn upsert_host_preparation_state(
    transaction: &Transaction<'_>,
    status: &HostPreparationStatus,
    ownership: &HostPreparationOwnership,
) -> Result<(), PersistenceError> {
    transaction.execute(
        "INSERT INTO host_preparation_states(
            state_key, summary_json, ownership_json, revision, updated_at
         ) VALUES ('global', ?1, ?2, ?3, ?4)
         ON CONFLICT(state_key) DO UPDATE SET
            summary_json=excluded.summary_json,
            ownership_json=excluded.ownership_json,
            revision=excluded.revision,
            updated_at=excluded.updated_at",
        params![
            serde_json::to_string(&status.summary)?,
            serde_json::to_string(ownership)?,
            to_i64(status.revision, "host preparation revision")?,
            UtcTimestamp::now().to_string(),
        ],
    )?;
    Ok(())
}

fn load_active_host_preparation_operation(
    connection: &Connection,
) -> Result<Option<HostPreparationOperationView>, PersistenceError> {
    let operation = connection
        .query_row(
            "SELECT operation_json
             FROM host_preparation_journal
             WHERE state NOT IN ('completed', 'rolled_back', 'denied', 'failed')
             ORDER BY created_at, host_preparation_operation_id
             LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    operation.map(|value| serde_json::from_str(&value).map_err(PersistenceError::from)).transpose()
}

fn load_host_preparation_journal(
    connection: &Connection,
    operation_id: HostPreparationOperationId,
) -> Result<HostPreparationJournalRecord, PersistenceError> {
    let stored = connection
        .query_row(
            "SELECT operation_json, broker_plan_json, approval_json
             FROM host_preparation_journal
             WHERE host_preparation_operation_id=?1",
            [operation_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::HostPreparationOperationNotFound)?;
    Ok(HostPreparationJournalRecord {
        operation: serde_json::from_str(&stored.0)?,
        broker_plan: serde_json::from_str(&stored.1)?,
        approval: stored.2.map(|value| serde_json::from_str(&value)).transpose()?,
    })
}

const fn host_preparation_action_name(action: HostPreparationAction) -> &'static str {
    match action {
        HostPreparationAction::Prepare => "prepare",
        HostPreparationAction::Repair => "repair",
        HostPreparationAction::Remove => "remove",
    }
}

const fn host_preparation_operation_state_name(
    state: HostPreparationOperationState,
) -> &'static str {
    match state {
        HostPreparationOperationState::Prepared => "prepared",
        HostPreparationOperationState::AwaitingElevation => "awaiting_elevation",
        HostPreparationOperationState::Applying => "applying",
        HostPreparationOperationState::Completed => "completed",
        HostPreparationOperationState::RollbackPending => "rollback_pending",
        HostPreparationOperationState::RolledBack => "rolled_back",
        HostPreparationOperationState::Denied => "denied",
        HostPreparationOperationState::Failed => "failed",
    }
}

const fn host_preparation_state_is_terminal(state: HostPreparationOperationState) -> bool {
    matches!(
        state,
        HostPreparationOperationState::Completed
            | HostPreparationOperationState::RolledBack
            | HostPreparationOperationState::Denied
            | HostPreparationOperationState::Failed
    )
}

const fn valid_host_preparation_transition(
    from: HostPreparationOperationState,
    to: HostPreparationOperationState,
) -> bool {
    matches!(
        (from, to),
        (
            HostPreparationOperationState::Prepared,
            HostPreparationOperationState::AwaitingElevation
                | HostPreparationOperationState::Applying
                | HostPreparationOperationState::Denied
                | HostPreparationOperationState::Failed
        ) | (
            HostPreparationOperationState::AwaitingElevation,
            HostPreparationOperationState::Applying | HostPreparationOperationState::Failed
        ) | (
            HostPreparationOperationState::Applying,
            HostPreparationOperationState::Completed
                | HostPreparationOperationState::RollbackPending
                | HostPreparationOperationState::Failed
        ) | (
            HostPreparationOperationState::RollbackPending,
            HostPreparationOperationState::Completed
                | HostPreparationOperationState::RolledBack
                | HostPreparationOperationState::Failed
        )
    )
}

const fn permission_mode_name(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::ReadOnly => "read_only",
        PermissionMode::WorkspaceReadWrite => "workspace_read_write",
        PermissionMode::FullAccess => "full_access",
    }
}

fn parse_permission_mode(value: &str) -> Result<PermissionMode, PersistenceError> {
    match value {
        "read_only" => Ok(PermissionMode::ReadOnly),
        "workspace_read_write" => Ok(PermissionMode::WorkspaceReadWrite),
        "full_access" => Ok(PermissionMode::FullAccess),
        _ => Err(PersistenceError::InvalidRequest(format!(
            "unknown persisted permission mode: {value}"
        ))),
    }
}

const fn grant_lifetime_name(lifetime: PermissionGrantLifetime) -> &'static str {
    match lifetime {
        PermissionGrantLifetime::Once => "once",
        PermissionGrantLifetime::TurnTask => "turn_task",
        PermissionGrantLifetime::Session => "session",
        PermissionGrantLifetime::Workspace => "workspace",
    }
}

const fn approval_state_name(state: ApprovalState) -> &'static str {
    match state {
        ApprovalState::Pending => "pending",
        ApprovalState::Approved => "approved",
        ApprovalState::Denied => "denied",
        ApprovalState::Expired => "expired",
        ApprovalState::Cancelled => "cancelled",
        ApprovalState::Invalidated => "invalidated",
    }
}

const fn approval_decision_name(decision: ApprovalDecisionKind) -> &'static str {
    match decision {
        ApprovalDecisionKind::AllowOnce => "allow_once",
        ApprovalDecisionKind::AllowSession => "allow_session",
        ApprovalDecisionKind::AllowWorkspace => "allow_workspace",
        ApprovalDecisionKind::AllowForCurrentTurn => "allow_for_current_turn",
        ApprovalDecisionKind::Deny => "deny",
    }
}

const fn sandbox_mode_name(mode: aura_permission_domain::NativeSandboxMode) -> &'static str {
    match mode {
        aura_permission_domain::NativeSandboxMode::CompatibilityAppContainer => {
            "compatibility_app_container"
        }
        aura_permission_domain::NativeSandboxMode::StrictAppContainer => "strict_app_container",
        aura_permission_domain::NativeSandboxMode::ObservedNative => "observed_native",
        aura_permission_domain::NativeSandboxMode::ObservedNativeNetworked => {
            "observed_native_networked"
        }
        aura_permission_domain::NativeSandboxMode::ElevatedBrokered => "elevated_brokered",
    }
}

const fn sandbox_state_name(state: SandboxExecutionState) -> &'static str {
    match state {
        SandboxExecutionState::Planned => "planned",
        SandboxExecutionState::Starting => "starting",
        SandboxExecutionState::Running => "running",
        SandboxExecutionState::Exited => "exited",
        SandboxExecutionState::Cancelled => "cancelled",
        SandboxExecutionState::TimedOut => "timed_out",
        SandboxExecutionState::PolicyViolation => "policy_violation",
        SandboxExecutionState::CleanupFailed => "cleanup_failed",
        SandboxExecutionState::StateUnknown => "state_unknown",
    }
}

fn parse_sandbox_state(value: &str) -> Result<SandboxExecutionState, PersistenceError> {
    match value {
        "planned" => Ok(SandboxExecutionState::Planned),
        "starting" => Ok(SandboxExecutionState::Starting),
        "running" => Ok(SandboxExecutionState::Running),
        "exited" => Ok(SandboxExecutionState::Exited),
        "cancelled" => Ok(SandboxExecutionState::Cancelled),
        "timed_out" => Ok(SandboxExecutionState::TimedOut),
        "policy_violation" => Ok(SandboxExecutionState::PolicyViolation),
        "cleanup_failed" => Ok(SandboxExecutionState::CleanupFailed),
        "state_unknown" => Ok(SandboxExecutionState::StateUnknown),
        _ => Err(PersistenceError::InvalidRequest(format!(
            "unknown persisted sandbox state: {value}"
        ))),
    }
}

const fn sandbox_state_is_terminal(state: SandboxExecutionState) -> bool {
    matches!(
        state,
        SandboxExecutionState::Exited
            | SandboxExecutionState::Cancelled
            | SandboxExecutionState::TimedOut
            | SandboxExecutionState::PolicyViolation
            | SandboxExecutionState::CleanupFailed
            | SandboxExecutionState::StateUnknown
    )
}

const fn sandbox_access_mask(access: SandboxFilesystemAccess) -> i64 {
    match access {
        SandboxFilesystemAccess::Read => 1,
        SandboxFilesystemAccess::ReadWrite => 2,
        SandboxFilesystemAccess::Create => 3,
    }
}

fn parse_sandbox_access(value: i64) -> Result<SandboxFilesystemAccess, PersistenceError> {
    match value {
        1 => Ok(SandboxFilesystemAccess::Read),
        2 => Ok(SandboxFilesystemAccess::ReadWrite),
        3 => Ok(SandboxFilesystemAccess::Create),
        _ => Err(PersistenceError::InvalidRequest(format!(
            "unknown persisted sandbox access mask: {value}"
        ))),
    }
}

const fn acl_journal_state_name(state: SandboxAclJournalState) -> &'static str {
    match state {
        SandboxAclJournalState::Prepared => "prepared",
        SandboxAclJournalState::Applied => "applied",
        SandboxAclJournalState::CleanupPending => "cleanup_pending",
        SandboxAclJournalState::Cleaned => "cleaned",
        SandboxAclJournalState::CleanupFailed => "cleanup_failed",
    }
}

fn parse_acl_journal_state(value: &str) -> Result<SandboxAclJournalState, PersistenceError> {
    match value {
        "prepared" => Ok(SandboxAclJournalState::Prepared),
        "applied" => Ok(SandboxAclJournalState::Applied),
        "cleanup_pending" => Ok(SandboxAclJournalState::CleanupPending),
        "cleaned" => Ok(SandboxAclJournalState::Cleaned),
        "cleanup_failed" => Ok(SandboxAclJournalState::CleanupFailed),
        _ => Err(PersistenceError::InvalidRequest(format!(
            "unknown persisted ACL journal state: {value}"
        ))),
    }
}

const fn network_identity_scope_name(
    scope: aura_permission_domain::NetworkGrantIdentityScope,
) -> &'static str {
    match scope {
        aura_permission_domain::NetworkGrantIdentityScope::Session => "session",
        aura_permission_domain::NetworkGrantIdentityScope::TurnTask => "turn_task",
        aura_permission_domain::NetworkGrantIdentityScope::SandboxExecution => "sandbox_execution",
        aura_permission_domain::NetworkGrantIdentityScope::SystemComponent => "system_component",
    }
}

const fn network_caller_name(caller: aura_security_contract::NetworkCallerKind) -> &'static str {
    match caller {
        aura_security_contract::NetworkCallerKind::ModelTransport => "model_transport",
        aura_security_contract::NetworkCallerKind::ModelsCatalog => "models_catalog",
        aura_security_contract::NetworkCallerKind::AuraTool => "aura_tool",
        aura_security_contract::NetworkCallerKind::WebRuntime => "web_runtime",
        aura_security_contract::NetworkCallerKind::AuraBrowser => "aura_browser",
        aura_security_contract::NetworkCallerKind::SandboxProcess => "sandbox_process",
        aura_security_contract::NetworkCallerKind::McpHttp => "mcp_http",
        aura_security_contract::NetworkCallerKind::ExternalExtensionProtocol => {
            "external_extension_protocol"
        }
        aura_security_contract::NetworkCallerKind::UpdateService => "update_service",
    }
}

fn parse_network_caller(
    value: &str,
) -> Result<aura_security_contract::NetworkCallerKind, PersistenceError> {
    match value {
        "model_transport" => Ok(aura_security_contract::NetworkCallerKind::ModelTransport),
        "models_catalog" => Ok(aura_security_contract::NetworkCallerKind::ModelsCatalog),
        "aura_tool" => Ok(aura_security_contract::NetworkCallerKind::AuraTool),
        "web_runtime" => Ok(aura_security_contract::NetworkCallerKind::WebRuntime),
        "aura_browser" => Ok(aura_security_contract::NetworkCallerKind::AuraBrowser),
        "sandbox_process" => Ok(aura_security_contract::NetworkCallerKind::SandboxProcess),
        "mcp_http" => Ok(aura_security_contract::NetworkCallerKind::McpHttp),
        "external_extension_protocol" => {
            Ok(aura_security_contract::NetworkCallerKind::ExternalExtensionProtocol)
        }
        "update_service" => Ok(aura_security_contract::NetworkCallerKind::UpdateService),
        _ => Err(PersistenceError::InvalidRequest(format!(
            "unknown persisted network caller: {value}"
        ))),
    }
}

const fn network_audit_state_name(state: NetworkRequestAuditState) -> &'static str {
    match state {
        NetworkRequestAuditState::Started => "started",
        NetworkRequestAuditState::Authorized => "authorized",
        NetworkRequestAuditState::Blocked => "blocked",
        NetworkRequestAuditState::Finished => "finished",
        NetworkRequestAuditState::Interrupted => "interrupted",
    }
}

fn parse_network_audit_state(value: &str) -> Result<NetworkRequestAuditState, PersistenceError> {
    match value {
        "started" => Ok(NetworkRequestAuditState::Started),
        "authorized" => Ok(NetworkRequestAuditState::Authorized),
        "blocked" => Ok(NetworkRequestAuditState::Blocked),
        "finished" => Ok(NetworkRequestAuditState::Finished),
        "interrupted" => Ok(NetworkRequestAuditState::Interrupted),
        _ => Err(PersistenceError::InvalidRequest(format!(
            "unknown persisted network audit state: {value}"
        ))),
    }
}

const fn network_inspection_level_name(level: NetworkInspectionLevel) -> &'static str {
    match level {
        NetworkInspectionLevel::StructuredHttp => "structured_http",
        NetworkInspectionLevel::OpaqueTunnel => "opaque_tunnel",
        NetworkInspectionLevel::ObservedNativeOnly => "observed_native_only",
    }
}

fn parse_network_inspection_level(value: &str) -> Result<NetworkInspectionLevel, PersistenceError> {
    match value {
        "structured_http" => Ok(NetworkInspectionLevel::StructuredHttp),
        "opaque_tunnel" => Ok(NetworkInspectionLevel::OpaqueTunnel),
        "observed_native_only" => Ok(NetworkInspectionLevel::ObservedNativeOnly),
        _ => Err(PersistenceError::InvalidRequest(format!(
            "unknown persisted network inspection level: {value}"
        ))),
    }
}

const fn quarantine_state_name(state: QuarantineState) -> &'static str {
    match state {
        QuarantineState::Quarantined => "quarantined",
        QuarantineState::Released => "released",
        QuarantineState::Discarded => "discarded",
    }
}

fn sanitized_source_url(value: &str) -> String {
    value.split(['?', '#']).next().unwrap_or_default().to_owned()
}

fn parse_timestamp(value: &str) -> Result<UtcTimestamp, PersistenceError> {
    serde_json::from_value(serde_json::Value::String(value.to_owned()))
        .map_err(PersistenceError::from)
}

fn to_i64(value: u64, label: &str) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| {
        PersistenceError::InvalidRequest(format!("{label} exceeds SQLite integer range"))
    })
}

fn from_i64(value: i64, label: &str) -> Result<u64, PersistenceError> {
    u64::try_from(value).map_err(|_| PersistenceError::InvalidRequest(format!("negative {label}")))
}
