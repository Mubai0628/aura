use crate::workspace_policy;
mod approval;
mod host_begin;
use host_begin::issue_elevation_launch_ticket;

mod host_recovery;
use host_recovery::{
    host_preparation_execution_error_code, host_preparation_failure_category,
    host_preparation_platform_error, host_preparation_release_error, record_host_preparation_stage,
};

mod host_resolution;
mod host_status;
mod main_authority;
pub(crate) use main_authority::resolve_workspace_mutation;
mod readiness_projection;
use readiness_projection::build_runtime_projection;
pub(crate) use readiness_projection::build_runtime_projection_at_start;
mod security_config;
pub use security_config::P04SecurityConfigValidator;
use security_config::{SecurityConfig, parse_default_permission_mode};

mod main_tool_mutation;
use main_tool_mutation::authenticated_tool_mutation_identity_hash;
mod file_consents;
mod mode_transitions;
mod network_consents;
mod session_consents;
use std::{
    collections::{BTreeMap, HashMap},
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::{Arc, Mutex},
};
mod artifact_evidence;
mod file_resources;
mod permission_defaults;
mod publication_dispatch;
use permission_defaults::permission_defaults_from_settings;
#[cfg(test)]
mod publication_recovery_tests;

use anyhow::{Context, Result, anyhow, ensure};
use aura_artifact_contract::{ArtifactAccess, ArtifactExportResult, ArtifactRecord};
use aura_config::{
    ConfigModule, ConfigValidationError, ModuleConfigDocument, ModuleConfigValidator,
    SettingsFacade,
};
use aura_domain_common::{
    ApprovalRequestId, CommandInvocationId, ContentHash, ContentHashAlgorithm, CorrelationId,
    ElevationLaunchTicketId, ErrorCategory, ErrorCode, HostPreparationOperationId,
    QuarantineRecordId, SecretNonce, SecurityOperationId, SessionId, UtcTimestamp, WorkspaceId,
    WorkspacePath,
};
use aura_fs_tools::{FsToolError, FsToolService};
use aura_ipc::method;
use aura_network_control::{
    AuthenticatedConfigurationChannelIdentity, ExecutableDetector, NetworkControlError,
    QuarantineStore,
};
use aura_permission_domain::{
    ApprovalRequestDraft, ApprovalView, AttachExternalReadOnlyInput,
    DeriveWorkspaceExternalGrantInput, DiscardQuarantineInput, ElevationLaunchTicket,
    ExternalGrantCapability, GetApprovalDetailsInput, GetSessionPermissionStateInput,
    HostPreparationAction, HostPreparationBeginResult, HostPreparationBrokerPlan,
    HostPreparationMutationInput, HostPreparationOperationState, HostPreparationOperationView,
    HostPreparationOwnership, HostPreparationStatus, IssueQuarantineDiscardConfirmationInput,
    ListPermissionGrantsInput, ListSessionExternalGrantsInput, PrepareQuarantineReleaseInput,
    PrepareQuarantineReleaseResult, QuarantineDiscardConfirmation, QuarantineReleaseBinding,
    QuarantineReleaseTarget, QuarantineState, ReleaseQuarantineInput, ResolveApprovalInput,
    ResolveHostPreparationInput, ResolvedScope, RevalidateExternalGrantInput,
    RevokeExternalGrantInput, RevokePermissionGrantInput, RuntimeComponentState,
    SecurityRuntimeProjection, SecuritySource,
};
use aura_persistence::{
    ARTIFACT_RANGE_LIMIT, ArtifactMetadataRequest, ArtifactOverwriteJournalBinding,
    ArtifactOverwriteJournalState, ArtifactReadRequest, HostPreparationOperationTransition,
    PersistenceCoordinator, PersistenceError,
};
use aura_runtime_contract::{IpcErrorEnvelope, IpcRequestTerminal};
use aura_sandbox::{
    CancellationRegistry, SandboxExecutor, SandboxExecutorOptions,
    execute_approved_host_preparation_broker_plan, host_preparation_plan_hash,
    validate_host_preparation_plan_nonce,
};
use aura_security_contract::{
    ApprovalDecisionKind, CanonicalPath, InvocationIdentity, OperationRiskFlag, PermissionMode,
    ResolvedCapability,
};
use aura_windows_platform::{
    ContentHashAlgorithm as PlatformHashAlgorithm, ElevatedHelperLaunchError,
    HandleBoundPublishedFile, HandleBoundReplaceContent, NativeFileAccess, PublishLocation,
    capture_null_device_security_descriptor, current_process_is_elevated, final_path_within,
    handle_bound_create_new, handle_bound_replace_with_backup_checked,
    installation_identity::{NullDeviceVerdict, probe_null_device_readiness},
    null_device_security_descriptor_matches, open_verified_publish_parent,
    probe_host_compatibility, probe_windows_sandbox,
};
use aura_workspace::{WorkspaceError, WorkspaceRuntime, domain_identity};
use aura_workspace_domain::{
    ArtifactExportRequest, ArtifactExportTarget, BufferSaveCommand, FsApplyChangesetRequest,
    FsApplyPatchRequest, FsChangesetOperation, FsCopyRequest, FsCreateDirectoryRequest,
    FsDeleteRequest, FsExpectedState, FsMoveRequest, FsMutationDisposition, FsWriteFileRequest,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::{
    external_grants::{ExternalGrantError, ExternalGrantService},
    p04_broker::{
        ActiveReleaseTrustError, ElevatedBrokerOptions, execute_elevated_host_preparation,
        record_host_preparation_write_evidence, validate_active_release_identity,
    },
};

use aura_tool_domain::TOOL_MUTATION_METHODS_V1;

/// The external destination's live-short-commit code, re-exported so the one dispatcher predicate
/// that decides "may this refusal be waited on" can name it without reaching into the module.
pub(crate) const EXTERNAL_RESOURCE_BUSY_CODE: &str = file_resources::EXTERNAL_RESOURCE_BUSY_CODE;

#[derive(Clone)]
pub struct P04Services {
    persistence: Arc<PersistenceCoordinator>,
    quarantine: Arc<QuarantineStore>,
    external_grants: Arc<ExternalGrantService>,
    fs: Arc<FsToolService>,
    workspace: Arc<WorkspaceRuntime>,
    discard_confirmations: Arc<Mutex<HashMap<QuarantineRecordId, PendingDiscardConfirmation>>>,
    host_preparation_nonces: Arc<Mutex<HashMap<HostPreparationOperationId, SecretNonce>>>,
    /// Owns the host probes and expiry settlements, so the status route itself never
    /// runs a host API or queues a write.
    host_preparation: Arc<crate::host_preparation::HostPreparationWorker>,
    broker: ElevatedBrokerOptions,
    default_permission_mode: PermissionMode,
    settings: Arc<SettingsFacade>,
    /// This Core process. Used by the Native session consent projection, which has
    /// to report a consent from an earlier Core run as no longer in force.
    runtime_instance_id: aura_domain_common::RuntimeInstanceId,
    publication_workers: crate::blocking_pool::BlockingPool,
    /// The run-information index, plus the observer a dropped fact is reported to.
    ///
    /// Is a real publication owner, so it publishes its own activity from its own commit
    /// point. It does not read the index, and nothing in the publication path consults it: a
    /// broken or full index cannot affect a publication.
    activity_index: aura_activity_runtime::ActivityIndex,
    activity_observer: aura_activity_runtime::ActivityObserverHandle,
}

impl P04Services {
    /// The index P04 publishes its real publication activity into.
    pub(crate) fn activity_index(&self) -> &aura_activity_runtime::ActivityIndex {
        &self.activity_index
    }

    /// The observer a dropped publication fact is reported to.
    pub(crate) fn activity_observer(&self) -> &aura_activity_runtime::ActivityObserverHandle {
        &self.activity_observer
    }

    /// The runtime instance this publication belongs to.
    pub(crate) const fn activity_runtime_instance_id(
        &self,
    ) -> aura_domain_common::RuntimeInstanceId {
        self.runtime_instance_id
    }
}

struct PendingDiscardConfirmation {
    token_hash: [u8; 32],
    content_hash: ContentHash,
    expires_at: UtcTimestamp,
}

struct CurrentHostPreparationOperation {
    operation: HostPreparationOperationView,
    status_revision: u64,
}

#[derive(Debug)]
enum HostPreparationRecoveryDisposition {
    Applied(HostPreparationOwnership),
    PreState,
    Unknown,
}

const HOST_PREPARATION_PLAN_TTL: time::Duration = time::Duration::minutes(3);
const ELEVATION_LAUNCH_TICKET_TTL: time::Duration = time::Duration::seconds(60);

impl P04Services {
    /// The one eligibility owner, read from the workspace runtime this service already holds.
    ///
    /// The handle is the **same instance** Main Fs, the Agent file tools and the Buffer coordinate
    /// through: `Arc::ptr_eq(runtime.eligibility_owner(), runtime.buffers().eligibility_owner())` is
    /// the same-instance property, and this is why it is a delegation rather than a field. A service
    /// with its own owner would protect nothing while looking exactly like this one.
    ///
    /// It exists because the previous revision of this batch claimed P04 shared the common owner
    /// while `aura-core` contained **zero** references to it: the platform's checked publisher did
    /// the final conditional commit, and a final conditional commit is not cross-entry admission.
    /// The three call sites in [`P04Services::export_artifact_to_external_grant`] and the quarantine
    /// release now really take a commit before they publish.
    fn eligibility_owner(&self) -> Arc<aura_workspace_domain::EligibilityOwner> {
        self.workspace.eligibility_owner()
    }

    /// Takes the common admission for one **external** destination, derived from the handle the
    /// grant already verified.
    ///
    /// The resource is an object/slot pair for the destination name, built from the grant's verified
    /// parent identity through [`aura_workspace::ResourceFacts::object_from_verified`] and
    /// `slot`-equivalent wording — never from path text, and never from a second identity derivation.
    /// An external destination is deliberately represented with the **parent identity the grant
    /// validated plus the leaf**, exactly the shape an in-workspace entry has, so an external write
    /// and an in-workspace write to one physical object compare as one resource.
    ///
    /// Returns the live commit, which the caller must release after its real publication — or after
    /// its recovery has taken the publication over. It is never released early: a caller that leaves
    /// while a publication is still landing must not hand the resource to somebody else.
    fn admit_external_destination(
        &self,
        parent: &aura_windows_platform::VerifiedFile,
        leaf_name: &str,
        target: Option<aura_windows_platform::NativeFileIdentity>,
        workspace_id: WorkspaceId,
        session_id: SessionId,
        correlation_id: CorrelationId,
    ) -> Result<file_resources::ExternalAdmission, IpcErrorEnvelope> {
        let path = WorkspacePath::parse(leaf_name).map_err(|error| {
            envelope(
                "artifact.invalid_target",
                ErrorCategory::Validation,
                error.to_string(),
                false,
                correlation_id,
            )
        })?;
        let label = aura_workspace_domain::ResourceOwner { workspace_id, session_id, path };
        file_resources::ExternalAdmission::acquire(
            self.eligibility_owner(),
            parent,
            leaf_name,
            target,
            label,
        )
        .map_err(|refusal| {
            let refusal = match refusal {
                file_resources::ExternalAdmissionError::Facts(error) => {
                    return envelope(
                        "file.resource_facts_unavailable",
                        ErrorCategory::Unavailable,
                        error.to_string(),
                        false,
                        correlation_id,
                    );
                }
                file_resources::ExternalAdmissionError::Refused(refusal) => refusal,
            };
            // **A live short commit is retryable, and says so in the typed code.**
            //
            // Nothing has been published when this is refused: the resource is held by another
            // execution that will finish, so the caller's asynchronous owner may wait within its
            // original budget and re-enter the unchanged request. A refusal that cannot be waited on
            // — a long editor claim, or a stale-eligibility answer — keeps the non-retryable reading,
            // because waiting cannot change it. Renaming one into the other, or reporting both as a
            // bare conflict, would be exactly the "simulate waiting by renaming an error" the
            // requirement forbids.
            let retryable =
                refusal.retryable && !self.eligibility_owner().is_edit_refusal(&refusal);
            envelope(
                if retryable {
                    file_resources::EXTERNAL_RESOURCE_BUSY_CODE
                } else {
                    "file.external_resource_held"
                },
                ErrorCategory::Conflict,
                format!(
                    "Another execution holds this external file target; no publication started: \
                     {refusal:?}"
                ),
                retryable,
                correlation_id,
            )
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn open(
        persistence: Arc<PersistenceCoordinator>,
        fs: Arc<FsToolService>,
        workspace: Arc<WorkspaceRuntime>,
        settings: &Arc<SettingsFacade>,
        broker: ElevatedBrokerOptions,
        runtime_instance_id: aura_domain_common::RuntimeInstanceId,
        publication_workers: crate::blocking_pool::BlockingPool,
        activity_index: aura_activity_runtime::ActivityIndex,
        activity_observer: aura_activity_runtime::ActivityObserverHandle,
    ) -> Result<Self> {
        // A damaged `settings/security.toml` or `aura.toml` used to `?`
        // out of `P04Services::open`, which aborted the whole Core runtime and
        // took away every unrelated capability plus status, stop and repair.
        //
        // The honest reading is narrower: what is lost is *P04's security
        // authority* — its permission defaults and the Aura default mode. That is
        // refused, with the real reason, and the rest of P04 is still constructed
        // so the process keeps its control paths.
        //
        // No default is substituted. `SecurityConfig::default()` or a default
        // permission mode would be a fabricated authority: the runtime cannot know
        // whether the stored policy was more restrictive, and an invented
        // *looser* default is exactly the silent widening V2 forbids.
        let permission_defaults = match permission_defaults_from_settings(settings) {
            Ok(defaults) => defaults,
            Err(fault) => {
                let (fault_code, domain) = fault.stable_code_and_domain();
                return Err(anyhow!(
                    "P04 security authority is unavailable ({fault_code}, domain {domain}): {fault}"
                ));
            }
        };
        let default_permission_mode = permission_defaults.mode;
        let quarantine = Arc::new(QuarantineStore::new(persistence.as_ref().clone()));
        let external_grants = Arc::new(ExternalGrantService::new(Arc::clone(&persistence)));
        let host_preparation = Arc::new(crate::host_preparation::HostPreparationWorker::start(
            Arc::clone(&persistence),
            record_host_preparation_stage,
        ));
        let services = Self {
            persistence,
            quarantine,
            external_grants,
            fs,
            workspace,
            discard_confirmations: Arc::new(Mutex::new(HashMap::new())),
            host_preparation_nonces: Arc::new(Mutex::new(HashMap::new())),
            host_preparation,
            broker,
            default_permission_mode,
            settings: Arc::clone(settings),
            // This process. The management projection has to report a reusable
            // consent issued by an earlier Core run as no longer in force, and the
            // dispatch path already compares exactly this value.
            runtime_instance_id,
            publication_workers,
            activity_index,
            activity_observer,
        };
        services.recover_security_state()?;
        Ok(services)
    }

    /// Lets the grant list read the session's current turn from the agent journal.
    pub fn bind_turn_journal(&self, journal: Arc<crate::broker::BrokerJournal>) {
        self.external_grants.bind_turn_journal(journal);
    }

    fn prepare_quarantine_release(
        &self,
        input: PrepareQuarantineReleaseInput,
        correlation_id: CorrelationId,
    ) -> Result<PrepareQuarantineReleaseResult, IpcErrorEnvelope> {
        let binding = self.resolve_quarantine_release_binding(
            input.quarantine_id,
            input.workspace_id,
            input.session_id,
            input.target.clone(),
            correlation_id,
        )?;
        self.ensure_permission_state(input.session_id, correlation_id)
            .map_err(|error| persistence_error(error, correlation_id))?;
        let snapshot = self
            .persistence
            .create_permission_snapshot(input.session_id, None, correlation_id)
            .map_err(|error| persistence_error(error, correlation_id))?;
        let mut identity =
            InvocationIdentity::runtime_user(input.workspace_id, input.session_id, correlation_id);
        identity.permission_snapshot_id = Some(snapshot.id);
        let fingerprint = binding
            .operation_fingerprint()
            .map_err(|error| internal_error(error, correlation_id))?;
        let scope = match &input.target {
            QuarantineReleaseTarget::Workspace { path } => ResolvedScope::WorkspacePath {
                workspace_id: input.workspace_id,
                canonical_relative_path: path.clone(),
            },
            QuarantineReleaseTarget::ExternalGrant { external_grant_id, .. } => {
                ResolvedScope::ExternalGrant { grant_id: *external_grant_id }
            }
        };
        let approval = self
            .persistence
            .create_approval_request(
                ApprovalRequestDraft {
                    identity,
                    operation_ids: vec![SecurityOperationId::new()],
                    operation_fingerprints: vec![fingerprint],
                    title: "Release Windows executable".to_owned(),
                    model_purpose: None,
                    actual_operation_summary: format!(
                        "Create a new file for quarantined executable {}.",
                        input.quarantine_id
                    ),
                    source_summary: "Aura Quarantine".to_owned(),
                    source: SecuritySource::UserInterface,
                    source_fingerprint: None,
                    capabilities: vec![ResolvedCapability::DownloadWindowsExecutable],
                    scopes: vec![scope],
                    risk_flags: vec![OperationRiskFlag::ExecutableArtifact],
                    allowed_decisions: vec![
                        ApprovalDecisionKind::AllowOnce,
                        ApprovalDecisionKind::Deny,
                    ],
                    permission_snapshot_id: snapshot.id,
                    release_descriptor: None,
                },
                correlation_id,
            )
            .map_err(|error| persistence_error(error, correlation_id))?;
        Ok(PrepareQuarantineReleaseResult {
            approval_id: approval.id,
            approval_request_revision: approval.request_revision,
            quarantine_id: binding.quarantine_id,
            content_hash: binding.content_hash,
            target: input.target,
        })
    }

    fn release_quarantine(
        &self,
        input: ReleaseQuarantineInput,
        correlation_id: CorrelationId,
    ) -> Result<aura_permission_domain::QuarantineRecordDetails, IpcErrorEnvelope> {
        let binding = self.resolve_quarantine_release_binding(
            input.quarantine_id,
            input.workspace_id,
            input.session_id,
            input.target,
            correlation_id,
        )?;
        let destination = PathBuf::from(&binding.final_destination.display);
        let parent_path = destination.parent().ok_or_else(|| {
            envelope(
                "quarantine.invalid_target",
                ErrorCategory::Validation,
                "Quarantine release target has no parent directory.",
                false,
                correlation_id,
            )
        })?;
        let parent = open_verified_publish_parent(parent_path)
            .map_err(|error| publish_error(error, correlation_id))?;
        if domain_identity(&parent.info().identity) != binding.destination_parent_identity {
            return Err(envelope(
                "quarantine.destination_parent_changed",
                ErrorCategory::Conflict,
                "The approved destination parent changed before release.",
                false,
                correlation_id,
            ));
        }
        let leaf = destination.file_name().ok_or_else(|| {
            envelope(
                "quarantine.invalid_target",
                ErrorCategory::Validation,
                "Quarantine release target has no file name.",
                false,
                correlation_id,
            )
        })?;
        // Refusal happens before the one-time approval or release journal is consumed.
        // The same guard survives publication and every durable settlement below.
        let _admission = self.admit_external_destination(
            &parent,
            &leaf.to_string_lossy(),
            None,
            input.workspace_id,
            input.session_id,
            correlation_id,
        )?;
        publication_dispatch::check_cancelled(correlation_id)?;
        let details = self
            .persistence
            .consume_quarantine_release_authorization(
                binding.clone(),
                input.approval_id,
                correlation_id,
            )
            .map_err(|error| persistence_error(error, correlation_id))?;
        self.persistence
            .mark_quarantine_release_applying(input.quarantine_id)
            .map_err(|error| persistence_error(error, correlation_id))?;
        let _activity = crate::agent_activity::publish_p04_publication_activity(
            self,
            input.workspace_id,
            input.session_id,
            correlation_id,
            None,
            _admission.requests(),
            "quarantine.release",
        );
        let mut external_committed = false;
        let result = (|| {
            let bytes = self
                .quarantine
                .read_verified(&details)
                .map_err(|error| quarantine_error(error, correlation_id))?;
            let mut published =
                handle_bound_create_new(&parent, leaf, &bytes).map_err(|error| {
                    external_committed = error.location() == PublishLocation::Unconfirmable;
                    publish_error(error, correlation_id)
                })?;
            if let Err(error) =
                verify_released_quarantine_file(&binding, &details, &mut published, correlation_id)
            {
                let error = abort_publication_error(published, error, correlation_id);
                external_committed = error.code.as_str() == "artifact.publish_rollback_failed";
                return Err(error);
            }
            published.commit().map_err(|error| {
                external_committed = error.location() == PublishLocation::Unconfirmable;
                publish_error(error, correlation_id)
            })?;
            external_committed = true;
            self.persistence
                .mark_quarantine_release_published(input.quarantine_id)
                .map_err(|error| persistence_error(error, correlation_id))?;
            self.persistence
                .resolve_quarantine(input.quarantine_id, QuarantineState::Released, correlation_id)
                .map_err(|error| persistence_error(error, correlation_id))
        })();
        match result {
            Err(error) => {
                let recorded = if external_committed {
                    self.persistence
                        .interrupt_quarantine_release(input.quarantine_id, error.code.to_string())
                } else {
                    self.persistence
                        .fail_quarantine_release(input.quarantine_id, error.code.to_string())
                };
                Err(file_resources::with_journal_failure(error, recorded))
            }
            Ok(result) => Ok(result),
        }
    }

    fn resolve_quarantine_release_binding(
        &self,
        quarantine_id: QuarantineRecordId,
        workspace_id: WorkspaceId,
        session_id: SessionId,
        target: QuarantineReleaseTarget,
        correlation_id: CorrelationId,
    ) -> Result<QuarantineReleaseBinding, IpcErrorEnvelope> {
        let details = self
            .persistence
            .quarantine_details(quarantine_id)
            .map_err(|error| persistence_error(error, correlation_id))?;
        if details.record.state != QuarantineState::Quarantined {
            return Err(envelope(
                "network.quarantine_not_found",
                ErrorCategory::NotFound,
                "The quarantine record is no longer available.",
                false,
                correlation_id,
            ));
        }
        ensure_session_workspace(&self.persistence, session_id, workspace_id, correlation_id)?;
        let (destination_parent_identity, final_destination) = match &target {
            QuarantineReleaseTarget::Workspace { path } => {
                let resolver = self
                    .workspace
                    .resolver(workspace_id)
                    .map_err(|error| quarantine_workspace_error(error, correlation_id))?;
                let parent = resolver
                    .validate_parent(path)
                    .map_err(|error| quarantine_workspace_error(error, correlation_id))?;
                (
                    domain_identity(&parent.parent.info().identity),
                    canonical_path(parent.parent_absolute_path.join(parent.leaf_name)),
                )
            }
            QuarantineReleaseTarget::ExternalGrant { external_grant_id, relative_path } => {
                let parent = self
                    .external_grants
                    .validate_create_parent(
                        *external_grant_id,
                        session_id,
                        relative_path,
                        correlation_id,
                    )
                    .map_err(|error| external_grant_error(error, correlation_id))?;
                if parent.grant.workspace_id != workspace_id {
                    return Err(envelope(
                        "file.grant_session_mismatch",
                        ErrorCategory::Permission,
                        "The External Grant does not belong to this Workspace and Session.",
                        false,
                        correlation_id,
                    ));
                }
                (parent.parent_identity, parent.final_destination)
            }
        };
        Ok(QuarantineReleaseBinding {
            quarantine_id,
            content_hash: details.record.content_hash,
            workspace_id,
            session_id,
            target,
            destination_parent_identity,
            final_destination,
        })
    }

    fn issue_quarantine_discard_confirmation(
        &self,
        input: IssueQuarantineDiscardConfirmationInput,
        correlation_id: CorrelationId,
    ) -> Result<QuarantineDiscardConfirmation, IpcErrorEnvelope> {
        let details = self
            .persistence
            .quarantine_details(input.quarantine_id)
            .map_err(|error| persistence_error(error, correlation_id))?;
        if details.record.state != QuarantineState::Quarantined {
            return Err(envelope(
                "network.quarantine_not_found",
                ErrorCategory::NotFound,
                "The quarantine record is no longer available.",
                false,
                correlation_id,
            ));
        }
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes).map_err(|error| internal_error(error, correlation_id))?;
        let nonce = SecretNonce::from_bytes(bytes);
        let expires_at =
            UtcTimestamp::from_value(UtcTimestamp::now().value() + time::Duration::seconds(60));
        let token_hash = *blake3::hash(nonce.expose().as_bytes()).as_bytes();
        self.discard_confirmations
            .lock()
            .map_err(|_| {
                envelope(
                    "security.internal",
                    ErrorCategory::Internal,
                    "Quarantine confirmation state is unavailable.",
                    false,
                    correlation_id,
                )
            })?
            .insert(
                input.quarantine_id,
                PendingDiscardConfirmation {
                    token_hash,
                    content_hash: details.record.content_hash.clone(),
                    expires_at,
                },
            );
        Ok(QuarantineDiscardConfirmation {
            quarantine_id: input.quarantine_id,
            content_hash: details.record.content_hash,
            confirmation_nonce: nonce,
            expires_at,
        })
    }

    fn discard_quarantine(
        &self,
        input: DiscardQuarantineInput,
        correlation_id: CorrelationId,
    ) -> Result<aura_permission_domain::QuarantineRecordDetails, IpcErrorEnvelope> {
        let details = self
            .persistence
            .quarantine_details(input.quarantine_id)
            .map_err(|error| persistence_error(error, correlation_id))?;
        let pending = self
            .discard_confirmations
            .lock()
            .map_err(|_| internal_error("discard confirmation lock poisoned", correlation_id))?
            .remove(&input.quarantine_id)
            .ok_or_else(|| {
                envelope(
                    "permission.confirmation_required",
                    ErrorCategory::Permission,
                    "A fresh one-time discard confirmation is required.",
                    false,
                    correlation_id,
                )
            })?;
        let presented = *blake3::hash(input.confirmation_nonce.expose().as_bytes()).as_bytes();
        if UtcTimestamp::now() > pending.expires_at
            || pending.content_hash != details.record.content_hash
            || !constant_time_eq(&presented, &pending.token_hash)
        {
            return Err(envelope(
                "permission.confirmation_expired",
                ErrorCategory::Permission,
                "The one-time discard confirmation is invalid or expired.",
                false,
                correlation_id,
            ));
        }
        self.quarantine
            .discard(&details, correlation_id)
            .map_err(|error| quarantine_error(error, correlation_id))?;
        self.persistence
            .quarantine_details(input.quarantine_id)
            .map_err(|error| persistence_error(error, correlation_id))
    }

    fn export_artifact(
        &self,
        input: ArtifactExportRequest,
        correlation_id: CorrelationId,
    ) -> Result<ArtifactExportResult, IpcErrorEnvelope> {
        ensure_session_workspace(
            &self.persistence,
            input.session_id,
            input.workspace_id,
            correlation_id,
        )?;
        if matches!(input.expected, FsExpectedState::Directory) {
            return Err(envelope(
                "artifact.invalid_expected_state",
                ErrorCategory::Validation,
                "Artifact export requires expected_kind=absent or file_hash.",
                false,
                correlation_id,
            ));
        }
        let (artifact, bytes) = self.read_export_artifact(&input, correlation_id)?;
        let target_name = export_target_name(&input.target);
        if !ExecutableDetector
            .detect(target_name, Some(&artifact.media_type), None, &bytes)
            .is_empty()
        {
            return Err(envelope(
                "quarantine.release_required",
                ErrorCategory::Permission,
                "Windows executable and installer artifacts require Quarantine Release.",
                false,
                correlation_id,
            ));
        }
        let (disposition, deferred_mutation_id, requires_reread) = match &input.target {
            ArtifactExportTarget::Workspace { path } => {
                self.authorize_workspace_artifact_export(&input, &artifact, correlation_id)?;
                let observation = crate::agent_activity::p04_file_observer(
                    self,
                    input.workspace_id,
                    input.session_id,
                    correlation_id,
                );
                let mutation =
                    aura_fs_tools::with_file_execution_observer(Some(observation), || {
                        self.fs.export_artifact_bytes(
                            input.workspace_id,
                            input.session_id,
                            path.clone(),
                            bytes,
                            input.expected.clone(),
                            correlation_id,
                        )
                    })
                    .map_err(|error| artifact_fs_error(error, correlation_id))?;
                (mutation.disposition, mutation.deferred_mutation_id, mutation.requires_reread)
            }
            ArtifactExportTarget::ExternalGrant { external_grant_id, relative_path } => {
                self.export_artifact_to_external_grant(
                    *external_grant_id,
                    input.workspace_id,
                    input.session_id,
                    relative_path,
                    &input.expected,
                    &bytes,
                    &artifact.content_hash,
                    correlation_id,
                )?;
                (FsMutationDisposition::Applied, None, false)
            }
        };
        Ok(ArtifactExportResult {
            artifact: artifact.clone(),
            target: input.target,
            disposition,
            content_hash: artifact.content_hash,
            deferred_mutation_id,
            requires_reread,
        })
    }

    fn read_export_artifact(
        &self,
        input: &ArtifactExportRequest,
        correlation_id: CorrelationId,
    ) -> Result<(ArtifactRecord, Vec<u8>), IpcErrorEnvelope> {
        let artifact = self
            .persistence
            .artifact_record(ArtifactMetadataRequest {
                artifact_id: input.artifact_id,
                session_id: Some(input.session_id),
                required_access: ArtifactAccess::EXPORT,
                allow_quarantine: false,
            })
            .map_err(|error| persistence_error(error, correlation_id))?;
        let capacity = usize::try_from(artifact.byte_length).map_err(|_| {
            envelope(
                "artifact.too_large",
                ErrorCategory::Validation,
                "Artifact is too large for the current export transaction.",
                false,
                correlation_id,
            )
        })?;
        let mut bytes = Vec::with_capacity(capacity);
        let mut offset = 0_u64;
        while offset < artifact.byte_length {
            let length = usize::try_from(artifact.byte_length.saturating_sub(offset))
                .unwrap_or(ARTIFACT_RANGE_LIMIT)
                .min(ARTIFACT_RANGE_LIMIT);
            let chunk = self
                .persistence
                .read_artifact_range(ArtifactReadRequest {
                    artifact_id: artifact.artifact_id,
                    session_id: Some(input.session_id),
                    required_access: ArtifactAccess::EXPORT,
                    offset,
                    length,
                    allow_quarantine: false,
                })
                .map_err(|error| persistence_error(error, correlation_id))?;
            if chunk.is_empty() {
                return Err(envelope(
                    "artifact.integrity_failed",
                    ErrorCategory::Internal,
                    "Artifact Store returned an incomplete export stream.",
                    false,
                    correlation_id,
                ));
            }
            offset = offset.saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
            bytes.extend_from_slice(&chunk);
        }
        if bytes.len() != capacity || ContentHash::blake3(&bytes) != artifact.content_hash {
            return Err(envelope(
                "artifact.integrity_failed",
                ErrorCategory::Internal,
                "Artifact bytes do not match the persisted length and hash.",
                false,
                correlation_id,
            ));
        }
        Ok((artifact, bytes))
    }

    fn authorize_workspace_artifact_export(
        &self,
        input: &ArtifactExportRequest,
        artifact: &ArtifactRecord,
        correlation_id: CorrelationId,
    ) -> Result<(), IpcErrorEnvelope> {
        let ArtifactExportTarget::Workspace { path } = &input.target else {
            return Err(internal_error("workspace Artifact target expected", correlation_id));
        };
        self.ensure_permission_state(input.session_id, correlation_id)
            .map_err(|error| persistence_error(error, correlation_id))?;
        let snapshot = self
            .persistence
            .create_permission_snapshot(input.session_id, None, correlation_id)
            .map_err(|error| persistence_error(error, correlation_id))?;
        let mut identity =
            InvocationIdentity::runtime_user(input.workspace_id, input.session_id, correlation_id);
        identity.permission_snapshot_id = Some(snapshot.id);
        let capability = match input.expected {
            FsExpectedState::Absent => ResolvedCapability::WorkspaceFileCreate,
            FsExpectedState::FileHash { .. } => ResolvedCapability::WorkspaceFileWrite,
            FsExpectedState::Directory => unreachable!("validated before authorization"),
        };
        crate::workspace_policy::authorize(
            method::ARTIFACT_EXPORT,
            &serde_json::json!({"request":input,"artifact_hash":artifact.content_hash}),
            identity,
            snapshot,
            vec![capability],
            vec![path.clone()],
        )
        .map_err(|reason| {
            envelope(
                "permission.denied",
                ErrorCategory::Permission,
                format!("Artifact export was denied: {reason:?}."),
                false,
                correlation_id,
            )
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn export_artifact_to_external_grant(
        &self,
        external_grant_id: aura_domain_common::ExternalGrantId,
        workspace_id: WorkspaceId,
        session_id: SessionId,
        relative_path: &WorkspacePath,
        expected: &FsExpectedState,
        bytes: &[u8],
        content_hash: &ContentHash,
        correlation_id: CorrelationId,
    ) -> Result<(), IpcErrorEnvelope> {
        match expected {
            FsExpectedState::Absent => {
                let parent = self
                    .external_grants
                    .validate_create_parent(
                        external_grant_id,
                        session_id,
                        relative_path,
                        correlation_id,
                    )
                    .map_err(|error| external_grant_error(error, correlation_id))?;
                if parent.grant.workspace_id != workspace_id {
                    return Err(envelope(
                        "file.grant_session_mismatch",
                        ErrorCategory::Permission,
                        "The External Grant does not belong to this Workspace and Session.",
                        false,
                        correlation_id,
                    ));
                }
                let destination = PathBuf::from(&parent.final_destination.display);
                // **The common admission, taken before the first side effect.**
                //
                // The checked platform publisher below does the final conditional commit, and that is
                // a *different* responsibility: it protects against an external writer at the last
                // instant. It cannot coordinate Aura's own entries, because it knows nothing about
                // who else inside this Core is writing the same real resource. Without this commit a
                // External export and a Main Fs write reaching one physical file would each take
                // their own decision and both would publish.
                let _admission = self.admit_external_destination(
                    &parent.parent,
                    &parent.leaf_name.to_string_lossy(),
                    None,
                    workspace_id,
                    session_id,
                    correlation_id,
                )?;
                publication_dispatch::check_cancelled(correlation_id)?;
                // P04's external export is a real publication with its own owner, so it
                // publishes its own activity under the lifecycle/publication identity it really
                // has - never an Agent run. The fact is published before the conditional publish
                // below and ended immediately after it, because the interval that matters to a
                // reader is exactly the one where this owner is writing the real target.
                let mut p04_activity = crate::agent_activity::publish_p04_publication_activity(
                    self,
                    workspace_id,
                    session_id,
                    correlation_id,
                    None,
                    _admission.requests(),
                    "artifact.export_external",
                );
                let published_result = (|| -> Result<(), IpcErrorEnvelope> {
                    let mut published =
                        handle_bound_create_new(&parent.parent, &parent.leaf_name, bytes)
                            .map_err(|error| publish_error(error, correlation_id))?;
                    if let Err(error) = self.verify_external_export(
                        external_grant_id,
                        session_id,
                        relative_path,
                        ExternalGrantCapability::Create,
                        &destination,
                        content_hash,
                        &mut published,
                        correlation_id,
                    ) {
                        return Err(abort_publication_error(published, error, correlation_id));
                    }
                    published.commit().map_err(|error| publish_error(error, correlation_id))?;
                    Ok(())
                })();
                // The activity ends before the publication's own error is propagated, so a failed
                // publication is not left advertised as still running. The guard also covers the
                // `?` below and any exit added later.
                p04_activity.end();
                // The publication is over — succeeded or failed — so the admission ends here. A
                // failure that left material behind keeps its own recovery responsibility, which the
                // abort path above already recorded.
                published_result?;
            }
            FsExpectedState::FileHash { hash } => {
                let mut resolved = self
                    .external_grants
                    .resolve_existing(
                        external_grant_id,
                        session_id,
                        relative_path,
                        ExternalGrantCapability::Write,
                        NativeFileAccess::Read,
                        correlation_id,
                    )
                    .map_err(|error| external_grant_error(error, correlation_id))?;
                if resolved.grant.workspace_id != workspace_id {
                    return Err(envelope(
                        "file.grant_session_mismatch",
                        ErrorCategory::Permission,
                        "The External Grant does not belong to this Workspace and Session.",
                        false,
                        correlation_id,
                    ));
                }
                let actual =
                    hash_verified_reader(resolved.file.file_mut(), hash.algorithm, correlation_id)?;
                if actual != *hash {
                    return Err(envelope(
                        "artifact.expected_hash_mismatch",
                        ErrorCategory::Conflict,
                        format!("Expected {hash}, but the external target is {actual}."),
                        false,
                        correlation_id,
                    ));
                }
                let destination = resolved.file.info().final_path.clone();
                let expected_identity = resolved.file.info().identity.clone();
                let parent_path = destination.parent().ok_or_else(|| {
                    envelope(
                        "artifact.invalid_target",
                        ErrorCategory::Validation,
                        "Artifact export target has no parent directory.",
                        false,
                        correlation_id,
                    )
                })?;
                let parent = open_verified_publish_parent(parent_path)
                    .map_err(|error| publish_error(error, correlation_id))?;
                if !final_path_within(parent.info(), resolved.file.info()) {
                    return Err(envelope(
                        "artifact.target_changed",
                        ErrorCategory::Conflict,
                        "The Artifact target parent changed before replacement.",
                        false,
                        correlation_id,
                    ));
                }
                let leaf = destination.file_name().ok_or_else(|| {
                    envelope(
                        "artifact.invalid_target",
                        ErrorCategory::Validation,
                        "Artifact export target has no file name.",
                        false,
                        correlation_id,
                    )
                })?;
                drop(resolved);
                let _admission = self.admit_external_destination(
                    &parent,
                    &leaf.to_string_lossy(),
                    Some(expected_identity.clone()),
                    workspace_id,
                    session_id,
                    correlation_id,
                )?;
                publication_dispatch::check_cancelled(correlation_id)?;
                let operation_id = SecurityOperationId::new();
                let backup_file_name = format!(".aura-backup-{operation_id}.tmp");
                self.persistence
                    .begin_artifact_overwrite(
                        ArtifactOverwriteJournalBinding {
                            operation_id,
                            workspace_id,
                            session_id,
                            external_grant_id,
                            relative_path: relative_path.clone(),
                            final_destination: canonical_path(destination.clone()),
                            backup_file_name: backup_file_name.clone(),
                            expected_target_identity: domain_identity(&expected_identity),
                            expected_content_hash: hash.clone(),
                            replacement_content_hash: content_hash.clone(),
                        },
                        correlation_id,
                    )
                    .map_err(|error| persistence_error(error, correlation_id))?;
                self.persistence
                    .transition_artifact_overwrite(
                        operation_id,
                        ArtifactOverwriteJournalState::Applying,
                        None,
                        correlation_id,
                    )
                    .map_err(|error| persistence_error(error, correlation_id))?;
                // The caller's original content condition, with the algorithm it was recorded
                // under, is carried to the exclusive handle. This is the condition the decision
                // was actually taken against: `hash` is the `expected_state` the request
                // supplied and that the journal bound, checked above against the resolved
                // target. It is deliberately NOT a fresh hash taken after admission, which
                // could only describe bytes nobody decided about.
                //
                // The algorithm is named explicitly so a SHA-256 digest is recomputed as
                // SHA-256. Feeding that hex to the BLAKE3 comparison would compare two
                // different functions' output and could only ever be wrong; there is no
                // conversion between algorithms here, because both sides hash the same object.
                let expected_content = HandleBoundReplaceContent::Hex {
                    algorithm: platform_hash_algorithm(hash.algorithm),
                    expected: hash.hex.as_str(),
                };
                let _activity = crate::agent_activity::publish_p04_publication_activity(
                    self,
                    workspace_id,
                    session_id,
                    correlation_id,
                    Some(operation_id),
                    _admission.requests(),
                    "artifact.overwrite_external",
                );
                let mut published = match handle_bound_replace_with_backup_checked(
                    &parent,
                    leaf,
                    &expected_identity,
                    expected_content,
                    bytes,
                    backup_file_name.as_ref(),
                ) {
                    Ok(published) => published,
                    Err(error) => {
                        // The journal records the real stage and the compensation result the
                        // platform reported, not a classification guessed from the error kind:
                        // `Unconfirmable` means the destination is not known to hold either
                        // version, so it must never be recorded as a clean abort.
                        let next = match error.location() {
                            PublishLocation::NeverPublished => {
                                ArtifactOverwriteJournalState::Aborted
                            }
                            PublishLocation::RolledBack => ArtifactOverwriteJournalState::Aborted,
                            PublishLocation::Unconfirmable => {
                                ArtifactOverwriteJournalState::NeedsReconcile
                            }
                        };
                        let record = self.persistence.transition_artifact_overwrite(
                            operation_id,
                            next,
                            Some(error.to_string()),
                            correlation_id,
                        );
                        return Err(file_resources::with_journal_failure(
                            publish_error(error, correlation_id),
                            record,
                        ));
                    }
                };
                if let Err(error) = self.verify_external_export(
                    external_grant_id,
                    session_id,
                    relative_path,
                    ExternalGrantCapability::Write,
                    &destination,
                    content_hash,
                    &mut published,
                    correlation_id,
                ) {
                    let operation_error_code = error.code.to_string();
                    let operation_error_message = error.message.clone();
                    if let Err(rollback) = published.abort() {
                        let record = self.persistence.transition_artifact_overwrite(
                            operation_id,
                            ArtifactOverwriteJournalState::NeedsReconcile,
                            Some(rollback.to_string()),
                            correlation_id,
                        );
                        return Err(file_resources::with_journal_failure(
                            envelope(
                                "artifact.publish_rollback_failed",
                                ErrorCategory::Internal,
                                format!(
                                    "Artifact verification failed ({operation_error_code}: {operation_error_message}); rollback failed ({rollback})."
                                ),
                                false,
                                correlation_id,
                            ),
                            record,
                        ));
                    }
                    let record = self.persistence.transition_artifact_overwrite(
                        operation_id,
                        ArtifactOverwriteJournalState::Aborted,
                        Some(operation_error_code),
                        correlation_id,
                    );
                    return Err(file_resources::with_journal_failure(error, record));
                }
                if let Err(error) = published.commit() {
                    let next = if error.location() == PublishLocation::Unconfirmable {
                        ArtifactOverwriteJournalState::NeedsReconcile
                    } else {
                        ArtifactOverwriteJournalState::Aborted
                    };
                    let record = self.persistence.transition_artifact_overwrite(
                        operation_id,
                        next,
                        Some(error.to_string()),
                        correlation_id,
                    );
                    return Err(file_resources::with_journal_failure(
                        publish_error(error, correlation_id),
                        record,
                    ));
                }
                #[cfg(test)]
                publication_recovery_tests::interrupt_at("applying");
                #[cfg(all(feature = "acceptance-barrier", debug_assertions))]
                aura_fs_tools::file_acceptance::hold_publication(
                    false,
                    operation_id,
                    correlation_id,
                );
                self.persistence
                    .transition_artifact_overwrite(
                        operation_id,
                        ArtifactOverwriteJournalState::Published,
                        None,
                        correlation_id,
                    )
                    .map_err(|error| {
                        file_resources::unrecorded_publication(error, operation_id, correlation_id)
                    })?;
                #[cfg(test)]
                publication_recovery_tests::interrupt_at("published");
                #[cfg(all(feature = "acceptance-barrier", debug_assertions))]
                aura_fs_tools::file_acceptance::hold_publication(
                    true,
                    operation_id,
                    correlation_id,
                );
                self.persistence
                    .transition_artifact_overwrite(
                        operation_id,
                        ArtifactOverwriteJournalState::Completed,
                        None,
                        correlation_id,
                    )
                    .map_err(|error| {
                        file_resources::unrecorded_publication(error, operation_id, correlation_id)
                    })?;
            }
            FsExpectedState::Directory => unreachable!("validated before external export"),
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn verify_external_export(
        &self,
        external_grant_id: aura_domain_common::ExternalGrantId,
        session_id: SessionId,
        relative_path: &WorkspacePath,
        capability: ExternalGrantCapability,
        expected_destination: &std::path::Path,
        expected_hash: &ContentHash,
        published: &mut HandleBoundPublishedFile,
        correlation_id: CorrelationId,
    ) -> Result<(), IpcErrorEnvelope> {
        self.external_grants
            .verify_published_file(
                external_grant_id,
                session_id,
                relative_path,
                capability,
                published.info(),
                correlation_id,
            )
            .map_err(|error| external_grant_error(error, correlation_id))?;
        if !same_windows_path(&published.info().final_path, expected_destination) {
            return Err(envelope(
                "artifact.export_integrity_failed",
                ErrorCategory::Internal,
                "The exported target failed held-handle final path verification.",
                false,
                correlation_id,
            ));
        }
        published.file_mut().seek(SeekFrom::Start(0)).map_err(|error| {
            envelope(
                "artifact.export_io_failed",
                ErrorCategory::Unavailable,
                format!("Failed to seek the held Artifact publication: {error}"),
                false,
                correlation_id,
            )
        })?;
        if hash_verified_reader(published.file_mut(), expected_hash.algorithm, correlation_id)?
            != *expected_hash
        {
            return Err(envelope(
                "artifact.export_integrity_failed",
                ErrorCategory::Internal,
                "The exported target failed held-handle content verification.",
                false,
                correlation_id,
            ));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyInput {}

fn ensure_session_workspace(
    persistence: &PersistenceCoordinator,
    session_id: SessionId,
    workspace_id: WorkspaceId,
    correlation_id: CorrelationId,
) -> Result<(), IpcErrorEnvelope> {
    let matches = persistence
        .workspace_session_projection()
        .map_err(|error| persistence_error(error, correlation_id))?
        .sessions
        .into_iter()
        .any(|session| session.session_id == session_id && session.workspace_id == workspace_id);
    if !matches {
        return Err(envelope(
            "file.grant_session_mismatch",
            ErrorCategory::Permission,
            "The Session does not belong to the requested Workspace.",
            false,
            correlation_id,
        ));
    }
    Ok(())
}

fn export_target_name(target: &ArtifactExportTarget) -> &str {
    let path = match target {
        ArtifactExportTarget::Workspace { path }
        | ArtifactExportTarget::ExternalGrant { relative_path: path, .. } => path,
    };
    path.as_str().rsplit('/').next().filter(|name| !name.is_empty()).unwrap_or("artifact")
}

/// The platform's digest enumeration for the caller's own algorithm.
///
/// The two enumerations are deliberately separate: the workspace contract owns what a
/// `ContentHash` means, and the platform owns what it can recompute over a held handle. Mapping
/// here, at the single call site that needs it, keeps the platform free of the workspace's types
/// and keeps the mapping total — a new algorithm on either side is a compile error rather than a
/// silent fallback to BLAKE3, which would compare a SHA-256 digest against a BLAKE3 one.
fn platform_hash_algorithm(algorithm: ContentHashAlgorithm) -> PlatformHashAlgorithm {
    match algorithm {
        ContentHashAlgorithm::Blake3 => PlatformHashAlgorithm::Blake3,
        ContentHashAlgorithm::Sha256 => PlatformHashAlgorithm::Sha256,
    }
}

fn hash_verified_reader(
    file: &mut std::fs::File,
    algorithm: ContentHashAlgorithm,
    correlation_id: CorrelationId,
) -> Result<ContentHash, IpcErrorEnvelope> {
    let mut buffer = [0_u8; 64 * 1024];
    match algorithm {
        ContentHashAlgorithm::Blake3 => {
            let mut hasher = blake3::Hasher::new();
            loop {
                let read = file.read(&mut buffer).map_err(|error| {
                    envelope(
                        "artifact.export_io_failed",
                        ErrorCategory::Unavailable,
                        format!("Failed to read verified Artifact target: {error}"),
                        false,
                        correlation_id,
                    )
                })?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            ContentHash::new(ContentHashAlgorithm::Blake3, hasher.finalize().to_hex().to_string())
                .map_err(|error| internal_error(error, correlation_id))
        }
        ContentHashAlgorithm::Sha256 => {
            let mut hasher = Sha256::new();
            loop {
                let read = file.read(&mut buffer).map_err(|error| {
                    envelope(
                        "artifact.export_io_failed",
                        ErrorCategory::Unavailable,
                        format!("Failed to read verified Artifact target: {error}"),
                        false,
                        correlation_id,
                    )
                })?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            ContentHash::new(ContentHashAlgorithm::Sha256, format!("{:x}", hasher.finalize()))
                .map_err(|error| internal_error(error, correlation_id))
        }
    }
}

fn artifact_fs_error(error: FsToolError, correlation_id: CorrelationId) -> IpcErrorEnvelope {
    let message = error.to_string();
    match error {
        FsToolError::ResourceBusy(_) => envelope(
            crate::p03::FILE_RESOURCE_BUSY_CODE,
            ErrorCategory::Conflict,
            message,
            true,
            correlation_id,
        ),
        FsToolError::Persistence(error) => persistence_error(error, correlation_id),
        FsToolError::Workspace(error) => quarantine_workspace_error(error, correlation_id),
        FsToolError::ExpectedAbsent { .. } | FsToolError::ExpectedHash { .. } => envelope(
            "artifact.expected_state_mismatch",
            ErrorCategory::Conflict,
            message,
            false,
            correlation_id,
        ),
        FsToolError::ProjectionFailed { .. } => envelope(
            "projection.unavailable",
            ErrorCategory::Unavailable,
            message,
            true,
            correlation_id,
        ),
        FsToolError::ReconciliationRequired(_) => envelope(
            "workspace.reconciliation_required",
            ErrorCategory::Conflict,
            message,
            false,
            correlation_id,
        ),
        FsToolError::Io { .. } | FsToolError::ChangesetFailed { .. } => envelope(
            "artifact.export_io_failed",
            ErrorCategory::Unavailable,
            message,
            false,
            correlation_id,
        ),
        _ => envelope(
            "artifact.invalid_export",
            ErrorCategory::Validation,
            message,
            false,
            correlation_id,
        ),
    }
}

fn canonical_path(path: PathBuf) -> CanonicalPath {
    let display = path.display().to_string();
    CanonicalPath { utf16: display.encode_utf16().collect(), display }
}

fn verify_released_quarantine_file(
    binding: &QuarantineReleaseBinding,
    details: &aura_permission_domain::QuarantineRecordDetails,
    published: &mut HandleBoundPublishedFile,
    correlation_id: CorrelationId,
) -> Result<(), IpcErrorEnvelope> {
    let destination = PathBuf::from(&binding.final_destination.display);
    if !same_windows_path(&published.info().final_path, &destination) {
        return Err(envelope(
            "network.quarantine_integrity_failed",
            ErrorCategory::Internal,
            "The released executable failed held-handle final path verification.",
            false,
            correlation_id,
        ));
    }
    if domain_identity(&published.parent_info().identity) != binding.destination_parent_identity {
        return Err(envelope(
            "network.quarantine_integrity_failed",
            ErrorCategory::Internal,
            "The released executable parent identity changed during release.",
            false,
            correlation_id,
        ));
    }
    published.file_mut().seek(SeekFrom::Start(0)).map_err(|error| {
        envelope(
            "network.quarantine_integrity_failed",
            ErrorCategory::Internal,
            format!("Failed to seek the held released executable: {error}"),
            false,
            correlation_id,
        )
    })?;
    let read_limit = details.record.byte_length.saturating_add(1);
    let mut bytes = Vec::with_capacity(
        usize::try_from(details.record.byte_length).unwrap_or(0).min(16 * 1024 * 1024),
    );
    let mut reader = published.file_mut().take(read_limit);
    reader.read_to_end(&mut bytes).map_err(|error| {
        envelope(
            "network.quarantine_integrity_failed",
            ErrorCategory::Internal,
            format!("Failed to verify the released executable bytes: {error}"),
            false,
            correlation_id,
        )
    })?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != details.record.byte_length
        || ContentHash::blake3(&bytes) != details.record.content_hash
    {
        return Err(envelope(
            "network.quarantine_integrity_failed",
            ErrorCategory::Internal,
            "The released executable does not match the quarantined content hash.",
            false,
            correlation_id,
        ));
    }
    Ok(())
}

fn same_windows_path(left: &std::path::Path, right: &std::path::Path) -> bool {
    let left = left.components().collect::<Vec<_>>();
    let right = right.components().collect::<Vec<_>>();
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
        })
}

fn constant_time_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter().zip(right).fold(0_u8, |difference, (left, right)| difference | (left ^ right)) == 0
}

fn external_grant_error(
    error: ExternalGrantError,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    let message = error.to_string();
    match error {
        ExternalGrantError::Persistence(error) => persistence_error(error, correlation_id),
        ExternalGrantError::InvalidSelection => envelope(
            "file.invalid_external_selection",
            ErrorCategory::Validation,
            message,
            false,
            correlation_id,
        ),
        ExternalGrantError::IdentityUnavailable => envelope(
            "file.grant_identity_unavailable",
            ErrorCategory::Unavailable,
            message,
            false,
            correlation_id,
        ),
        ExternalGrantError::IdentityChanged => envelope(
            "file.grant_identity_changed",
            ErrorCategory::Conflict,
            message,
            false,
            correlation_id,
        ),
        ExternalGrantError::Suspended => envelope(
            "file.grant_suspended",
            ErrorCategory::Unavailable,
            message,
            false,
            correlation_id,
        ),
        ExternalGrantError::Revoked
        | ExternalGrantError::Expired
        | ExternalGrantError::PermissionGrantUnavailable => envelope(
            "permission.grant_not_applicable",
            ErrorCategory::Permission,
            message,
            false,
            correlation_id,
        ),
        ExternalGrantError::CapabilityUnavailable => envelope(
            "file.grant_capability_unavailable",
            ErrorCategory::Permission,
            message,
            false,
            correlation_id,
        ),
        ExternalGrantError::ScopeUnavailable => envelope(
            "file.scope_unavailable",
            ErrorCategory::Permission,
            message,
            false,
            correlation_id,
        ),
        ExternalGrantError::PathResolution(_) => envelope(
            "workspace.path_resolution_failed",
            ErrorCategory::Unavailable,
            message,
            false,
            correlation_id,
        ),
    }
}

fn quarantine_workspace_error(
    error: WorkspaceError,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    let message = error.to_string();
    match error {
        WorkspaceError::Persistence(error) => persistence_error(error, correlation_id),
        WorkspaceError::ScopeUnavailable { .. } => envelope(
            "file.scope_unavailable",
            ErrorCategory::Permission,
            message,
            false,
            correlation_id,
        ),
        WorkspaceError::NotFound { .. } | WorkspaceError::WrongKind { .. } => envelope(
            "workspace.path_not_found",
            ErrorCategory::NotFound,
            message,
            false,
            correlation_id,
        ),
        WorkspaceError::Offline | WorkspaceError::Archived => envelope(
            "workspace.unavailable",
            ErrorCategory::Unavailable,
            message,
            false,
            correlation_id,
        ),
        WorkspaceError::PathResolutionFailed { .. } | WorkspaceError::RootIdentityChanged => {
            envelope(
                "workspace.path_resolution_failed",
                ErrorCategory::Unavailable,
                message,
                false,
                correlation_id,
            )
        }
        _ => {
            envelope("workspace.internal", ErrorCategory::Internal, message, false, correlation_id)
        }
    }
}

fn quarantine_error(error: NetworkControlError, correlation_id: CorrelationId) -> IpcErrorEnvelope {
    let message = error.to_string();
    match error {
        NetworkControlError::Persistence(error) => persistence_error(error, correlation_id),
        NetworkControlError::QuarantineIo(error)
            if error.kind() == std::io::ErrorKind::AlreadyExists =>
        {
            envelope(
                "quarantine.target_already_exists",
                ErrorCategory::Conflict,
                message,
                false,
                correlation_id,
            )
        }
        NetworkControlError::QuarantineIo(_) => envelope(
            "network.quarantine_io_failed",
            ErrorCategory::Unavailable,
            message,
            false,
            correlation_id,
        ),
        NetworkControlError::QuarantineIntegrity => envelope(
            "network.quarantine_integrity_failed",
            ErrorCategory::Internal,
            message,
            false,
            correlation_id,
        ),
        NetworkControlError::QuarantineNotFound => envelope(
            "network.quarantine_not_found",
            ErrorCategory::NotFound,
            message,
            false,
            correlation_id,
        ),
        NetworkControlError::QuarantineReleaseNotApproved => envelope(
            "permission.approval_required",
            ErrorCategory::Permission,
            message,
            false,
            correlation_id,
        ),
        _ => envelope("network.internal", ErrorCategory::Internal, message, false, correlation_id),
    }
}

fn publish_error(error: impl std::fmt::Display, correlation_id: CorrelationId) -> IpcErrorEnvelope {
    envelope(
        "artifact.handle_bound_publish_failed",
        ErrorCategory::Unavailable,
        format!("Handle-bound file publication failed: {error}"),
        false,
        correlation_id,
    )
}

fn abort_publication_error(
    published: HandleBoundPublishedFile,
    operation: IpcErrorEnvelope,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    match published.abort() {
        Ok(()) => operation,
        Err(rollback) => envelope(
            "artifact.publish_rollback_failed",
            ErrorCategory::Internal,
            format!(
                "File publication failed ({}: {}); rollback failed ({rollback}).",
                operation.code, operation.message
            ),
            false,
            correlation_id,
        ),
    }
}

fn decode<T: DeserializeOwned>(
    value: &Value,
    correlation_id: CorrelationId,
) -> Result<T, IpcErrorEnvelope> {
    serde_json::from_value(value.clone()).map_err(|error| {
        envelope(
            "ipc.invalid_params",
            ErrorCategory::Validation,
            error.to_string(),
            false,
            correlation_id,
        )
    })
}

fn decode_empty(value: &Value, correlation_id: CorrelationId) -> Result<(), IpcErrorEnvelope> {
    decode::<EmptyInput>(value, correlation_id).map(|_| ())
}

fn encode(
    value: impl serde::Serialize,
    correlation_id: CorrelationId,
) -> Result<Value, IpcErrorEnvelope> {
    serde_json::to_value(value).map_err(|error| {
        envelope(
            "ipc.encode_failed",
            ErrorCategory::Internal,
            error.to_string(),
            false,
            correlation_id,
        )
    })
}

/// Maps a consent management failure onto the IPC surface.
///
/// The class is what matters: a stale revision is a conflict the caller can retry
/// after reloading, an absent consent is not, and an unbound journal is a Core
/// state problem rather than something the user did.
fn consent_error(
    error: crate::native_session_grants::NativeSessionConsentError,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    use crate::native_session_grants::NativeSessionConsentError as E;
    let (code, category) = match error {
        E::SessionNotFound => ("permission.session_not_found", ErrorCategory::NotFound),
        E::NotFound => ("permission.grant_not_found", ErrorCategory::NotFound),
        E::RevisionConflict => ("permission.grant_state_changed", ErrorCategory::Conflict),
        E::Unusable => ("permission.grant_not_applicable", ErrorCategory::NotFound),
        E::JournalUnbound => ("security.journal_unbound", ErrorCategory::Unavailable),
        E::Store => ("security.store_unavailable", ErrorCategory::Unavailable),
    };
    envelope(code, category, error.to_string(), false, correlation_id)
}

fn persistence_error(error: PersistenceError, correlation_id: CorrelationId) -> IpcErrorEnvelope {
    let message = error.to_string();
    let (code, category, retryable) = match error {
        PersistenceError::PermissionStateNotFound => {
            ("permission.session_not_found", ErrorCategory::NotFound, false)
        }
        PersistenceError::PermissionRevisionConflict => {
            ("permission.mode_changed", ErrorCategory::Conflict, false)
        }
        PersistenceError::PermissionSnapshotNotFound => {
            ("permission.snapshot_stale", ErrorCategory::Conflict, false)
        }
        PersistenceError::PermissionGrantNotFound => {
            ("permission.grant_not_applicable", ErrorCategory::NotFound, false)
        }
        PersistenceError::ExternalGrantNotFound => {
            ("file.grant_not_found", ErrorCategory::NotFound, false)
        }
        PersistenceError::ExternalGrantSessionMismatch => {
            ("file.grant_session_mismatch", ErrorCategory::Permission, false)
        }
        PersistenceError::ExternalGrantStateConflict => {
            ("file.grant_state_changed", ErrorCategory::Conflict, false)
        }
        PersistenceError::ApprovalNotFound => {
            ("approval.not_found", ErrorCategory::NotFound, false)
        }
        PersistenceError::ApprovalStale => {
            ("approval.stale_request", ErrorCategory::Conflict, false)
        }
        PersistenceError::ApprovalAlreadyResolved => {
            ("approval.already_resolved", ErrorCategory::Conflict, false)
        }
        PersistenceError::ApprovalDecisionNotAllowed => {
            ("approval.decision_not_allowed", ErrorCategory::Permission, false)
        }
        PersistenceError::HostPreparationRevisionConflict => {
            ("sandbox.host_preparation_revision_conflict", ErrorCategory::Conflict, false)
        }
        PersistenceError::HostPreparationOperationNotFound => {
            ("sandbox.host_preparation_not_found", ErrorCategory::NotFound, false)
        }
        PersistenceError::HostPreparationStateConflict => {
            ("sandbox.host_preparation_busy", ErrorCategory::Conflict, false)
        }
        PersistenceError::QuarantineNotFound => {
            ("network.quarantine_not_found", ErrorCategory::NotFound, false)
        }
        PersistenceError::QuarantineReleaseNotApproved => {
            ("permission.approval_required", ErrorCategory::Permission, false)
        }
        PersistenceError::WriterUnavailable | PersistenceError::WriterResponseClosed => {
            ("persistence.unavailable", ErrorCategory::Unavailable, true)
        }
        PersistenceError::WriterBusy { .. } => {
            ("persistence.busy", ErrorCategory::Unavailable, true)
        }
        PersistenceError::WriterUnresponsive { .. } => {
            ("persistence.outcome_unknown", ErrorCategory::Unavailable, false)
        }
        _ => ("persistence.internal", ErrorCategory::Internal, false),
    };
    let mut answer = envelope(code, category, message, retryable, correlation_id);
    if code == crate::command_settlement::PERSISTENCE_OUTCOME_UNKNOWN_CODE {
        answer.details.insert(
            crate::command_settlement::OUTCOME_RECOVERY_DETAIL.to_owned(),
            serde_json::json!(crate::command_settlement::OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY),
        );
    }
    answer
}

fn internal_error(
    error: impl std::fmt::Display,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    envelope("security.internal", ErrorCategory::Internal, error.to_string(), false, correlation_id)
}

fn envelope(
    code: &str,
    category: ErrorCategory,
    message: impl Into<String>,
    retryable: bool,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    IpcErrorEnvelope {
        code: ErrorCode::new(code).expect("static P04 error code is valid"),
        category,
        message: message.into(),
        retryable,
        correlation_id,
        terminal: IpcRequestTerminal::Failed,
        details: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests;

pub(crate) struct ResolvedWorkspaceMutation {
    pub(crate) workspace_id: WorkspaceId,
    pub(crate) session_id: SessionId,
    pub(crate) capabilities: Vec<ResolvedCapability>,
    pub(crate) paths: Vec<WorkspacePath>,
}
