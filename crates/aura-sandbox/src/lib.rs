#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::Read,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, TryRecvError},
    },
    time::{Duration, Instant},
};

use aura_domain_common::{
    CancellationId, ContentHash, ContentHashAlgorithm, ContractVersion, CorrelationId,
    NetworkGrantId, RuntimeInstanceId, SandboxAclJournalId, SandboxExecutionId, SecretNonce,
    SecurityOperationId, UtcTimestamp,
};
use aura_permission_domain::{
    AuraStdioDispatchBootstrapV1, CanonicalExecutableRef, ElevatedHelperChallengeProof,
    ElevatedHelperCommand, ElevatedHelperHello, ElevationLaunchTicket, ExternalIsolationMode,
    ExternalProcessLaunchPlan, ExternalTrustRecord, HostPreparationAction,
    HostPreparationBrokerPlan, HostPreparationBrokerResult, HostPreparationOwnership,
    NativeSandboxMode, NetworkAccessRequirement, NetworkEnforcementKind, NetworkExecutionMode,
    OperationSecurityPlan, PermissionDecision, ProcessOutputLimits, SandboxAclJournalEntry,
    SandboxAclJournalState, SandboxExecutionPlan, SandboxExecutionRecord, SandboxExecutionState,
    SandboxFilesystemAccess, SandboxFilesystemGrant, SandboxHostEvent, SandboxNetworkPolicyError,
    SandboxNetworkPolicyErrorCode, SanitizedEnvironmentEntry,
    ToolConformanceAuraStdioDispatchBootstrapV1, WindowsOsString,
};
use aura_persistence::{PersistenceCoordinator, PersistenceError};
use aura_runtime_contract::ReleaseDescriptor;
use aura_security_contract::CanonicalPath;
use aura_security_contract::TrustState;
use aura_windows_platform::{
    AppContainerProfile, NativeAclAccess, NativeEnvironmentEntry, NativeRestrictedMode,
    NativeSandboxProcess, NativeSecretEnvironmentEntry, capture_file_dacl,
    delete_appcontainer_profile, grant_file_dacl, open_verified_executable,
    probe_host_compatibility, revoke_file_dacl, spawn_restricted_process,
    spawn_restricted_process_with_secret_environment,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

mod host_preparation_guard;

// The write boundary of the Host Preparation broker lives in its own module: the
// guard, the facts it gathers and the resource seam it writes through are one
// responsibility, and this file is not where another one belongs.
use host_preparation_guard::{apply_host_preparation_broker_plan, host_preparation_nul_ready};

enum StdioDispatchBootstrapV1 {
    Tool(AuraStdioDispatchBootstrapV1),
    ToolConformance(ToolConformanceAuraStdioDispatchBootstrapV1),
}

impl StdioDispatchBootstrapV1 {
    const fn tool_call_id(&self) -> aura_domain_common::ToolCallId {
        match self {
            Self::Tool(value) => value.tool_call_id(),
            Self::ToolConformance(value) => value.tool_call_id(),
        }
    }

    const fn tool_execution_attempt_id(&self) -> aura_domain_common::ToolExecutionAttemptId {
        match self {
            Self::Tool(value) => value.tool_execution_attempt_id(),
            Self::ToolConformance(value) => value.tool_execution_attempt_id(),
        }
    }

    const fn output_root(&self) -> &CanonicalPath {
        match self {
            Self::Tool(value) => value.output_root(),
            Self::ToolConformance(value) => value.output_root(),
        }
    }
}

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("sandbox planning requires an allow decision")]
    DecisionNotAllowed,
    #[error("sandbox command was not registered for the security operation")]
    CommandNotRegistered,
    #[error("sandbox mode must be selected explicitly before P04 authorization")]
    SandboxModeNotSelected,
    #[error("sandbox execution plan hash does not match the canonical plan")]
    InvalidPlanHash,
    #[error("host preparation broker plan hash does not match the canonical plan")]
    InvalidHostPreparationPlanHash,
    #[error("host preparation broker plan has expired")]
    HostPreparationPlanExpired,
    #[error("host preparation broker plan nonce is invalid")]
    HostPreparationPlanNonceInvalid,
    #[error("elevation launch ticket is invalid for the frozen Host Preparation plan")]
    ElevationLaunchTicketInvalid,
    #[error("elevation launch ticket has expired")]
    ElevationLaunchTicketExpired,
    #[error("host preparation requires an elevated broker process")]
    HostPreparationRequiresElevation,
    #[error("host preparation broker plan is inconsistent: {0}")]
    InvalidHostPreparationPlan(String),
    #[error("host preparation would delete a verified Aura device capability: {0}")]
    HostPreparationCapabilityConflict(String),
    #[error("elevated helper handshake is invalid")]
    ElevatedHelperHandshakeInvalid,
    #[error("elevated helper bootstrap nonce was already consumed")]
    ElevatedHelperNonceConsumed,
    #[error("elevated helper command does not match the frozen release or plan")]
    ElevatedHelperCommandMismatch,
    #[error("elevated operations must be executed by aura-elevated-helper")]
    ElevationRequiresBroker,
    #[error("sandbox.host_preparation_required")]
    HostPreparationRequired,
    #[error("sandbox path contains an invalid NUL")]
    InvalidWindowsPath,
    #[error("sandbox output limits and timeout must be non-zero")]
    InvalidLimits,
    #[error("external process launch plan is not bound to an external trusted source")]
    ExternalSourceNotTrustBound,
    #[error("external process source is not currently trusted with the frozen fingerprint")]
    ExternalSourceNotTrusted,
    #[error("sandbox executable is a directory")]
    ExecutableIsDirectory,
    #[error("sandbox executable identity changed after the plan was frozen")]
    ExecutableIdentityMismatch,
    #[error("sandbox executable content changed after the plan was frozen")]
    ExecutableHashMismatch,
    #[error("sandbox AppContainer profile already existed before this execution")]
    AppContainerProfileAlreadyExists,
    #[error("sandbox network policy rejected the frozen execution plan: {0:?}")]
    NetworkPolicy(Box<SandboxNetworkPolicyError>),
    #[error("sandbox network enforcement does not match the selected mode and declaration")]
    NetworkEnforcementMismatch,
    #[error("Windows sandbox operation failed: {0}")]
    Platform(#[from] std::io::Error),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error("sandbox execution cleanup failed: {0}")]
    CleanupFailed(String),
}

impl SandboxError {
    /// The wire code for a refusal this process can *prove* precedes every
    /// resource write, if it is one.
    ///
    /// Only errors raised before the first write qualify. The elevated helper
    /// reports these as a failed terminal result so Core can tell "refused, nothing
    /// written" from "we do not know what happened"; every other failure keeps the
    /// unknown-state reporting it already had.
    #[must_use]
    pub fn pre_write_refusal_code(&self) -> Option<&'static str> {
        match self {
            Self::HostPreparationCapabilityConflict(_) => {
                Some("sandbox.host_preparation_capability_conflict")
            }
            Self::InvalidHostPreparationPlan(_) => Some("sandbox.host_preparation_plan_invalid"),
            _ => None,
        }
    }
}

pub struct ExternalManagedProcess {
    isolation: ExternalIsolationMode,
    network_enforcement: NetworkEnforcementKind,
    process: Option<NativeSandboxProcess>,
    profile_name: Option<String>,
}

impl ExternalManagedProcess {
    #[must_use]
    pub const fn isolation(&self) -> ExternalIsolationMode {
        self.isolation
    }

    #[must_use]
    pub const fn network_enforcement(&self) -> NetworkEnforcementKind {
        self.network_enforcement
    }

    #[must_use]
    pub fn process_id(&self) -> Option<u32> {
        self.process.as_ref().map(NativeSandboxProcess::process_id)
    }

    pub fn token_is_appcontainer(&self) -> Result<bool, SandboxError> {
        self.process
            .as_ref()
            .ok_or_else(|| SandboxError::CleanupFailed("external process is closed".to_owned()))?
            .token_is_appcontainer()
            .map_err(SandboxError::Platform)
    }

    pub fn token_is_elevated(&self) -> Result<bool, SandboxError> {
        self.process
            .as_ref()
            .ok_or_else(|| SandboxError::CleanupFailed("external process is closed".to_owned()))?
            .token_is_elevated()
            .map_err(SandboxError::Platform)
    }

    pub fn terminate_tree(&self, exit_code: u32) -> Result<(), SandboxError> {
        self.process
            .as_ref()
            .ok_or_else(|| SandboxError::CleanupFailed("external process is closed".to_owned()))?
            .terminate(exit_code)
            .map_err(SandboxError::Platform)
    }

    pub fn wait_timeout(&self, timeout: Duration) -> Result<Option<u32>, SandboxError> {
        self.process
            .as_ref()
            .ok_or_else(|| SandboxError::CleanupFailed("external process is closed".to_owned()))?
            .wait_timeout(timeout)
            .map_err(SandboxError::Platform)
    }

    pub fn wait_for_process_tree_exit(&self, timeout: Duration) -> Result<bool, SandboxError> {
        self.process
            .as_ref()
            .ok_or_else(|| SandboxError::CleanupFailed("external process is closed".to_owned()))?
            .wait_for_process_tree_exit(timeout)
            .map_err(SandboxError::Platform)
    }

    pub fn close(mut self) -> Result<(), SandboxError> {
        self.close_inner()
    }

    fn close_inner(&mut self) -> Result<(), SandboxError> {
        if let Some(process) = self.process.take() {
            if !process.wait_for_process_tree_exit(Duration::ZERO)? {
                process.terminate(0xC000_013A)?;
                let _ = process.wait_for_process_tree_exit(Duration::from_secs(2))?;
            }
            drop(process);
        }
        if let Some(profile_name) = self.profile_name.take() {
            delete_appcontainer_profile(&profile_name)?;
        }
        Ok(())
    }
}

impl Drop for ExternalManagedProcess {
    fn drop(&mut self) {
        let _ = self.close_inner();
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SandboxCommandDescriptor {
    pub executable: CanonicalExecutableRef,
    pub args: Vec<WindowsOsString>,
    pub working_directory: CanonicalPath,
    pub environment: Vec<SanitizedEnvironmentEntry>,
    pub filesystem_grants: Vec<SandboxFilesystemGrant>,
    pub network_grant_ids: Vec<NetworkGrantId>,
    pub timeout_ms: u64,
    pub output_limits: ProcessOutputLimits,
    pub cancellation_id: CancellationId,
}

pub trait SandboxOperationResolver: Send + Sync {
    fn command_for(&self, operation_id: SecurityOperationId) -> Option<SandboxCommandDescriptor>;
}

pub trait SandboxPlanner {
    fn plan(
        &self,
        operation: &OperationSecurityPlan,
        decision: &PermissionDecision,
    ) -> Result<SandboxExecutionPlan, SandboxError>;
}

pub struct DefaultSandboxPlanner<R> {
    resolver: R,
}

impl<R> DefaultSandboxPlanner<R> {
    #[must_use]
    pub const fn new(resolver: R) -> Self {
        Self { resolver }
    }
}

impl<R: SandboxOperationResolver> SandboxPlanner for DefaultSandboxPlanner<R> {
    fn plan(
        &self,
        operation: &OperationSecurityPlan,
        decision: &PermissionDecision,
    ) -> Result<SandboxExecutionPlan, SandboxError> {
        let PermissionDecision::Allow { snapshot } = decision else {
            return Err(SandboxError::DecisionNotAllowed);
        };
        let command = self
            .resolver
            .command_for(operation.operation_id)
            .ok_or(SandboxError::CommandNotRegistered)?;
        if command.timeout_ms == 0
            || command.output_limits.stdout_bytes == 0
            || command.output_limits.stderr_bytes == 0
            || command.output_limits.combined_bytes == 0
        {
            return Err(SandboxError::InvalidLimits);
        }
        let mode = operation.requested_sandbox.ok_or(SandboxError::SandboxModeNotSelected)?;
        let mut plan = SandboxExecutionPlan {
            execution_id: SandboxExecutionId::new(),
            identity: operation.identity.clone(),
            permission_snapshot_id: snapshot.id,
            mode,
            network_access: operation.network_access,
            network_enforcement: NetworkEnforcementKind::AppContainerNetworkDenied,
            executable: command.executable,
            args: command.args,
            working_directory: command.working_directory,
            environment: command.environment,
            filesystem_grants: command.filesystem_grants,
            network_grant_ids: command.network_grant_ids,
            timeout_ms: command.timeout_ms,
            output_limits: command.output_limits,
            cancellation_id: command.cancellation_id,
            plan_hash: ContentHash::blake3(b"uninitialized"),
        };
        plan.network_enforcement =
            validate_native_network_policy(plan.mode, plan.network_access, &plan.network_grant_ids)
                .map_err(|error| SandboxError::NetworkPolicy(Box::new(error)))?;
        plan.plan_hash = plan_hash(&plan)?;
        Ok(plan)
    }
}

#[must_use]
fn native_network_policy_error(
    code: SandboxNetworkPolicyErrorCode,
    declared_access: NetworkAccessRequirement,
    mode: NativeSandboxMode,
) -> SandboxNetworkPolicyError {
    SandboxNetworkPolicyError {
        code,
        declared_access,
        selected_mode: NetworkExecutionMode::Native { mode },
        required_declaration: NetworkAccessRequirement::Required,
        allowed_networked_modes: vec![NetworkExecutionMode::Native {
            mode: NativeSandboxMode::ObservedNativeNetworked,
        }],
    }
}

pub fn validate_native_network_policy(
    mode: NativeSandboxMode,
    access: NetworkAccessRequirement,
    grant_ids: &[NetworkGrantId],
) -> Result<NetworkEnforcementKind, SandboxNetworkPolicyError> {
    match mode {
        NativeSandboxMode::CompatibilityAppContainer | NativeSandboxMode::StrictAppContainer => {
            if access == NetworkAccessRequirement::Required {
                return Err(native_network_policy_error(
                    SandboxNetworkPolicyErrorCode::AppContainerNetworkDenied,
                    access,
                    mode,
                ));
            }
            if !grant_ids.is_empty() {
                return Err(native_network_policy_error(
                    SandboxNetworkPolicyErrorCode::NetworkAccessDeclarationRequired,
                    access,
                    mode,
                ));
            }
            Ok(NetworkEnforcementKind::AppContainerNetworkDenied)
        }
        NativeSandboxMode::ObservedNative => {
            if access == NetworkAccessRequirement::Required || !grant_ids.is_empty() {
                return Err(native_network_policy_error(
                    SandboxNetworkPolicyErrorCode::NetworkAccessDeclarationRequired,
                    access,
                    mode,
                ));
            }
            Ok(NetworkEnforcementKind::ObservedNativeNotProxyEnforced)
        }
        NativeSandboxMode::ObservedNativeNetworked => {
            if !grant_ids.is_empty() {
                return Err(native_network_policy_error(
                    SandboxNetworkPolicyErrorCode::NetworkGrantUnavailableInSelectedMode,
                    access,
                    mode,
                ));
            }
            if access != NetworkAccessRequirement::Required {
                return Err(native_network_policy_error(
                    SandboxNetworkPolicyErrorCode::NetworkAccessDeclarationRequired,
                    access,
                    mode,
                ));
            }
            Ok(NetworkEnforcementKind::ObservedNativeNotProxyEnforced)
        }
        NativeSandboxMode::ElevatedBrokered => {
            if access == NetworkAccessRequirement::Required || !grant_ids.is_empty() {
                return Err(native_network_policy_error(
                    SandboxNetworkPolicyErrorCode::NetworkGrantUnavailableInSelectedMode,
                    access,
                    mode,
                ));
            }
            Ok(NetworkEnforcementKind::ObservedNativeNotProxyEnforced)
        }
    }
}

#[must_use]
fn external_network_policy_error(
    code: SandboxNetworkPolicyErrorCode,
    declared_access: NetworkAccessRequirement,
    mode: ExternalIsolationMode,
) -> SandboxNetworkPolicyError {
    SandboxNetworkPolicyError {
        code,
        declared_access,
        selected_mode: NetworkExecutionMode::External { mode },
        required_declaration: NetworkAccessRequirement::Required,
        allowed_networked_modes: vec![NetworkExecutionMode::External {
            mode: ExternalIsolationMode::ManagedUserNetworked,
        }],
    }
}

pub fn validate_external_network_policy(
    mode: ExternalIsolationMode,
    access: NetworkAccessRequirement,
) -> Result<NetworkEnforcementKind, SandboxNetworkPolicyError> {
    match mode {
        ExternalIsolationMode::StrictAppContainer => {
            if access == NetworkAccessRequirement::Required {
                return Err(external_network_policy_error(
                    SandboxNetworkPolicyErrorCode::AppContainerNetworkDenied,
                    access,
                    mode,
                ));
            }
            Ok(NetworkEnforcementKind::AppContainerNetworkDenied)
        }
        ExternalIsolationMode::CompatibilityManagedUser
        | ExternalIsolationMode::NativeManagedUser => {
            if access == NetworkAccessRequirement::Required {
                return Err(external_network_policy_error(
                    SandboxNetworkPolicyErrorCode::NetworkAccessDeclarationRequired,
                    access,
                    mode,
                ));
            }
            Ok(NetworkEnforcementKind::ObservedNativeNotProxyEnforced)
        }
        ExternalIsolationMode::ManagedUserNetworked => {
            if access != NetworkAccessRequirement::Required {
                return Err(external_network_policy_error(
                    SandboxNetworkPolicyErrorCode::NetworkAccessDeclarationRequired,
                    access,
                    mode,
                ));
            }
            Ok(NetworkEnforcementKind::ObservedNativeNotProxyEnforced)
        }
    }
}

#[derive(Clone, Default)]
pub struct InMemorySandboxOperationResolver {
    commands: Arc<Mutex<BTreeMap<SecurityOperationId, SandboxCommandDescriptor>>>,
}

impl InMemorySandboxOperationResolver {
    pub fn register(
        &self,
        operation_id: SecurityOperationId,
        command: SandboxCommandDescriptor,
    ) -> Result<(), SandboxError> {
        let mut commands = self.commands.lock().map_err(|_| {
            SandboxError::CleanupFailed("command registry lock poisoned".to_owned())
        })?;
        if let Some(existing) = commands.get(&operation_id) {
            if existing == &command {
                return Ok(());
            }
            return Err(SandboxError::CleanupFailed(
                "sandbox command descriptor is immutable once registered".to_owned(),
            ));
        }
        commands.insert(operation_id, command);
        Ok(())
    }
}

impl SandboxOperationResolver for InMemorySandboxOperationResolver {
    fn command_for(&self, operation_id: SecurityOperationId) -> Option<SandboxCommandDescriptor> {
        self.commands.lock().ok()?.get(&operation_id).cloned()
    }
}

pub fn plan_hash(plan: &SandboxExecutionPlan) -> Result<ContentHash, SandboxError> {
    #[derive(Serialize)]
    struct HashablePlan<'a> {
        execution_id: SandboxExecutionId,
        identity: &'a aura_security_contract::InvocationIdentity,
        permission_snapshot_id: aura_domain_common::PermissionSnapshotId,
        mode: NativeSandboxMode,
        network_access: NetworkAccessRequirement,
        network_enforcement: NetworkEnforcementKind,
        executable: &'a CanonicalExecutableRef,
        args: &'a [WindowsOsString],
        working_directory: &'a CanonicalPath,
        environment: &'a [SanitizedEnvironmentEntry],
        filesystem_grants: &'a [SandboxFilesystemGrant],
        network_grant_ids: &'a [NetworkGrantId],
        timeout_ms: u64,
        output_limits: &'a ProcessOutputLimits,
        cancellation_id: CancellationId,
    }
    let hashable = HashablePlan {
        execution_id: plan.execution_id,
        identity: &plan.identity,
        permission_snapshot_id: plan.permission_snapshot_id,
        mode: plan.mode,
        network_access: plan.network_access,
        network_enforcement: plan.network_enforcement,
        executable: &plan.executable,
        args: &plan.args,
        working_directory: &plan.working_directory,
        environment: &plan.environment,
        filesystem_grants: &plan.filesystem_grants,
        network_grant_ids: &plan.network_grant_ids,
        timeout_ms: plan.timeout_ms,
        output_limits: &plan.output_limits,
        cancellation_id: plan.cancellation_id,
    };
    let bytes = serde_json::to_vec(&hashable)
        .map_err(|error| SandboxError::CleanupFailed(error.to_string()))?;
    Ok(ContentHash::blake3(&bytes))
}

pub fn external_process_plan_hash(
    plan: &ExternalProcessLaunchPlan,
) -> Result<ContentHash, SandboxError> {
    #[derive(Serialize)]
    struct HashableExternalPlan<'a> {
        execution_id: SandboxExecutionId,
        identity: &'a aura_security_contract::InvocationIdentity,
        permission_snapshot_id: aura_domain_common::PermissionSnapshotId,
        source: &'a aura_permission_domain::SecuritySource,
        source_fingerprint: &'a ContentHash,
        isolation: ExternalIsolationMode,
        network_access: NetworkAccessRequirement,
        network_enforcement: NetworkEnforcementKind,
        executable: &'a CanonicalExecutableRef,
        args: &'a [WindowsOsString],
        working_directory: &'a CanonicalPath,
        environment: &'a [SanitizedEnvironmentEntry],
        timeout_ms: u64,
        output_limits: &'a ProcessOutputLimits,
        cancellation_id: CancellationId,
    }
    let hashable = HashableExternalPlan {
        execution_id: plan.execution_id,
        identity: &plan.identity,
        permission_snapshot_id: plan.permission_snapshot_id,
        source: &plan.source,
        source_fingerprint: &plan.source_fingerprint,
        isolation: plan.isolation,
        network_access: plan.network_access,
        network_enforcement: plan.network_enforcement,
        executable: &plan.executable,
        args: &plan.args,
        working_directory: &plan.working_directory,
        environment: &plan.environment,
        timeout_ms: plan.timeout_ms,
        output_limits: &plan.output_limits,
        cancellation_id: plan.cancellation_id,
    };
    let bytes = serde_json::to_vec(&hashable)
        .map_err(|error| SandboxError::CleanupFailed(error.to_string()))?;
    Ok(ContentHash::blake3(&bytes))
}

/// Captures the immutable executable identity used by both native and trusted
/// external launch plans. The execution path opens the file again under a
/// no-write/no-delete share lock and compares both identity and content hash.
pub fn snapshot_executable_ref(
    path: &Path,
    algorithm: ContentHashAlgorithm,
) -> Result<CanonicalExecutableRef, SandboxError> {
    let verified = open_verified_executable(path)?;
    if verified.info().is_directory {
        return Err(SandboxError::ExecutableIsDirectory);
    }
    let content_hash = hash_verified_executable(verified.file(), algorithm)?;
    let final_path = &verified.info().final_path;
    Ok(CanonicalExecutableRef {
        path: CanonicalPath {
            display: final_path.display().to_string(),
            utf16: final_path.as_os_str().encode_wide().collect(),
        },
        identity: aura_domain_common::FileIdentity {
            volume_serial_number: verified.info().identity.volume_serial_number.to_string(),
            file_id: verified
                .info()
                .identity
                .file_id
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        },
        content_hash,
    })
}

#[cfg(test)]
fn launch_external_process(
    plan: &ExternalProcessLaunchPlan,
    trust: &ExternalTrustRecord,
) -> Result<ExternalManagedProcess, SandboxError> {
    let network_enforcement = validate_external_process_plan(plan, trust)?;

    let executable = verify_executable_ref(&plan.executable)?;
    let executable_path = executable.info().final_path.clone();
    let working_directory = path_from_canonical(&plan.working_directory)?;
    let arguments = plan.args.iter().map(|arg| arg.utf16.clone()).collect::<Vec<_>>();
    let environment = plan
        .environment
        .iter()
        .map(|entry| NativeEnvironmentEntry {
            name: entry.name.clone(),
            value_utf16: entry.value.utf16.clone(),
        })
        .collect::<Vec<_>>();

    let (native_mode, profile, profile_name) = match plan.isolation {
        ExternalIsolationMode::CompatibilityManagedUser
        | ExternalIsolationMode::NativeManagedUser
        | ExternalIsolationMode::ManagedUserNetworked => {
            (NativeRestrictedMode::StandardUser, None, None)
        }
        ExternalIsolationMode::StrictAppContainer => {
            let host = probe_host_compatibility()?;
            if !host.system_drive_ready || !host_preparation_nul_ready() {
                return Err(SandboxError::HostPreparationRequired);
            }
            let profile_name = format!("Aura.External.{}", plan.execution_id);
            let profile = create_fresh_appcontainer_profile(&profile_name)?;
            (NativeRestrictedMode::AppContainer, Some(profile), Some(profile_name))
        }
    };

    let launched = spawn_restricted_process(
        native_mode,
        profile.as_ref(),
        &executable_path,
        &arguments,
        &working_directory,
        &environment,
    );
    drop(executable);
    let process = match launched {
        Ok(process) => process,
        Err(error) => {
            if let Some(profile_name) = profile_name.as_deref() {
                let _ = delete_appcontainer_profile(profile_name);
            }
            return Err(SandboxError::Platform(error));
        }
    };

    Ok(ExternalManagedProcess {
        isolation: plan.isolation,
        network_enforcement,
        process: Some(process),
        profile_name,
    })
}

pub fn validate_external_process_plan(
    plan: &ExternalProcessLaunchPlan,
    trust: &ExternalTrustRecord,
) -> Result<NetworkEnforcementKind, SandboxError> {
    if !plan.source.security_source_ref().requires_trust() {
        return Err(SandboxError::ExternalSourceNotTrustBound);
    }
    let trusted_fingerprint = match &trust.state {
        TrustState::Trusted { fingerprint, .. } => fingerprint,
        TrustState::SystemTrusted | TrustState::Untrusted | TrustState::Revoked => {
            return Err(SandboxError::ExternalSourceNotTrusted);
        }
    };
    if trust.source != plan.source
        || trust.effective_fingerprint.as_ref() != Some(&plan.source_fingerprint)
        || trusted_fingerprint != &plan.source_fingerprint
    {
        return Err(SandboxError::ExternalSourceNotTrusted);
    }
    if external_process_plan_hash(plan)? != plan.plan_hash {
        return Err(SandboxError::InvalidPlanHash);
    }
    let network_enforcement = validate_external_network_policy(plan.isolation, plan.network_access)
        .map_err(|error| SandboxError::NetworkPolicy(Box::new(error)))?;
    if network_enforcement != plan.network_enforcement {
        return Err(SandboxError::NetworkEnforcementMismatch);
    }
    if plan.timeout_ms == 0
        || plan.output_limits.stdout_bytes == 0
        || plan.output_limits.stderr_bytes == 0
        || plan.output_limits.combined_bytes == 0
    {
        return Err(SandboxError::InvalidLimits);
    }
    Ok(network_enforcement)
}

pub fn sandbox_plan_for_external_process(
    plan: &ExternalProcessLaunchPlan,
    trust: &ExternalTrustRecord,
) -> Result<SandboxExecutionPlan, SandboxError> {
    let network_enforcement = validate_external_process_plan(plan, trust)?;
    let mode = match plan.isolation {
        ExternalIsolationMode::CompatibilityManagedUser
        | ExternalIsolationMode::NativeManagedUser => NativeSandboxMode::ObservedNative,
        ExternalIsolationMode::StrictAppContainer => NativeSandboxMode::StrictAppContainer,
        ExternalIsolationMode::ManagedUserNetworked => NativeSandboxMode::ObservedNativeNetworked,
    };
    let mut sandbox = SandboxExecutionPlan {
        execution_id: plan.execution_id,
        identity: plan.identity.clone(),
        permission_snapshot_id: plan.permission_snapshot_id,
        mode,
        network_access: plan.network_access,
        network_enforcement,
        executable: plan.executable.clone(),
        args: plan.args.clone(),
        working_directory: plan.working_directory.clone(),
        environment: plan.environment.clone(),
        filesystem_grants: Vec::new(),
        network_grant_ids: Vec::new(),
        timeout_ms: plan.timeout_ms,
        output_limits: plan.output_limits.clone(),
        cancellation_id: plan.cancellation_id,
        plan_hash: ContentHash::blake3(b"uninitialized"),
    };
    sandbox.plan_hash = plan_hash(&sandbox)?;
    Ok(sandbox)
}

fn verify_executable_ref(
    expected: &CanonicalExecutableRef,
) -> Result<aura_windows_platform::VerifiedFile, SandboxError> {
    let path = path_from_canonical(&expected.path)?;
    let verified = open_verified_executable(&path)?;
    if verified.info().is_directory {
        return Err(SandboxError::ExecutableIsDirectory);
    }
    let identity = &verified.info().identity;
    let file_id = identity.file_id.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    if expected.identity.volume_serial_number != identity.volume_serial_number.to_string()
        || expected.identity.file_id != file_id
    {
        return Err(SandboxError::ExecutableIdentityMismatch);
    }
    if hash_verified_executable(verified.file(), expected.content_hash.algorithm)?
        != expected.content_hash
    {
        return Err(SandboxError::ExecutableHashMismatch);
    }
    Ok(verified)
}

fn create_fresh_appcontainer_profile(name: &str) -> Result<AppContainerProfile, SandboxError> {
    let profile = AppContainerProfile::ensure(name)?;
    if !profile.was_created() {
        return Err(SandboxError::AppContainerProfileAlreadyExists);
    }
    Ok(profile)
}

fn hash_verified_executable(
    file: &std::fs::File,
    algorithm: ContentHashAlgorithm,
) -> Result<ContentHash, SandboxError> {
    let mut reader = file;
    let mut buffer = [0_u8; 64 * 1024];
    let hex = match algorithm {
        ContentHashAlgorithm::Blake3 => {
            let mut hasher = blake3::Hasher::new();
            loop {
                let read = reader.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            hasher.finalize().to_hex().to_string()
        }
        ContentHashAlgorithm::Sha256 => {
            let mut hasher = Sha256::new();
            loop {
                let read = reader.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            format!("{:x}", hasher.finalize())
        }
    };
    ContentHash::new(algorithm, hex).map_err(|error| SandboxError::CleanupFailed(error.to_string()))
}

pub fn host_preparation_plan_hash(
    plan: &HostPreparationBrokerPlan,
) -> Result<ContentHash, SandboxError> {
    #[derive(Serialize)]
    struct HashableHostPreparationPlan<'a> {
        operation_id: aura_domain_common::HostPreparationOperationId,
        action: HostPreparationAction,
        host_preparation_revision: u64,
        runtime_instance_id: RuntimeInstanceId,
        release_descriptor: &'a ReleaseDescriptor,
        plan_nonce_hash: &'a ContentHash,
        expires_at: UtcTimestamp,
        system_drive_root: &'a CanonicalPath,
        ownership_before: &'a HostPreparationOwnership,
        add_all_application_packages_ace: bool,
        add_all_restricted_application_packages_ace: bool,
        update_null_device_security: bool,
    }
    let hashable = HashableHostPreparationPlan {
        operation_id: plan.operation_id,
        action: plan.action,
        host_preparation_revision: plan.host_preparation_revision,
        runtime_instance_id: plan.runtime_instance_id,
        release_descriptor: &plan.release_descriptor,
        plan_nonce_hash: &plan.plan_nonce_hash,
        expires_at: plan.expires_at,
        system_drive_root: &plan.system_drive_root,
        ownership_before: &plan.ownership_before,
        add_all_application_packages_ace: plan.add_all_application_packages_ace,
        add_all_restricted_application_packages_ace: plan
            .add_all_restricted_application_packages_ace,
        update_null_device_security: plan.update_null_device_security,
    };
    let bytes = serde_json::to_vec(&hashable)
        .map_err(|error| SandboxError::CleanupFailed(error.to_string()))?;
    Ok(ContentHash::blake3(&bytes))
}

pub fn execute_host_preparation_broker_plan(
    plan: &HostPreparationBrokerPlan,
) -> Result<HostPreparationBrokerResult, SandboxError> {
    if host_preparation_plan_hash(plan)? != plan.plan_hash {
        return Err(SandboxError::InvalidHostPreparationPlanHash);
    }
    if UtcTimestamp::now() > plan.expires_at {
        return Err(SandboxError::HostPreparationPlanExpired);
    }
    apply_host_preparation_broker_plan(plan)
}

pub fn execute_approved_host_preparation_broker_plan(
    plan: &HostPreparationBrokerPlan,
    ticket: &ElevationLaunchTicket,
) -> Result<HostPreparationBrokerResult, SandboxError> {
    if host_preparation_plan_hash(plan)? != plan.plan_hash {
        return Err(SandboxError::InvalidHostPreparationPlanHash);
    }
    validate_elevation_launch_ticket(plan, ticket)?;
    apply_host_preparation_broker_plan(plan)
}

pub struct ElevatedBrokerGuard {
    expected_nonce: Option<SecretNonce>,
    expected_parent_process_id: u32,
    expected_protocol_version: ContractVersion,
    expected_parent_runtime_instance_id: RuntimeInstanceId,
    expected_helper_runtime_instance_id: RuntimeInstanceId,
}

impl ElevatedBrokerGuard {
    #[must_use]
    pub const fn new(
        expected_nonce: SecretNonce,
        expected_parent_process_id: u32,
        expected_protocol_version: ContractVersion,
        expected_parent_runtime_instance_id: RuntimeInstanceId,
        expected_helper_runtime_instance_id: RuntimeInstanceId,
    ) -> Self {
        Self {
            expected_nonce: Some(expected_nonce),
            expected_parent_process_id,
            expected_protocol_version,
            expected_parent_runtime_instance_id,
            expected_helper_runtime_instance_id,
        }
    }

    pub fn accept_hello(&mut self, hello: &ElevatedHelperHello) -> Result<(), SandboxError> {
        if hello.protocol_version != self.expected_protocol_version
            || hello.parent_process_id != self.expected_parent_process_id
            || hello.helper_process_id == 0
            || hello.parent_runtime_instance_id != self.expected_parent_runtime_instance_id
            || hello.helper_runtime_instance_id != self.expected_helper_runtime_instance_id
        {
            return Err(SandboxError::ElevatedHelperHandshakeInvalid);
        }
        Ok(())
    }

    pub fn accept_challenge_proof(
        &mut self,
        proof: &ElevatedHelperChallengeProof,
    ) -> Result<(), SandboxError> {
        let expected =
            self.expected_nonce.as_ref().ok_or(SandboxError::ElevatedHelperNonceConsumed)?;
        if !constant_time_equal(
            expected.expose().as_bytes(),
            proof.bootstrap_nonce.expose().as_bytes(),
        ) {
            return Err(SandboxError::ElevatedHelperHandshakeInvalid);
        }
        self.expected_nonce.take();
        Ok(())
    }
}

pub fn validate_elevated_helper_command(
    command: &ElevatedHelperCommand,
    expected_release_descriptor: &ReleaseDescriptor,
    expected_runtime_instance_id: RuntimeInstanceId,
) -> Result<(), SandboxError> {
    if command.protocol_version.major != ContractVersion::CURRENT.major
        || command.protocol_version.minor > ContractVersion::CURRENT.minor
        || &command.release_descriptor != expected_release_descriptor
        || command.runtime_instance_id != expected_runtime_instance_id
        || &command.plan.release_descriptor != expected_release_descriptor
        || command.plan.release_descriptor.identity.component_manifest_hash.algorithm
            != ContentHashAlgorithm::Sha256
        || command.plan.runtime_instance_id != expected_runtime_instance_id
        || host_preparation_plan_hash(&command.plan)? != command.plan.plan_hash
    {
        return Err(SandboxError::ElevatedHelperCommandMismatch);
    }
    validate_host_preparation_plan_nonce(&command.plan, &command.plan_nonce)?;
    validate_elevation_launch_ticket(&command.plan, &command.elevation_launch_ticket)?;
    Ok(())
}

pub fn validate_elevation_launch_ticket(
    plan: &HostPreparationBrokerPlan,
    ticket: &ElevationLaunchTicket,
) -> Result<(), SandboxError> {
    let nonce = ticket.ticket_nonce.expose();
    if ticket.operation_id != plan.operation_id
        || ticket.plan_hash != plan.plan_hash
        || ticket.runtime_instance_id != plan.runtime_instance_id
        || ticket.release_descriptor != plan.release_descriptor
        || ticket.issued_at > plan.expires_at
        || ticket.expires_at <= ticket.issued_at
        || ticket.expires_at.value() > ticket.issued_at.value() + time::Duration::seconds(60)
        || nonce.len() != 64
        || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(SandboxError::ElevationLaunchTicketInvalid);
    }
    if UtcTimestamp::now() > ticket.expires_at {
        return Err(SandboxError::ElevationLaunchTicketExpired);
    }
    Ok(())
}

pub fn validate_host_preparation_plan_nonce(
    plan: &HostPreparationBrokerPlan,
    nonce: &SecretNonce,
) -> Result<(), SandboxError> {
    let value = nonce.expose();
    if value.len() != 64
        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
        || ContentHash::blake3(value.as_bytes()) != plan.plan_nonce_hash
    {
        return Err(SandboxError::HostPreparationPlanNonceInvalid);
    }
    Ok(())
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter().zip(right).fold(0_u8, |difference, (left, right)| difference | (left ^ right)) == 0
}

#[derive(Clone)]
pub struct CancellationRegistry {
    flags: Arc<Mutex<BTreeMap<CancellationId, Arc<AtomicBool>>>>,
}

impl Default for CancellationRegistry {
    fn default() -> Self {
        Self { flags: Arc::new(Mutex::new(BTreeMap::new())) }
    }
}

impl CancellationRegistry {
    pub fn register(&self, id: CancellationId) -> Result<Arc<AtomicBool>, SandboxError> {
        let flag = Arc::new(AtomicBool::new(false));
        self.flags
            .lock()
            .map_err(|_| SandboxError::CleanupFailed("cancellation registry poisoned".to_owned()))?
            .insert(id, Arc::clone(&flag));
        Ok(flag)
    }

    pub fn cancel(&self, id: CancellationId) -> Result<bool, SandboxError> {
        let flags = self.flags.lock().map_err(|_| {
            SandboxError::CleanupFailed("cancellation registry poisoned".to_owned())
        })?;
        if let Some(flag) = flags.get(&id) {
            flag.store(true, Ordering::Release);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn remove(&self, id: CancellationId) {
        if let Ok(mut flags) = self.flags.lock() {
            flags.remove(&id);
        }
    }
}

#[derive(Clone, Debug)]
pub struct SandboxExecutorOptions {
    pub appcontainer_profile_prefix: String,
    pub poll_interval: Duration,
}

impl Default for SandboxExecutorOptions {
    fn default() -> Self {
        Self {
            appcontainer_profile_prefix: "Aura.Sandbox.P04".to_owned(),
            poll_interval: Duration::from_millis(25),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SandboxExecutionOutcome {
    pub execution_id: SandboxExecutionId,
    pub state: SandboxExecutionState,
    pub process_id: Option<u32>,
    pub exit_code: Option<u32>,
    pub network_enforcement: NetworkEnforcementKind,
}

#[derive(Default)]
struct OutputTracker {
    stdout_bytes: u64,
    stderr_bytes: u64,
    combined_bytes: u64,
    stdout_sequence: u64,
    stderr_sequence: u64,
}

#[derive(Clone, Copy)]
struct OutputDrainResult {
    made_progress: bool,
    limit_exceeded: bool,
}

/// In-process Host-to-execution input. It is never serialized or persisted;
/// the Sandbox Host places only identity-bound Aura Stdio bytes on this
/// channel.
pub enum SandboxProcessInputV1 {
    Write(Vec<u8>),
    Close,
}

#[derive(Clone)]
pub struct SandboxExecutor {
    persistence: PersistenceCoordinator,
    cancellation: CancellationRegistry,
    options: SandboxExecutorOptions,
}

impl SandboxExecutor {
    #[must_use]
    pub fn new(
        persistence: PersistenceCoordinator,
        cancellation: CancellationRegistry,
        options: SandboxExecutorOptions,
    ) -> Self {
        Self { persistence, cancellation, options }
    }

    pub fn execute<F>(
        &self,
        plan: SandboxExecutionPlan,
        emit: F,
    ) -> Result<SandboxExecutionOutcome, SandboxError>
    where
        F: FnMut(SandboxHostEvent),
    {
        self.execute_with_secret_environment(plan, None, emit)
    }

    /// Executes an already-authorized plan while injecting one post-Allow
    /// Credential environment value only into the native launch. The value is
    /// absent from the persisted Sandbox plan, events, and hashes.
    pub fn execute_with_secret_environment<F>(
        &self,
        plan: SandboxExecutionPlan,
        secret_environment: Option<NativeSecretEnvironmentEntry>,
        emit: F,
    ) -> Result<SandboxExecutionOutcome, SandboxError>
    where
        F: FnMut(SandboxHostEvent),
    {
        self.execute_with_runtime_io(plan, None, secret_environment, None, emit)
    }

    pub fn execute_with_runtime_io<F>(
        &self,
        plan: SandboxExecutionPlan,
        stdio_bootstrap: Option<AuraStdioDispatchBootstrapV1>,
        secret_environment: Option<NativeSecretEnvironmentEntry>,
        stdin_commands: Option<Receiver<SandboxProcessInputV1>>,
        emit: F,
    ) -> Result<SandboxExecutionOutcome, SandboxError>
    where
        F: FnMut(SandboxHostEvent),
    {
        self.execute_with_runtime_io_inner(
            plan,
            stdio_bootstrap.map(StdioDispatchBootstrapV1::Tool),
            secret_environment,
            stdin_commands,
            emit,
        )
    }

    /// W3 counterpart to `execute_with_runtime_io`. The bootstrap carries
    /// only Conformance-owned `tcl_/tea_` identities and cannot be converted
    /// into an installed Tool dispatch binding.
    pub fn execute_with_conformance_runtime_io<F>(
        &self,
        plan: SandboxExecutionPlan,
        stdio_bootstrap: Option<ToolConformanceAuraStdioDispatchBootstrapV1>,
        secret_environment: Option<NativeSecretEnvironmentEntry>,
        stdin_commands: Option<Receiver<SandboxProcessInputV1>>,
        emit: F,
    ) -> Result<SandboxExecutionOutcome, SandboxError>
    where
        F: FnMut(SandboxHostEvent),
    {
        self.execute_with_runtime_io_inner(
            plan,
            stdio_bootstrap.map(StdioDispatchBootstrapV1::ToolConformance),
            secret_environment,
            stdin_commands,
            emit,
        )
    }

    fn execute_with_runtime_io_inner<F>(
        &self,
        plan: SandboxExecutionPlan,
        stdio_bootstrap: Option<StdioDispatchBootstrapV1>,
        secret_environment: Option<NativeSecretEnvironmentEntry>,
        stdin_commands: Option<Receiver<SandboxProcessInputV1>>,
        mut emit: F,
    ) -> Result<SandboxExecutionOutcome, SandboxError>
    where
        F: FnMut(SandboxHostEvent),
    {
        if plan_hash(&plan)? != plan.plan_hash {
            return Err(SandboxError::InvalidPlanHash);
        }
        let network_enforcement =
            validate_native_network_policy(plan.mode, plan.network_access, &plan.network_grant_ids)
                .map_err(|error| SandboxError::NetworkPolicy(Box::new(error)))?;
        if network_enforcement != plan.network_enforcement {
            return Err(SandboxError::NetworkEnforcementMismatch);
        }
        if plan.mode == NativeSandboxMode::ElevatedBrokered {
            return Err(SandboxError::ElevationRequiresBroker);
        }
        if matches!(
            plan.mode,
            NativeSandboxMode::CompatibilityAppContainer | NativeSandboxMode::StrictAppContainer
        ) {
            let host = probe_host_compatibility()?;
            if !host.system_drive_ready || !host_preparation_nul_ready() {
                return Err(SandboxError::HostPreparationRequired);
            }
        }
        let now = UtcTimestamp::now();
        let correlation_id = plan.identity.correlation_id;
        self.persistence.create_sandbox_execution(
            SandboxExecutionRecord {
                plan: plan.clone(),
                state: SandboxExecutionState::Planned,
                process_id: None,
                exit_code: None,
                created_at: now,
                updated_at: now,
                terminal_at: None,
            },
            correlation_id,
        )?;
        self.persistence.transition_sandbox_execution(
            plan.execution_id,
            SandboxExecutionState::Planned,
            SandboxExecutionState::Starting,
            None,
            None,
            correlation_id,
        )?;
        emit(SandboxHostEvent::Started { execution_id: plan.execution_id });

        let cancel_flag = self.cancellation.register(plan.cancellation_id)?;
        let result = self.execute_started(
            &plan,
            &cancel_flag,
            &mut emit,
            stdio_bootstrap,
            secret_environment,
            stdin_commands,
        );
        self.cancellation.remove(plan.cancellation_id);
        result
    }

    fn execute_started<F>(
        &self,
        plan: &SandboxExecutionPlan,
        cancel_flag: &AtomicBool,
        emit: &mut F,
        stdio_bootstrap: Option<StdioDispatchBootstrapV1>,
        secret_environment: Option<NativeSecretEnvironmentEntry>,
        stdin_commands: Option<Receiver<SandboxProcessInputV1>>,
    ) -> Result<SandboxExecutionOutcome, SandboxError>
    where
        F: FnMut(SandboxHostEvent),
    {
        let correlation_id = plan.identity.correlation_id;
        let appcontainer = matches!(
            plan.mode,
            NativeSandboxMode::CompatibilityAppContainer | NativeSandboxMode::StrictAppContainer
        );
        let profile_name = appcontainer
            .then(|| format!("{}.{}", self.options.appcontainer_profile_prefix, plan.execution_id));
        let profile =
            match profile_name.as_deref().map(create_fresh_appcontainer_profile).transpose() {
                Ok(profile) => profile,
                Err(SandboxError::AppContainerProfileAlreadyExists) => {
                    let _ = self.persistence.transition_sandbox_execution(
                        plan.execution_id,
                        SandboxExecutionState::Starting,
                        SandboxExecutionState::PolicyViolation,
                        None,
                        None,
                        correlation_id,
                    );
                    emit(SandboxHostEvent::PolicyViolation {
                        execution_id: plan.execution_id,
                        code: "sandbox.appcontainer_profile_already_exists".to_owned(),
                    });
                    return Err(SandboxError::AppContainerProfileAlreadyExists);
                }
                Err(error) => return Err(error),
            };
        let mut journals = Vec::new();
        let preparation = (|| -> Result<(), SandboxError> {
            if let Some(profile) = &profile {
                let sid = profile.sid_string()?;
                for grant in &plan.filesystem_grants {
                    journals.push(self.apply_acl_grant(plan, grant, profile.name(), &sid)?);
                }
            }
            Ok(())
        })();
        if let Err(error) = preparation {
            let cleanup = self.cleanup_acl_journals(
                plan.execution_id,
                &journals,
                profile_name.as_deref(),
                emit,
            );
            let terminal = if cleanup.is_ok() {
                SandboxExecutionState::PolicyViolation
            } else {
                SandboxExecutionState::CleanupFailed
            };
            let _ = self.persistence.transition_sandbox_execution(
                plan.execution_id,
                SandboxExecutionState::Starting,
                terminal,
                None,
                None,
                correlation_id,
            );
            emit(SandboxHostEvent::PolicyViolation {
                execution_id: plan.execution_id,
                code: "sandbox.acl_prepare_failed".to_owned(),
            });
            return cleanup.and(Err(error));
        }

        let run_result = self.execute_prepared(
            plan,
            cancel_flag,
            emit,
            appcontainer,
            profile.as_ref(),
            stdio_bootstrap,
            secret_environment,
            stdin_commands,
        );
        let cleanup_result =
            self.cleanup_acl_journals(plan.execution_id, &journals, profile_name.as_deref(), emit);
        match (run_result, cleanup_result) {
            (Ok(outcome), Ok(())) => Ok(outcome),
            (Ok(outcome), Err(cleanup_error)) => {
                let _ = self.persistence.transition_sandbox_execution(
                    plan.execution_id,
                    outcome.state,
                    SandboxExecutionState::CleanupFailed,
                    outcome.process_id,
                    outcome.exit_code,
                    correlation_id,
                );
                Err(cleanup_error)
            }
            (Err(run_error), Ok(())) => {
                self.mark_unknown_after_runtime_error(plan, correlation_id);
                Err(run_error)
            }
            (Err(run_error), Err(cleanup_error)) => {
                self.mark_cleanup_failed(plan, correlation_id);
                Err(SandboxError::CleanupFailed(format!(
                    "execution failed: {run_error}; cleanup failed: {cleanup_error}"
                )))
            }
        }
    }

    // This private orchestration boundary keeps the exact execution plan, cancellation,
    // input/output, and lifecycle channels visible to the sandbox verifier.
    #[allow(clippy::too_many_arguments)]
    fn execute_prepared<F>(
        &self,
        plan: &SandboxExecutionPlan,
        cancel_flag: &AtomicBool,
        emit: &mut F,
        appcontainer: bool,
        profile: Option<&AppContainerProfile>,
        stdio_bootstrap: Option<StdioDispatchBootstrapV1>,
        secret_environment: Option<NativeSecretEnvironmentEntry>,
        stdin_commands: Option<Receiver<SandboxProcessInputV1>>,
    ) -> Result<SandboxExecutionOutcome, SandboxError>
    where
        F: FnMut(SandboxHostEvent),
    {
        let correlation_id = plan.identity.correlation_id;
        let executable = match verify_executable_ref(&plan.executable) {
            Ok(executable) => executable,
            Err(error) => {
                let _ = self.persistence.transition_sandbox_execution(
                    plan.execution_id,
                    SandboxExecutionState::Starting,
                    SandboxExecutionState::PolicyViolation,
                    None,
                    None,
                    correlation_id,
                );
                emit(SandboxHostEvent::PolicyViolation {
                    execution_id: plan.execution_id,
                    code: "sandbox.executable_verification_failed".to_owned(),
                });
                return Err(error);
            }
        };
        let executable_path = executable.info().final_path.clone();
        let working_directory = path_from_canonical(&plan.working_directory)?;
        let arguments = plan.args.iter().map(|arg| arg.utf16.clone()).collect::<Vec<_>>();
        let mut environment = plan
            .environment
            .iter()
            .map(|entry| NativeEnvironmentEntry {
                name: entry.name.clone(),
                value_utf16: entry.value.utf16.clone(),
            })
            .collect::<Vec<_>>();
        if let Some(bootstrap) = stdio_bootstrap {
            environment.extend([
                NativeEnvironmentEntry {
                    name: aura_tool_contract::AURA_STDIO_TOOL_CALL_ID_ENVIRONMENT_VARIABLE_V1
                        .to_owned(),
                    value_utf16: bootstrap.tool_call_id().to_string().encode_utf16().collect(),
                },
                NativeEnvironmentEntry {
                    name:
                        aura_tool_contract::AURA_STDIO_TOOL_EXECUTION_ATTEMPT_ID_ENVIRONMENT_VARIABLE_V1
                            .to_owned(),
                    value_utf16: bootstrap
                        .tool_execution_attempt_id()
                        .to_string()
                        .encode_utf16()
                        .collect(),
                },
                NativeEnvironmentEntry {
                    name: aura_tool_contract::AURA_TOOL_OUTPUT_ROOT_ENVIRONMENT_VARIABLE_V1
                        .to_owned(),
                    value_utf16: bootstrap.output_root().utf16.clone(),
                },
            ]);
        }
        let platform_mode = if appcontainer {
            NativeRestrictedMode::AppContainer
        } else {
            NativeRestrictedMode::StandardUser
        };
        let launched = match secret_environment {
            Some(secret) => spawn_restricted_process_with_secret_environment(
                platform_mode,
                profile,
                &executable_path,
                &arguments,
                &working_directory,
                &environment,
                secret,
            ),
            None => spawn_restricted_process(
                platform_mode,
                profile,
                &executable_path,
                &arguments,
                &working_directory,
                &environment,
            ),
        };
        let mut process = match launched {
            Ok(process) => {
                drop(executable);
                process
            }
            Err(error) => {
                drop(executable);
                let _ = self.persistence.transition_sandbox_execution(
                    plan.execution_id,
                    SandboxExecutionState::Starting,
                    SandboxExecutionState::PolicyViolation,
                    None,
                    None,
                    correlation_id,
                );
                return Err(SandboxError::Platform(error));
            }
        };
        let mut stdin_commands = stdin_commands;
        if stdin_commands.is_none() {
            process.close_stdin();
        }
        let process_id = process.process_id();
        self.persistence.transition_sandbox_execution(
            plan.execution_id,
            SandboxExecutionState::Starting,
            SandboxExecutionState::Running,
            Some(process_id),
            None,
            correlation_id,
        )?;
        emit(SandboxHostEvent::ProcessStarted { execution_id: plan.execution_id, process_id });

        let started = Instant::now();
        let timeout = Duration::from_millis(plan.timeout_ms);
        let mut output = OutputTracker::default();
        let (state, exit_code) = loop {
            apply_stdin_commands(&mut process, &mut stdin_commands)?;
            let drained = drain_process_output(&process, plan, &mut output, emit)?;
            if drained.limit_exceeded {
                process.terminate(0xC000_0409)?;
                let exit = process.wait_timeout(Duration::from_secs(2))?;
                let tree_exited = process.wait_for_process_tree_exit(Duration::from_secs(2))?;
                emit(SandboxHostEvent::PolicyViolation {
                    execution_id: plan.execution_id,
                    code: if exit.is_some() && tree_exited {
                        "sandbox.output_limit_exceeded"
                    } else {
                        "sandbox.runtime_termination_unknown"
                    }
                    .to_owned(),
                });
                break if tree_exited {
                    termination_outcome(exit, SandboxExecutionState::PolicyViolation)
                } else {
                    (SandboxExecutionState::StateUnknown, None)
                };
            }
            if cancel_flag.load(Ordering::Acquire) {
                process.terminate(0xC000_013A)?;
                let exit = process.wait_timeout(Duration::from_secs(2))?;
                let tree_exited = process.wait_for_process_tree_exit(Duration::from_secs(2))?;
                if exit.is_some() && tree_exited {
                    drain_remaining_output(&process, plan, &mut output, emit)?;
                    emit(SandboxHostEvent::Cancelled { execution_id: plan.execution_id });
                    break termination_outcome(exit, SandboxExecutionState::Cancelled);
                }
                emit(SandboxHostEvent::PolicyViolation {
                    execution_id: plan.execution_id,
                    code: "sandbox.runtime_termination_unknown".to_owned(),
                });
                break (SandboxExecutionState::StateUnknown, None);
            }
            if started.elapsed() >= timeout {
                process.terminate(0x102)?;
                let exit = process.wait_timeout(Duration::from_secs(2))?;
                let tree_exited = process.wait_for_process_tree_exit(Duration::from_secs(2))?;
                if exit.is_some() && tree_exited {
                    drain_remaining_output(&process, plan, &mut output, emit)?;
                    emit(SandboxHostEvent::Timeout { execution_id: plan.execution_id });
                    break termination_outcome(exit, SandboxExecutionState::TimedOut);
                }
                emit(SandboxHostEvent::PolicyViolation {
                    execution_id: plan.execution_id,
                    code: "sandbox.runtime_termination_unknown".to_owned(),
                });
                break (SandboxExecutionState::StateUnknown, None);
            }
            if let Some(exit_code) = process.wait_timeout(self.options.poll_interval)? {
                if !process.wait_for_process_tree_exit(Duration::ZERO)? {
                    process.terminate(0xC000_013A)?;
                }
                if !process.wait_for_process_tree_exit(Duration::from_secs(2))? {
                    emit(SandboxHostEvent::PolicyViolation {
                        execution_id: plan.execution_id,
                        code: "sandbox.runtime_termination_unknown".to_owned(),
                    });
                    break (SandboxExecutionState::StateUnknown, None);
                }
                let exceeded = drain_remaining_output(&process, plan, &mut output, emit)?;
                if exceeded {
                    emit(SandboxHostEvent::PolicyViolation {
                        execution_id: plan.execution_id,
                        code: "sandbox.output_limit_exceeded".to_owned(),
                    });
                    break (SandboxExecutionState::PolicyViolation, Some(exit_code));
                }
                emit(SandboxHostEvent::Exit { execution_id: plan.execution_id, exit_code });
                break (SandboxExecutionState::Exited, Some(exit_code));
            }
        };
        self.persistence.transition_sandbox_execution(
            plan.execution_id,
            SandboxExecutionState::Running,
            state,
            Some(process_id),
            exit_code,
            correlation_id,
        )?;
        Ok(SandboxExecutionOutcome {
            execution_id: plan.execution_id,
            state,
            process_id: Some(process_id),
            exit_code,
            network_enforcement: plan.network_enforcement,
        })
    }

    fn mark_unknown_after_runtime_error(
        &self,
        plan: &SandboxExecutionPlan,
        correlation_id: CorrelationId,
    ) {
        for expected in [SandboxExecutionState::Running, SandboxExecutionState::Starting] {
            if self
                .persistence
                .transition_sandbox_execution(
                    plan.execution_id,
                    expected,
                    SandboxExecutionState::StateUnknown,
                    None,
                    None,
                    correlation_id,
                )
                .is_ok()
            {
                break;
            }
        }
    }

    fn mark_cleanup_failed(&self, plan: &SandboxExecutionPlan, correlation_id: CorrelationId) {
        for expected in [
            SandboxExecutionState::Running,
            SandboxExecutionState::Starting,
            SandboxExecutionState::Exited,
            SandboxExecutionState::Cancelled,
            SandboxExecutionState::TimedOut,
            SandboxExecutionState::PolicyViolation,
            SandboxExecutionState::StateUnknown,
        ] {
            if self
                .persistence
                .transition_sandbox_execution(
                    plan.execution_id,
                    expected,
                    SandboxExecutionState::CleanupFailed,
                    None,
                    None,
                    correlation_id,
                )
                .is_ok()
            {
                break;
            }
        }
    }

    fn apply_acl_grant(
        &self,
        plan: &SandboxExecutionPlan,
        grant: &SandboxFilesystemGrant,
        appcontainer_profile_name: &str,
        appcontainer_sid: &str,
    ) -> Result<SandboxAclJournalEntry, SandboxError> {
        let path = path_from_canonical(&grant.path)?;
        let original_dacl_sddl = capture_file_dacl(&path)?;
        let now = UtcTimestamp::now();
        let prepared = SandboxAclJournalEntry {
            id: SandboxAclJournalId::new(),
            execution_id: plan.execution_id,
            target_identity: grant.identity.clone(),
            target_path: grant.path.clone(),
            original_dacl_hash: ContentHash::blake3(original_dacl_sddl.as_bytes()),
            original_dacl_sddl,
            appcontainer_profile_name: appcontainer_profile_name.to_owned(),
            appcontainer_sid: appcontainer_sid.to_owned(),
            access: grant.access,
            recursive: grant.recursive,
            state: SandboxAclJournalState::Prepared,
            created_at: now,
            updated_at: now,
            error_code: None,
        };
        self.persistence
            .prepare_sandbox_acl_journal(prepared.clone(), plan.identity.correlation_id)?;
        let native_access = match grant.access {
            SandboxFilesystemAccess::Read => NativeAclAccess::Read,
            SandboxFilesystemAccess::ReadWrite | SandboxFilesystemAccess::Create => {
                NativeAclAccess::ReadWrite
            }
        };
        if let Err(error) = grant_file_dacl(&path, appcontainer_sid, native_access, grant.recursive)
        {
            let cleanup_result = revoke_file_dacl(&path, appcontainer_sid, grant.recursive);
            let (next, code) = if cleanup_result.is_ok() {
                (SandboxAclJournalState::Cleaned, Some("sandbox.acl_apply_failed".to_owned()))
            } else {
                (
                    SandboxAclJournalState::CleanupFailed,
                    Some("sandbox.acl_apply_and_restore_failed".to_owned()),
                )
            };
            let _ = self.persistence.update_sandbox_acl_journal(
                prepared.id,
                SandboxAclJournalState::Prepared,
                next,
                code,
                plan.identity.correlation_id,
            );
            return Err(SandboxError::Platform(error));
        }
        Ok(self.persistence.update_sandbox_acl_journal(
            prepared.id,
            SandboxAclJournalState::Prepared,
            SandboxAclJournalState::Applied,
            None,
            plan.identity.correlation_id,
        )?)
    }

    fn cleanup_acl_journals<F>(
        &self,
        execution_id: SandboxExecutionId,
        journals: &[SandboxAclJournalEntry],
        appcontainer_profile_name: Option<&str>,
        emit: &mut F,
    ) -> Result<(), SandboxError>
    where
        F: FnMut(SandboxHostEvent),
    {
        let mut failures = Vec::new();
        for journal in journals.iter().rev() {
            let current = self.persistence.update_sandbox_acl_journal(
                journal.id,
                journal.state,
                SandboxAclJournalState::CleanupPending,
                None,
                CorrelationId::new(),
            )?;
            let path = path_from_canonical(&current.target_path)?;
            match revoke_file_dacl(&path, &current.appcontainer_sid, current.recursive) {
                Ok(()) => {
                    self.persistence.update_sandbox_acl_journal(
                        current.id,
                        SandboxAclJournalState::CleanupPending,
                        SandboxAclJournalState::Cleaned,
                        None,
                        CorrelationId::new(),
                    )?;
                    emit(SandboxHostEvent::CleanupCompleted { execution_id: current.execution_id });
                }
                Err(error) => {
                    let code =
                        format!("sandbox.acl_restore_failed.{}", error.raw_os_error().unwrap_or(0));
                    self.persistence.update_sandbox_acl_journal(
                        current.id,
                        SandboxAclJournalState::CleanupPending,
                        SandboxAclJournalState::CleanupFailed,
                        Some(code.clone()),
                        CorrelationId::new(),
                    )?;
                    emit(SandboxHostEvent::CleanupFailed {
                        execution_id: current.execution_id,
                        code: code.clone(),
                    });
                    failures.push(code);
                }
            }
        }
        if failures.is_empty() {
            if let Some(profile_name) = appcontainer_profile_name
                && let Err(error) = delete_appcontainer_profile(profile_name)
            {
                let code = format!(
                    "sandbox.appcontainer_profile_delete_failed.{}",
                    error.raw_os_error().unwrap_or(0)
                );
                emit(SandboxHostEvent::CleanupFailed { execution_id, code: code.clone() });
                return Err(SandboxError::CleanupFailed(code));
            }
            Ok(())
        } else {
            Err(SandboxError::CleanupFailed(failures.join(",")))
        }
    }

    pub fn recover_acl_journal<F>(&self, mut emit: F) -> Result<u64, SandboxError>
    where
        F: FnMut(SandboxHostEvent),
    {
        let entries = self.persistence.pending_sandbox_acl_cleanup()?;
        let mut restored = 0_u64;
        let mut profile_cleanup = BTreeMap::<String, bool>::new();
        for entry in entries {
            let expected = entry.state;
            let pending = if expected == SandboxAclJournalState::CleanupPending {
                entry
            } else {
                self.persistence.update_sandbox_acl_journal(
                    entry.id,
                    expected,
                    SandboxAclJournalState::CleanupPending,
                    None,
                    CorrelationId::new(),
                )?
            };
            let path = path_from_canonical(&pending.target_path)?;
            profile_cleanup.entry(pending.appcontainer_profile_name.clone()).or_insert(true);
            match revoke_file_dacl(&path, &pending.appcontainer_sid, pending.recursive) {
                Ok(()) => {
                    self.persistence.update_sandbox_acl_journal(
                        pending.id,
                        SandboxAclJournalState::CleanupPending,
                        SandboxAclJournalState::Cleaned,
                        None,
                        CorrelationId::new(),
                    )?;
                    emit(SandboxHostEvent::CleanupCompleted { execution_id: pending.execution_id });
                    restored = restored.saturating_add(1);
                }
                Err(error) => {
                    profile_cleanup.insert(pending.appcontainer_profile_name.clone(), false);
                    let code =
                        format!("sandbox.acl_restore_failed.{}", error.raw_os_error().unwrap_or(0));
                    self.persistence.update_sandbox_acl_journal(
                        pending.id,
                        SandboxAclJournalState::CleanupPending,
                        SandboxAclJournalState::CleanupFailed,
                        Some(code.clone()),
                        CorrelationId::new(),
                    )?;
                    emit(SandboxHostEvent::CleanupFailed {
                        execution_id: pending.execution_id,
                        code,
                    });
                }
            }
        }
        for (profile_name, clean) in profile_cleanup {
            if clean && delete_appcontainer_profile(&profile_name).is_err() {
                return Err(SandboxError::CleanupFailed(format!(
                    "sandbox.appcontainer_profile_delete_failed:{profile_name}"
                )));
            }
        }
        Ok(restored)
    }
}

fn apply_stdin_commands(
    process: &mut NativeSandboxProcess,
    commands: &mut Option<Receiver<SandboxProcessInputV1>>,
) -> Result<(), SandboxError> {
    let Some(receiver) = commands.take() else {
        return Ok(());
    };
    loop {
        match receiver.try_recv() {
            Ok(SandboxProcessInputV1::Write(bytes)) => {
                process.write_stdin(&bytes).map_err(SandboxError::Platform)?;
            }
            Ok(SandboxProcessInputV1::Close) => {
                process.close_stdin();
                *commands = None;
                return Ok(());
            }
            Err(TryRecvError::Empty) => {
                *commands = Some(receiver);
                return Ok(());
            }
            Err(TryRecvError::Disconnected) => {
                process.close_stdin();
                return Ok(());
            }
        }
    }
}

const MAX_OUTPUT_CHUNK_BYTES: usize = 64 * 1024;

fn drain_remaining_output<F>(
    process: &NativeSandboxProcess,
    plan: &SandboxExecutionPlan,
    tracker: &mut OutputTracker,
    emit: &mut F,
) -> Result<bool, SandboxError>
where
    F: FnMut(SandboxHostEvent),
{
    loop {
        let result = drain_process_output(process, plan, tracker, emit)?;
        if result.limit_exceeded {
            return Ok(true);
        }
        if !result.made_progress {
            return Ok(false);
        }
    }
}

fn drain_process_output<F>(
    process: &NativeSandboxProcess,
    plan: &SandboxExecutionPlan,
    tracker: &mut OutputTracker,
    emit: &mut F,
) -> Result<OutputDrainResult, SandboxError>
where
    F: FnMut(SandboxHostEvent),
{
    let stdout_remaining = plan
        .output_limits
        .stdout_bytes
        .saturating_sub(tracker.stdout_bytes)
        .min(plan.output_limits.combined_bytes.saturating_sub(tracker.combined_bytes));
    let stdout = process.drain_stdout(output_read_budget(stdout_remaining))?;
    let stdout_allowed =
        usize::try_from(stdout_remaining.min(stdout.len() as u64)).unwrap_or(stdout.len());
    if stdout_allowed > 0 {
        let bytes = stdout[..stdout_allowed].to_vec();
        let length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        emit(SandboxHostEvent::StdoutChunk {
            execution_id: plan.execution_id,
            sequence: tracker.stdout_sequence,
            bytes,
        });
        tracker.stdout_sequence = tracker.stdout_sequence.saturating_add(1);
        tracker.stdout_bytes = tracker.stdout_bytes.saturating_add(length);
        tracker.combined_bytes = tracker.combined_bytes.saturating_add(length);
    }
    let mut limit_exceeded = stdout.len() > stdout_allowed;

    let stderr_remaining = plan
        .output_limits
        .stderr_bytes
        .saturating_sub(tracker.stderr_bytes)
        .min(plan.output_limits.combined_bytes.saturating_sub(tracker.combined_bytes));
    let stderr = process.drain_stderr(output_read_budget(stderr_remaining))?;
    let stderr_allowed =
        usize::try_from(stderr_remaining.min(stderr.len() as u64)).unwrap_or(stderr.len());
    if stderr_allowed > 0 {
        let bytes = stderr[..stderr_allowed].to_vec();
        let length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        emit(SandboxHostEvent::StderrChunk {
            execution_id: plan.execution_id,
            sequence: tracker.stderr_sequence,
            bytes,
        });
        tracker.stderr_sequence = tracker.stderr_sequence.saturating_add(1);
        tracker.stderr_bytes = tracker.stderr_bytes.saturating_add(length);
        tracker.combined_bytes = tracker.combined_bytes.saturating_add(length);
    }
    limit_exceeded |= stderr.len() > stderr_allowed;
    Ok(OutputDrainResult {
        made_progress: !stdout.is_empty() || !stderr.is_empty(),
        limit_exceeded,
    })
}

fn output_read_budget(remaining: u64) -> usize {
    usize::try_from(remaining.min((MAX_OUTPUT_CHUNK_BYTES - 1) as u64))
        .unwrap_or(MAX_OUTPUT_CHUNK_BYTES - 1)
        .saturating_add(1)
}

fn termination_outcome(
    exit_code: Option<u32>,
    confirmed_state: SandboxExecutionState,
) -> (SandboxExecutionState, Option<u32>) {
    match exit_code {
        Some(exit_code) => (confirmed_state, Some(exit_code)),
        None => (SandboxExecutionState::StateUnknown, None),
    }
}

pub(crate) fn path_from_canonical(path: &CanonicalPath) -> Result<PathBuf, SandboxError> {
    if path.utf16.is_empty() || path.utf16.contains(&0) {
        return Err(SandboxError::InvalidWindowsPath);
    }
    Ok(PathBuf::from(OsString::from_wide(&path.utf16)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unconfirmed_process_termination_is_never_reported_as_cancelled_or_timed_out() {
        assert_eq!(
            termination_outcome(None, SandboxExecutionState::Cancelled),
            (SandboxExecutionState::StateUnknown, None)
        );
        assert_eq!(
            termination_outcome(None, SandboxExecutionState::TimedOut),
            (SandboxExecutionState::StateUnknown, None)
        );
        assert_eq!(
            termination_outcome(Some(0x102), SandboxExecutionState::TimedOut),
            (SandboxExecutionState::TimedOut, Some(0x102))
        );
    }
    use aura_domain_common::{
        ApprovalRequestId, ElevationLaunchTicketId, ReleaseBundleId, Revision, RuntimeInstanceId,
        SessionId, WorkspaceId,
    };
    use aura_persistence::{PersistenceOptions, PersistenceStartup};
    use aura_runtime_contract::{BundleNamespace, ReleaseIdentity};
    use aura_workspace_domain::{
        CatalogFreshness, SessionSummary, WorkspaceState, WorkspaceSummary,
    };
    use serde::Deserialize;
    use std::os::windows::ffi::OsStrExt;

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct SandboxHostEventFixture {
        schema_version: u32,
        event_kinds: Vec<String>,
    }

    #[test]
    fn sandbox_host_event_names_match_the_stable_fixture() {
        let execution_id = SandboxExecutionId::new();
        let events = [
            SandboxHostEvent::Started { execution_id },
            SandboxHostEvent::ProcessStarted { execution_id, process_id: 42 },
            SandboxHostEvent::StdoutChunk { execution_id, sequence: 1, bytes: vec![1] },
            SandboxHostEvent::StderrChunk { execution_id, sequence: 2, bytes: vec![2] },
            SandboxHostEvent::Exit { execution_id, exit_code: 0 },
            SandboxHostEvent::Cancelled { execution_id },
            SandboxHostEvent::Timeout { execution_id },
            SandboxHostEvent::PolicyViolation { execution_id, code: "fixture".to_owned() },
            SandboxHostEvent::CleanupCompleted { execution_id },
            SandboxHostEvent::CleanupFailed { execution_id, code: "fixture".to_owned() },
        ];
        let serialized_names = events
            .iter()
            .map(|event| {
                serde_json::to_value(event)
                    .expect("serialize sandbox host event")
                    .get("event_kind")
                    .and_then(serde_json::Value::as_str)
                    .expect("event kind")
                    .to_owned()
            })
            .collect::<Vec<_>>();
        let fixture: SandboxHostEventFixture =
            serde_json::from_str(include_str!("../../../fixtures/sandbox/host-events.json"))
                .expect("sandbox host event fixture must remain valid");

        assert_eq!(fixture.schema_version, 1);
        assert_eq!(serialized_names, fixture.event_kinds);
    }

    #[test]
    fn plan_hash_detects_any_mutation() {
        let mut plan = test_plan(NativeSandboxMode::ObservedNative);
        plan.plan_hash = plan_hash(&plan).expect("plan hash");
        assert_eq!(plan_hash(&plan).expect("stable plan hash"), plan.plan_hash);
        plan.timeout_ms += 1;
        assert_ne!(plan_hash(&plan).expect("changed hash"), plan.plan_hash);

        let mut plan = test_plan(NativeSandboxMode::ObservedNative);
        plan.plan_hash = plan_hash(&plan).expect("plan hash");
        plan.network_enforcement = NetworkEnforcementKind::CoreControlPlane;
        assert_ne!(plan_hash(&plan).expect("changed enforcement hash"), plan.plan_hash);
    }

    #[test]
    fn default_planner_requires_explicit_mode_and_registered_descriptor_is_immutable() {
        let template = test_plan(NativeSandboxMode::CompatibilityAppContainer);
        let operation_id = SecurityOperationId::new();
        let resolver = InMemorySandboxOperationResolver::default();
        let descriptor = SandboxCommandDescriptor {
            executable: template.executable.clone(),
            args: template.args.clone(),
            working_directory: template.working_directory.clone(),
            environment: template.environment.clone(),
            filesystem_grants: template.filesystem_grants.clone(),
            network_grant_ids: template.network_grant_ids.clone(),
            timeout_ms: template.timeout_ms,
            output_limits: template.output_limits.clone(),
            cancellation_id: template.cancellation_id,
        };
        resolver.register(operation_id, descriptor.clone()).expect("register descriptor");
        resolver.register(operation_id, descriptor.clone()).expect("idempotent re-register");
        let mut replacement = descriptor;
        replacement.args.push(WindowsOsString { utf16: "changed".encode_utf16().collect() });
        assert!(resolver.register(operation_id, replacement).is_err());

        let snapshot = aura_security_contract::PermissionSnapshot {
            id: template.permission_snapshot_id,
            session_id: template.identity.session_id.expect("session"),
            workspace_id: template.identity.workspace_id.expect("workspace"),
            mode: aura_security_contract::PermissionMode::FullAccess,
            session_permission_revision: 1,
            active_grant_ids: vec![],
            created_at: UtcTimestamp::now(),
        };
        let mut operation = OperationSecurityPlan {
            operation_id,
            identity: template.identity,
            source: aura_permission_domain::SecuritySource::UserInterface,
            capabilities: vec![aura_security_contract::ResolvedCapability::ProcessExecute],
            scopes: vec![aura_permission_domain::ResolvedScope::WorkspaceRoot {
                workspace_id: snapshot.workspace_id,
            }],
            risk_flags: vec![],
            purpose: None,
            source_fingerprint: None,
            requested_sandbox: None,
            network_access: NetworkAccessRequirement::NotRequired,
            network_plan: None,
            operation_fingerprint: ContentHash::blake3(b"explicit-mode-test"),
        };
        operation.identity.permission_snapshot_id = Some(snapshot.id);
        let decision = PermissionDecision::Allow { snapshot };
        let planner = DefaultSandboxPlanner::new(resolver);
        assert!(matches!(
            planner.plan(&operation, &decision),
            Err(SandboxError::SandboxModeNotSelected)
        ));
        operation.requested_sandbox = Some(NativeSandboxMode::CompatibilityAppContainer);
        assert_eq!(
            planner.plan(&operation, &decision).expect("planned").mode,
            NativeSandboxMode::CompatibilityAppContainer
        );
    }

    #[test]
    fn appcontainer_network_access_fails_closed_with_structured_replan_guidance() {
        for mode in
            [NativeSandboxMode::CompatibilityAppContainer, NativeSandboxMode::StrictAppContainer]
        {
            assert_eq!(
                validate_native_network_policy(mode, NetworkAccessRequirement::NotRequired, &[],),
                Ok(NetworkEnforcementKind::AppContainerNetworkDenied)
            );
            let error =
                validate_native_network_policy(mode, NetworkAccessRequirement::Required, &[])
                    .expect_err("AppContainer must remain network denied");
            assert_eq!(error.code, SandboxNetworkPolicyErrorCode::AppContainerNetworkDenied);
            assert_eq!(error.required_declaration, NetworkAccessRequirement::Required);
            assert_eq!(
                error.allowed_networked_modes,
                vec![NetworkExecutionMode::Native {
                    mode: NativeSandboxMode::ObservedNativeNetworked,
                }]
            );
        }
    }

    #[test]
    fn preexisting_appcontainer_profile_is_rejected_fail_closed() {
        let profile_name = format!("Aura.Test.FreshProfile.{}", SandboxExecutionId::new());
        let first = create_fresh_appcontainer_profile(&profile_name).expect("create fresh profile");
        assert!(matches!(
            create_fresh_appcontainer_profile(&profile_name),
            Err(SandboxError::AppContainerProfileAlreadyExists)
        ));
        drop(first);
        delete_appcontainer_profile(&profile_name).expect("delete test profile");
    }

    #[test]
    fn native_networked_mode_is_explicit_and_never_a_silent_fallback() {
        let undeclared = validate_native_network_policy(
            NativeSandboxMode::ObservedNativeNetworked,
            NetworkAccessRequirement::NotRequired,
            &[],
        )
        .expect_err("networked mode requires an explicit declaration");
        assert_eq!(
            undeclared.code,
            SandboxNetworkPolicyErrorCode::NetworkAccessDeclarationRequired
        );

        let unselected = validate_native_network_policy(
            NativeSandboxMode::ObservedNative,
            NetworkAccessRequirement::Required,
            &[],
        )
        .expect_err("ObservedNative must not silently become networked");
        assert_eq!(
            unselected.code,
            SandboxNetworkPolicyErrorCode::NetworkAccessDeclarationRequired
        );
        assert_eq!(
            validate_native_network_policy(
                NativeSandboxMode::ObservedNativeNetworked,
                NetworkAccessRequirement::Required,
                &[],
            ),
            Ok(NetworkEnforcementKind::ObservedNativeNotProxyEnforced)
        );
        let misleading_grant = validate_native_network_policy(
            NativeSandboxMode::ObservedNativeNetworked,
            NetworkAccessRequirement::Required,
            &[NetworkGrantId::new()],
        )
        .expect_err("an unenforced native route must not accept a network Grant");
        assert_eq!(
            misleading_grant.code,
            SandboxNetworkPolicyErrorCode::NetworkGrantUnavailableInSelectedMode
        );
    }

    #[test]
    fn external_strict_remains_network_denied_and_managed_networking_is_explicit() {
        let strict = validate_external_network_policy(
            ExternalIsolationMode::StrictAppContainer,
            NetworkAccessRequirement::Required,
        )
        .expect_err("Strict external AppContainer must remain network denied");
        assert_eq!(strict.code, SandboxNetworkPolicyErrorCode::AppContainerNetworkDenied);
        assert_eq!(
            strict.allowed_networked_modes,
            vec![NetworkExecutionMode::External {
                mode: ExternalIsolationMode::ManagedUserNetworked,
            }]
        );

        let compatibility = validate_external_network_policy(
            ExternalIsolationMode::CompatibilityManagedUser,
            NetworkAccessRequirement::Required,
        )
        .expect_err("compatibility mode must not silently become networked");
        assert_eq!(
            compatibility.code,
            SandboxNetworkPolicyErrorCode::NetworkAccessDeclarationRequired
        );
        assert_eq!(
            validate_external_network_policy(
                ExternalIsolationMode::ManagedUserNetworked,
                NetworkAccessRequirement::Required,
            ),
            Ok(NetworkEnforcementKind::ObservedNativeNotProxyEnforced)
        );
    }

    #[test]
    fn external_compatibility_launches_as_managed_standard_user_not_appcontainer() {
        let native = test_plan(NativeSandboxMode::ObservedNative);
        let mut plan = ExternalProcessLaunchPlan {
            execution_id: SandboxExecutionId::new(),
            identity: native.identity,
            permission_snapshot_id: native.permission_snapshot_id,
            source: aura_permission_domain::SecuritySource::Hook {
                hook_id: "fixture-hook".to_owned(),
            },
            source_fingerprint: ContentHash::blake3(b"trusted external fixture"),
            isolation: ExternalIsolationMode::CompatibilityManagedUser,
            network_access: NetworkAccessRequirement::NotRequired,
            network_enforcement: NetworkEnforcementKind::ObservedNativeNotProxyEnforced,
            executable: native.executable,
            args: ["/d", "/s", "/c", "ping.exe -n 3 127.0.0.1 >nul"]
                .map(|value| WindowsOsString { utf16: value.encode_utf16().collect() })
                .to_vec(),
            working_directory: native.working_directory,
            environment: native.environment,
            timeout_ms: native.timeout_ms,
            output_limits: native.output_limits,
            cancellation_id: native.cancellation_id,
            plan_hash: ContentHash::blake3(b"uninitialized"),
        };
        plan.plan_hash = external_process_plan_hash(&plan).expect("external plan hash");
        let trust = trusted_external_record(&plan);

        let process = launch_external_process(&plan, &trust).expect("launch external managed user");
        assert_eq!(process.isolation(), ExternalIsolationMode::CompatibilityManagedUser);
        assert_eq!(
            process.network_enforcement(),
            NetworkEnforcementKind::ObservedNativeNotProxyEnforced
        );
        assert!(!process.token_is_appcontainer().expect("AppContainer token state"));
        assert!(!process.token_is_elevated().expect("elevation token state"));
        process.terminate_tree(0xC000_013A).expect("terminate external process tree");
        process.close().expect("close external process");
    }

    #[test]
    fn external_launch_rejects_non_external_source_and_changed_executable_hash() {
        let native = test_plan(NativeSandboxMode::ObservedNative);
        let mut plan = ExternalProcessLaunchPlan {
            execution_id: SandboxExecutionId::new(),
            identity: native.identity,
            permission_snapshot_id: native.permission_snapshot_id,
            source: aura_permission_domain::SecuritySource::UserInterface,
            source_fingerprint: ContentHash::blake3(b"not external"),
            isolation: ExternalIsolationMode::CompatibilityManagedUser,
            network_access: NetworkAccessRequirement::NotRequired,
            network_enforcement: NetworkEnforcementKind::ObservedNativeNotProxyEnforced,
            executable: native.executable,
            args: vec![],
            working_directory: native.working_directory,
            environment: native.environment,
            timeout_ms: native.timeout_ms,
            output_limits: native.output_limits,
            cancellation_id: native.cancellation_id,
            plan_hash: ContentHash::blake3(b"uninitialized"),
        };
        plan.plan_hash = external_process_plan_hash(&plan).expect("external plan hash");
        let trust = trusted_external_record(&plan);
        assert!(matches!(
            launch_external_process(&plan, &trust),
            Err(SandboxError::ExternalSourceNotTrustBound)
        ));

        plan.source = aura_permission_domain::SecuritySource::McpServer {
            server_id: "fixture-server".to_owned(),
        };
        let mut trust = trusted_external_record(&plan);
        trust.state = TrustState::Revoked;
        plan.plan_hash = external_process_plan_hash(&plan).expect("revoked plan hash");
        assert!(matches!(
            launch_external_process(&plan, &trust),
            Err(SandboxError::ExternalSourceNotTrusted)
        ));

        plan.executable.content_hash = ContentHash::blake3(b"changed executable");
        plan.plan_hash = external_process_plan_hash(&plan).expect("changed plan hash");
        let trust = trusted_external_record(&plan);
        assert!(matches!(
            launch_external_process(&plan, &trust),
            Err(SandboxError::ExecutableHashMismatch)
        ));
    }

    #[test]
    fn host_preparation_plan_hash_binds_release_runtime_revision_nonce_and_expiry() {
        let nonce = SecretNonce::from_bytes([0x31; 32]);
        let mut plan = test_host_preparation_plan(&nonce);
        plan.plan_hash = host_preparation_plan_hash(&plan).expect("host plan hash");
        assert_eq!(
            host_preparation_plan_hash(&plan).expect("stable host plan hash"),
            plan.plan_hash
        );

        let mut changed = plan.clone();
        changed.host_preparation_revision += 1;
        assert_ne!(host_preparation_plan_hash(&changed).expect("revision hash"), plan.plan_hash);
        let mut changed = plan.clone();
        changed.runtime_instance_id = RuntimeInstanceId::new();
        assert_ne!(host_preparation_plan_hash(&changed).expect("runtime hash"), plan.plan_hash);
        let mut changed = plan.clone();
        changed.release_descriptor.identity.release_bundle_id = ReleaseBundleId::new();
        assert_ne!(host_preparation_plan_hash(&changed).expect("release hash"), plan.plan_hash);
        let mut changed = plan.clone();
        changed.release_descriptor.identity.component_manifest_hash =
            ContentHash::new(ContentHashAlgorithm::Sha256, "22".repeat(32))
                .expect("other manifest hash");
        assert_ne!(host_preparation_plan_hash(&changed).expect("manifest hash"), plan.plan_hash);
        let mut changed = plan.clone();
        changed.release_descriptor.bundle_namespace =
            "aura.desktop.test.other-plan".parse().expect("other namespace");
        assert_ne!(host_preparation_plan_hash(&changed).expect("namespace hash"), plan.plan_hash);
        let mut changed = plan.clone();
        changed.plan_nonce_hash = ContentHash::blake3(b"other nonce");
        assert_ne!(host_preparation_plan_hash(&changed).expect("nonce hash"), plan.plan_hash);
        let mut changed = plan.clone();
        changed.expires_at =
            UtcTimestamp::from_value(changed.expires_at.value() + time::Duration::seconds(1));
        assert_ne!(host_preparation_plan_hash(&changed).expect("expiry hash"), plan.plan_hash);
    }

    #[test]
    fn managed_user_networked_is_standard_user_and_explicitly_not_proxy_enforced() {
        let native = test_plan(NativeSandboxMode::ObservedNative);
        let mut plan = ExternalProcessLaunchPlan {
            execution_id: SandboxExecutionId::new(),
            identity: native.identity,
            permission_snapshot_id: native.permission_snapshot_id,
            source: aura_permission_domain::SecuritySource::McpServer {
                server_id: "networked-fixture".to_owned(),
            },
            source_fingerprint: ContentHash::blake3(b"trusted networked external fixture"),
            isolation: ExternalIsolationMode::ManagedUserNetworked,
            network_access: NetworkAccessRequirement::Required,
            network_enforcement: NetworkEnforcementKind::ObservedNativeNotProxyEnforced,
            executable: native.executable,
            args: ["/d", "/s", "/c", "ping.exe -n 3 127.0.0.1 >nul"]
                .map(|value| WindowsOsString { utf16: value.encode_utf16().collect() })
                .to_vec(),
            working_directory: native.working_directory,
            environment: native.environment,
            timeout_ms: native.timeout_ms,
            output_limits: native.output_limits,
            cancellation_id: native.cancellation_id,
            plan_hash: ContentHash::blake3(b"uninitialized"),
        };
        plan.plan_hash = external_process_plan_hash(&plan).expect("external plan hash");
        let trust = trusted_external_record(&plan);

        let production_plan =
            sandbox_plan_for_external_process(&plan, &trust).expect("production Host plan");
        assert_eq!(production_plan.mode, NativeSandboxMode::ObservedNativeNetworked);
        assert_eq!(
            production_plan.network_enforcement,
            NetworkEnforcementKind::ObservedNativeNotProxyEnforced
        );
        assert!(production_plan.network_grant_ids.is_empty());
        assert_eq!(
            plan_hash(&production_plan).expect("production plan hash"),
            production_plan.plan_hash
        );

        let process =
            launch_external_process(&plan, &trust).expect("launch networked managed user");
        assert_eq!(process.isolation(), ExternalIsolationMode::ManagedUserNetworked);
        assert_eq!(
            process.network_enforcement(),
            NetworkEnforcementKind::ObservedNativeNotProxyEnforced
        );
        assert!(!process.token_is_appcontainer().expect("AppContainer token state"));
        assert!(!process.token_is_elevated().expect("elevation token state"));
        process.terminate_tree(0xC000_013A).expect("terminate external process tree");
        process.close().expect("close external process");
    }

    #[test]
    fn host_preparation_plan_nonce_is_exact_and_expired_plan_fails_closed() {
        let nonce = SecretNonce::from_bytes([0x41; 32]);
        let mut plan = test_host_preparation_plan(&nonce);
        validate_host_preparation_plan_nonce(&plan, &nonce).expect("valid plan nonce");
        assert!(matches!(
            validate_host_preparation_plan_nonce(&plan, &SecretNonce::from_bytes([0x42; 32]),),
            Err(SandboxError::HostPreparationPlanNonceInvalid)
        ));

        plan.expires_at =
            UtcTimestamp::from_value(UtcTimestamp::now().value() - time::Duration::seconds(1));
        plan.plan_hash = host_preparation_plan_hash(&plan).expect("expired plan hash");
        assert!(matches!(
            execute_host_preparation_broker_plan(&plan),
            Err(SandboxError::HostPreparationPlanExpired)
        ));
    }

    #[test]
    fn elevation_launch_ticket_replaces_plan_wait_ttl_only_after_timely_approval() {
        let plan_nonce = SecretNonce::from_bytes([0x51; 32]);
        let mut plan = test_host_preparation_plan(&plan_nonce);
        let approved_at =
            UtcTimestamp::from_value(UtcTimestamp::now().value() - time::Duration::seconds(2));
        plan.expires_at =
            UtcTimestamp::from_value(approved_at.value() + time::Duration::seconds(1));
        plan.plan_hash = host_preparation_plan_hash(&plan).expect("expired approved plan hash");
        let ticket = test_elevation_launch_ticket(&plan, approved_at);

        validate_elevation_launch_ticket(&plan, &ticket)
            .expect("ticket issued before Plan expiry remains valid during its own TTL");

        let mut expired = ticket.clone();
        expired.expires_at =
            UtcTimestamp::from_value(UtcTimestamp::now().value() - time::Duration::milliseconds(1));
        assert!(matches!(
            validate_elevation_launch_ticket(&plan, &expired),
            Err(SandboxError::ElevationLaunchTicketExpired)
        ));

        let mut overlong = ticket.clone();
        overlong.expires_at =
            UtcTimestamp::from_value(overlong.issued_at.value() + time::Duration::seconds(61));
        assert!(matches!(
            validate_elevation_launch_ticket(&plan, &overlong),
            Err(SandboxError::ElevationLaunchTicketInvalid)
        ));

        let mut mismatched = ticket;
        mismatched.plan_hash = ContentHash::blake3(b"different plan");
        assert!(matches!(
            validate_elevation_launch_ticket(&plan, &mismatched),
            Err(SandboxError::ElevationLaunchTicketInvalid)
        ));
    }

    #[test]
    fn elevated_helper_nonce_is_consumed_once_and_replay_is_rejected() {
        let nonce = SecretNonce::from_bytes([0x5a; 32]);
        let parent_runtime = RuntimeInstanceId::new();
        let helper_runtime = RuntimeInstanceId::new();
        let mut guard = ElevatedBrokerGuard::new(
            nonce.clone(),
            42,
            ContractVersion::CURRENT,
            parent_runtime,
            helper_runtime,
        );
        let hello = ElevatedHelperHello {
            protocol_version: ContractVersion::CURRENT,
            parent_process_id: 42,
            helper_process_id: 84,
            parent_runtime_instance_id: parent_runtime,
            helper_runtime_instance_id: helper_runtime,
        };
        guard.accept_hello(&hello).expect("first hello");
        let proof = ElevatedHelperChallengeProof { bootstrap_nonce: nonce };
        guard.accept_challenge_proof(&proof).expect("first proof");
        assert!(matches!(
            guard.accept_challenge_proof(&proof),
            Err(SandboxError::ElevatedHelperNonceConsumed)
        ));
    }

    #[test]
    fn invalid_elevated_helper_identity_does_not_consume_the_nonce() {
        let nonce = SecretNonce::from_bytes([0xa5; 32]);
        let parent_runtime = RuntimeInstanceId::new();
        let helper_runtime = RuntimeInstanceId::new();
        let mut guard = ElevatedBrokerGuard::new(
            nonce.clone(),
            42,
            ContractVersion::CURRENT,
            parent_runtime,
            helper_runtime,
        );
        let invalid = ElevatedHelperHello {
            protocol_version: ContractVersion::CURRENT,
            parent_process_id: 7,
            helper_process_id: 84,
            parent_runtime_instance_id: parent_runtime,
            helper_runtime_instance_id: helper_runtime,
        };
        assert!(matches!(
            guard.accept_hello(&invalid),
            Err(SandboxError::ElevatedHelperHandshakeInvalid)
        ));
        let valid = ElevatedHelperHello {
            protocol_version: ContractVersion::CURRENT,
            parent_process_id: 42,
            helper_process_id: 84,
            parent_runtime_instance_id: parent_runtime,
            helper_runtime_instance_id: helper_runtime,
        };
        guard.accept_hello(&valid).expect("valid retry");
        guard
            .accept_challenge_proof(&ElevatedHelperChallengeProof { bootstrap_nonce: nonce })
            .expect("valid proof");
    }

    fn run_appcontainer_network_probe_child(address: std::ffi::OsString) {
        let address =
            address.to_string_lossy().parse::<std::net::SocketAddr>().expect("probe address");
        let is_appcontainer =
            aura_windows_platform::current_process_is_appcontainer().expect("query lowbox token");
        let network_denied =
            std::net::TcpStream::connect_timeout(&address, Duration::from_secs(1)).is_err();
        println!(
            "AURA_APPCONTAINER_PROBE token_appcontainer={is_appcontainer} network_denied={network_denied}"
        );
        assert!(is_appcontainer, "probe child did not receive an AppContainer token");
        assert!(network_denied, "zero-capability AppContainer reached a loopback listener");
    }

    fn write_appcontainer_network_evidence_report(report: serde_json::Value) {
        let Some(path) = std::env::var_os("AURA_APPCONTAINER_NETWORK_EVIDENCE_REPORT") else {
            return;
        };
        let path = std::path::PathBuf::from(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create AppContainer evidence directory");
        }
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&report).expect("serialize AppContainer evidence report"),
        )
        .expect("write AppContainer evidence report");
    }

    #[test]
    fn creating_an_appcontainer_child_requires_local_app_data_in_the_block() {
        // The deterministic counter-example for the W7 gate failure: same mode, same
        // profile, same image, same arguments, same working directory — the only
        // difference is whether the block handed to the launch carries
        // `LOCALAPPDATA`. A custom block replaces the parent's environment, so the
        // caller having one does not count.
        let name = format!("Aura.Sandbox.LocalAppData.{}", std::process::id());
        let profile = AppContainerProfile::ensure(&name).expect("AppContainer profile");
        assert!(profile.was_created(), "this test needs a fresh profile: {name}");
        let directory = tempfile::tempdir().expect("temporary directory");
        let image = std::path::Path::new(r"C:\Windows\System32\cmd.exe");
        assert!(image.is_file(), "the probe image must exist");
        let arguments =
            ["/d", "/c", "exit"].map(|value| value.encode_utf16().collect::<Vec<u16>>()).to_vec();

        let without = match spawn_restricted_process(
            NativeRestrictedMode::AppContainer,
            Some(&profile),
            image,
            &arguments,
            directory.path(),
            &[],
        ) {
            Err(error) => error,
            Ok(process) => {
                drop(process);
                panic!("an AppContainer launch without LOCALAPPDATA must fail on this platform");
            }
        };
        let message = format!("{without} / {without:?}");
        assert!(
            message.contains("appcontainer_local_app_data_missing"),
            "the failure must name the platform requirement: {message}"
        );

        let with = spawn_restricted_process(
            NativeRestrictedMode::AppContainer,
            Some(&profile),
            image,
            &arguments,
            directory.path(),
            &[NativeEnvironmentEntry {
                name: "LOCALAPPDATA".to_owned(),
                value_utf16: directory.path().to_string_lossy().encode_utf16().collect(),
            }],
        )
        .expect("the same launch must succeed once the block carries LOCALAPPDATA");
        drop(with);
        let _ = delete_appcontainer_profile(&name);
    }

    #[test]
    #[ignore = "real Windows AppContainer evidence; run with `cargo xtask appcontainer-network-evidence`"]
    fn compatibility_appcontainer_has_lowbox_token_and_denies_loopback() {
        if let Some(address) = std::env::var_os("AURA_APPCONTAINER_NETWORK_PROBE") {
            run_appcontainer_network_probe_child(address);
            return;
        }

        let host = probe_host_compatibility().expect("host compatibility probe");
        if !host.system_drive_ready || !host_preparation_nul_ready() {
            eprintln!(
                "AURA_APPCONTAINER_EVIDENCE status=PendingHostPreparation gate_status=Inconclusive reason=host_preparation_unavailable readiness={:?}",
                aura_windows_platform::installation_identity::probe_null_device_readiness()
            );
            write_appcontainer_network_evidence_report(serde_json::json!({
                "schema_version": 1,
                "evidence_id": "appcontainer_lowbox_token_and_loopback_denial",
                "status": "PendingHostPreparation",
                "gate_status": "Inconclusive",
                "reason": "host_preparation_unavailable",
                "host_preparation_available": false,
                "required_assertions": {
                    "child_token_is_appcontainer_lowbox": null,
                    "loopback_connection_denied": null
                }
            }));
            panic!(
                "AURA_APPCONTAINER_EVIDENCE_UNAVAILABLE: Host Preparation is required for the real lowbox-token and loopback-denial probe"
            );
        }

        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .expect("bind loopback probe");
        listener.set_nonblocking(true).expect("nonblocking listener");
        let address = listener.local_addr().expect("listener address");
        let directory = tempfile::tempdir().expect("temporary sandbox fixture");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace fixture");
        let probe_executable = workspace.join("aura-appcontainer-network-probe.exe");
        std::fs::copy(std::env::current_exe().expect("current test executable"), &probe_executable)
            .expect("copy AppContainer probe executable");

        let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
            directory.path().join("aura.db"),
            directory.path().join("artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("start persistence")
        {
            PersistenceStartup::Ready(value) => value,
            PersistenceStartup::RecoveryRequired(_) => panic!("unexpected migration recovery"),
        };
        let workspace_id = WorkspaceId::new();
        let session_id = SessionId::new();
        persistence
            .upsert_workspace_session(
                WorkspaceSummary {
                    workspace_id,
                    display_name: "AppContainer Network Probe".to_owned(),
                    selected_root: workspace.display().to_string(),
                    canonical_root: workspace.display().to_string(),
                    root_identity: None,
                    state: WorkspaceState::Online,
                    revision: Revision::INITIAL,
                    catalog_freshness: CatalogFreshness::Unavailable,
                    authorization_epoch: Revision::INITIAL,
                },
                SessionSummary {
                    session_id,
                    workspace_id,
                    title: "Sandbox Network Probe".to_owned(),
                    state: "idle".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("seed workspace and session");
        persistence
            .ensure_session_permission_state(
                session_id,
                workspace_id,
                aura_security_contract::PermissionMode::WorkspaceReadWrite,
                CorrelationId::new(),
            )
            .expect("permission state");
        let snapshot = persistence
            .create_permission_snapshot(session_id, None, CorrelationId::new())
            .expect("permission snapshot");
        let executor = SandboxExecutor::new(
            persistence,
            CancellationRegistry::default(),
            SandboxExecutorOptions::default(),
        );
        let mut plan = test_plan(NativeSandboxMode::CompatibilityAppContainer);
        plan.identity = aura_security_contract::InvocationIdentity::runtime_user(
            workspace_id,
            session_id,
            CorrelationId::new(),
        );
        plan.identity.permission_snapshot_id = Some(snapshot.id);
        plan.permission_snapshot_id = snapshot.id;
        plan.executable = snapshot_executable_ref(&probe_executable, ContentHashAlgorithm::Blake3)
            .expect("snapshot probe executable");
        plan.args = [
            "--ignored",
            "--exact",
            "tests::compatibility_appcontainer_has_lowbox_token_and_denies_loopback",
            "--nocapture",
        ]
        .map(|value| WindowsOsString { utf16: value.encode_utf16().collect() })
        .to_vec();
        plan.working_directory = canonical_path(&workspace);
        // The probe's own marker, plus the one variable the platform requires:
        // creating an AppContainer child resolves the container's local storage
        // through `LOCALAPPDATA` **in the block it is handed**, and a custom block
        // replaces the parent's environment entirely. Without it the launch fails
        // with ERROR_ENVVAR_NOT_FOUND (203) before the child image is considered.
        // Real plans are built by `aura-backend-windows-user`'s baseline, which
        // sets it to Aura's own work directory — never to the launching user's —
        // so this fixture states the same shape instead of borrowing the caller's.
        plan.environment = vec![
            SanitizedEnvironmentEntry {
                name: "AURA_APPCONTAINER_NETWORK_PROBE".to_owned(),
                value: WindowsOsString { utf16: address.to_string().encode_utf16().collect() },
            },
            SanitizedEnvironmentEntry {
                name: "LOCALAPPDATA".to_owned(),
                value: WindowsOsString {
                    utf16: workspace.to_string_lossy().encode_utf16().collect(),
                },
            },
        ];
        plan.filesystem_grants = vec![
            SandboxFilesystemGrant {
                path: canonical_path(&workspace),
                identity: None,
                access: SandboxFilesystemAccess::Read,
                recursive: true,
            },
            SandboxFilesystemGrant {
                path: canonical_path(&probe_executable),
                identity: None,
                access: SandboxFilesystemAccess::Read,
                recursive: false,
            },
        ];
        plan.timeout_ms = 10_000;
        plan.plan_hash = plan_hash(&plan).expect("plan hash");
        let mut events = Vec::new();
        let outcome = executor.execute(plan, |event| events.push(event)).expect("execute probe");
        assert_eq!(outcome.exit_code, Some(0), "AppContainer probe child failed");
        let output = events
            .iter()
            .filter_map(|event| match event {
                SandboxHostEvent::StdoutChunk { bytes, .. } => Some(bytes.as_slice()),
                _ => None,
            })
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        let output = String::from_utf8_lossy(&output);
        assert!(
            output.contains("AURA_APPCONTAINER_PROBE token_appcontainer=true network_denied=true"),
            "probe did not prove both lowbox token and network denial: {output}"
        );
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "loopback listener unexpectedly accepted an AppContainer connection"
        );
        write_appcontainer_network_evidence_report(serde_json::json!({
            "schema_version": 1,
            "evidence_id": "appcontainer_lowbox_token_and_loopback_denial",
            "status": "passed",
            "gate_status": "passed",
            "reason": null,
            "host_preparation_available": true,
            "required_assertions": {
                "child_token_is_appcontainer_lowbox": true,
                "loopback_connection_denied": true
            }
        }));
    }

    #[test]
    fn compatibility_appcontainer_runs_and_restores_its_acl_journal() {
        let host = probe_host_compatibility().expect("host compatibility probe");
        if !host.system_drive_ready || !host_preparation_nul_ready() {
            eprintln!(
                "SKIPPED compatibility_appcontainer_runs_and_restores_its_acl_journal: Host Preparation is unavailable ({:?})",
                aura_windows_platform::installation_identity::probe_null_device_readiness()
            );
            let mut plan = test_plan(NativeSandboxMode::CompatibilityAppContainer);
            plan.plan_hash = plan_hash(&plan).expect("plan hash");
            let directory = tempfile::tempdir().expect("temporary sandbox fixture");
            let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
                directory.path().join("aura.db"),
                directory.path().join("artifacts"),
                RuntimeInstanceId::new(),
            ))
            .expect("start persistence")
            {
                PersistenceStartup::Ready(value) => value,
                PersistenceStartup::RecoveryRequired(_) => panic!("unexpected migration recovery"),
            };
            let executor = SandboxExecutor::new(
                persistence,
                CancellationRegistry::default(),
                SandboxExecutorOptions::default(),
            );
            assert!(matches!(
                executor.execute(plan, |_| {}),
                Err(SandboxError::HostPreparationRequired)
            ));
            return;
        }
        let directory = tempfile::tempdir().expect("temporary sandbox fixture");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace fixture");
        let before = capture_file_dacl(&workspace).expect("DACL before execution");
        let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
            directory.path().join("aura.db"),
            directory.path().join("artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("start persistence")
        {
            PersistenceStartup::Ready(value) => value,
            PersistenceStartup::RecoveryRequired(_) => panic!("unexpected migration recovery"),
        };
        let workspace_id = WorkspaceId::new();
        let session_id = SessionId::new();
        persistence
            .upsert_workspace_session(
                WorkspaceSummary {
                    workspace_id,
                    display_name: "Sandbox Fixture".to_owned(),
                    selected_root: workspace.display().to_string(),
                    canonical_root: workspace.display().to_string(),
                    root_identity: None,
                    state: WorkspaceState::Online,
                    revision: Revision::INITIAL,
                    catalog_freshness: CatalogFreshness::Unavailable,
                    authorization_epoch: Revision::INITIAL,
                },
                SessionSummary {
                    session_id,
                    workspace_id,
                    title: "Sandbox".to_owned(),
                    state: "idle".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("seed workspace and session");
        persistence
            .ensure_session_permission_state(
                session_id,
                workspace_id,
                aura_security_contract::PermissionMode::WorkspaceReadWrite,
                CorrelationId::new(),
            )
            .expect("permission state");
        let snapshot = persistence
            .create_permission_snapshot(session_id, None, CorrelationId::new())
            .expect("permission snapshot");
        let cancellation = CancellationRegistry::default();
        let executor = SandboxExecutor::new(
            persistence.clone(),
            cancellation,
            SandboxExecutorOptions::default(),
        );
        let mut plan = test_plan(NativeSandboxMode::CompatibilityAppContainer);
        plan.identity = aura_security_contract::InvocationIdentity::runtime_user(
            workspace_id,
            session_id,
            CorrelationId::new(),
        );
        plan.identity.permission_snapshot_id = Some(snapshot.id);
        plan.permission_snapshot_id = snapshot.id;
        plan.args = ["/d", "/s", "/c", "echo aura-appcontainer"]
            .map(|value| WindowsOsString { utf16: value.encode_utf16().collect() })
            .to_vec();
        plan.environment = [
            ("ComSpec", r"C:\Windows\System32\cmd.exe"),
            // Creating an AppContainer child resolves the container's local storage
            // through `LOCALAPPDATA` in the block it is handed; a custom block
            // replaces the parent environment, so this plan has to state it. The
            // production baseline points it at Aura's own work directory and never
            // at the launching user's.
            ("LOCALAPPDATA", workspace.to_str().expect("UTF-8 temporary path")),
            ("PATHEXT", ".COM;.EXE;.BAT;.CMD"),
            ("SystemRoot", r"C:\Windows"),
            ("TEMP", workspace.to_str().expect("UTF-8 temporary path")),
            ("TMP", workspace.to_str().expect("UTF-8 temporary path")),
        ]
        .into_iter()
        .map(|(name, value)| SanitizedEnvironmentEntry {
            name: name.to_owned(),
            value: WindowsOsString { utf16: value.encode_utf16().collect() },
        })
        .collect();
        plan.working_directory = canonical_path(std::path::Path::new(r"C:\Windows"));
        plan.filesystem_grants = vec![SandboxFilesystemGrant {
            path: canonical_path(&workspace),
            identity: None,
            access: SandboxFilesystemAccess::ReadWrite,
            recursive: true,
        }];
        plan.plan_hash = plan_hash(&plan).expect("plan hash");
        let mut events = Vec::new();
        let outcome = executor.execute(plan, |event| events.push(event)).expect("execute plan");
        assert_eq!(outcome.state, SandboxExecutionState::Exited);
        assert!(events.iter().any(|event| matches!(
            event,
            SandboxHostEvent::StdoutChunk { bytes, .. }
                if String::from_utf8_lossy(bytes).contains("aura-appcontainer")
        )));
        assert!(
            persistence.pending_sandbox_acl_cleanup().expect("pending ACL journals").is_empty()
        );
        let after = capture_file_dacl(&workspace).expect("DACL after execution");
        assert_eq!(before, after, "Aura must remove only the ACE added for this execution");
    }

    #[test]
    fn the_attempt_local_environment_is_sealed_into_the_plan_hash() {
        // A launch block that replaces the parent environment is part of what was
        // authorized: the host recomputes this hash before it starts a child, so a
        // tampered injected value cannot keep passing as the approved plan.
        let mut plan = test_plan(NativeSandboxMode::CompatibilityAppContainer);
        plan.environment
            .extend(aura_permission_domain::attempt_local_environment(r"C:\Aura\attempt-1"));
        plan.plan_hash = plan_hash(&plan).expect("sealed plan hash");
        let sealed = plan.plan_hash.clone();

        let local_app_data = plan
            .environment
            .iter_mut()
            .find(|entry| entry.name.eq_ignore_ascii_case("LOCALAPPDATA"))
            .expect("the attempt-local binding is present");
        local_app_data.value =
            WindowsOsString { utf16: r"C:\Aura\elsewhere".encode_utf16().collect() };

        assert_ne!(plan_hash(&plan).expect("tampered hash"), sealed);
        assert_ne!(
            plan_hash(&plan).expect("tampered hash"),
            plan.plan_hash,
            "the host-side check must reject a plan whose contents no longer match its hash"
        );
    }

    pub(crate) fn test_host_preparation_plan(nonce: &SecretNonce) -> HostPreparationBrokerPlan {
        let bundle_namespace: BundleNamespace =
            "aura.desktop.test.sandbox".parse().expect("test namespace");
        let mut plan = HostPreparationBrokerPlan {
            operation_id: aura_domain_common::HostPreparationOperationId::new(),
            action: HostPreparationAction::Prepare,
            host_preparation_revision: 7,
            runtime_instance_id: RuntimeInstanceId::new(),
            release_descriptor: ReleaseDescriptor {
                bundle_namespace,
                identity: ReleaseIdentity {
                    release_bundle_id: ReleaseBundleId::new(),
                    component_manifest_hash: ContentHash::new(
                        ContentHashAlgorithm::Sha256,
                        "11".repeat(32),
                    )
                    .expect("manifest hash"),
                },
            },
            plan_nonce_hash: ContentHash::blake3(nonce.expose().as_bytes()),
            expires_at: UtcTimestamp::from_value(
                UtcTimestamp::now().value() + time::Duration::minutes(3),
            ),
            system_drive_root: CanonicalPath {
                display: r"C:\".to_owned(),
                utf16: std::ffi::OsStr::new(r"C:\").encode_wide().collect(),
            },
            ownership_before: HostPreparationOwnership::default(),
            add_all_application_packages_ace: true,
            add_all_restricted_application_packages_ace: true,
            update_null_device_security: false,
            plan_hash: ContentHash::blake3(&[]),
        };
        plan.plan_hash = host_preparation_plan_hash(&plan).expect("host preparation plan hash");
        plan
    }

    fn test_elevation_launch_ticket(
        plan: &HostPreparationBrokerPlan,
        issued_at: UtcTimestamp,
    ) -> ElevationLaunchTicket {
        ElevationLaunchTicket {
            ticket_id: ElevationLaunchTicketId::new(),
            approval_id: ApprovalRequestId::new(),
            operation_id: plan.operation_id,
            plan_hash: plan.plan_hash.clone(),
            runtime_instance_id: plan.runtime_instance_id,
            release_descriptor: plan.release_descriptor.clone(),
            issued_at,
            expires_at: UtcTimestamp::from_value(issued_at.value() + time::Duration::seconds(60)),
            ticket_nonce: SecretNonce::from_bytes([0x61; 32]),
        }
    }

    fn canonical_path(path: &std::path::Path) -> CanonicalPath {
        CanonicalPath {
            display: path.display().to_string(),
            utf16: path.as_os_str().encode_wide().collect(),
        }
    }

    fn trusted_external_record(plan: &ExternalProcessLaunchPlan) -> ExternalTrustRecord {
        ExternalTrustRecord {
            id: aura_domain_common::TrustRecordId::new(),
            source: plan.source.clone(),
            state: TrustState::Trusted {
                fingerprint: plan.source_fingerprint.clone(),
                trusted_at: UtcTimestamp::now(),
            },
            effective_fingerprint: Some(plan.source_fingerprint.clone()),
            updated_at: UtcTimestamp::now(),
        }
    }

    fn test_plan(mode: NativeSandboxMode) -> SandboxExecutionPlan {
        let workspace_id = aura_domain_common::WorkspaceId::new();
        let session_id = aura_domain_common::SessionId::new();
        let identity = aura_security_contract::InvocationIdentity::runtime_user(
            workspace_id,
            session_id,
            CorrelationId::new(),
        );
        SandboxExecutionPlan {
            execution_id: SandboxExecutionId::new(),
            identity,
            permission_snapshot_id: aura_domain_common::PermissionSnapshotId::new(),
            mode,
            network_access: if mode == NativeSandboxMode::ObservedNativeNetworked {
                NetworkAccessRequirement::Required
            } else {
                NetworkAccessRequirement::NotRequired
            },
            network_enforcement: validate_native_network_policy(
                mode,
                if mode == NativeSandboxMode::ObservedNativeNetworked {
                    NetworkAccessRequirement::Required
                } else {
                    NetworkAccessRequirement::NotRequired
                },
                &[],
            )
            .expect("test network policy"),
            executable: snapshot_executable_ref(
                std::path::Path::new(r"C:\Windows\System32\cmd.exe"),
                ContentHashAlgorithm::Blake3,
            )
            .expect("snapshot cmd.exe"),
            args: vec![],
            working_directory: CanonicalPath {
                display: r"C:\Windows".to_owned(),
                utf16: r"C:\Windows".encode_utf16().collect(),
            },
            environment: vec![],
            filesystem_grants: vec![],
            network_grant_ids: vec![],
            timeout_ms: 5_000,
            output_limits: ProcessOutputLimits {
                stdout_bytes: 1024,
                stderr_bytes: 1024,
                combined_bytes: 2048,
            },
            cancellation_id: CancellationId::new(),
            plan_hash: ContentHash::blake3(b"initial"),
        }
    }
}
