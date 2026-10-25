//! Current acceptance facts, separate from the historical r06-host-v0 report.
use aura_domain_common::{ContentHash, UtcTimestamp, jcs::CanonicalSha256HashV1};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const EXECUTION_HOST_ACCEPTANCE_VERSION: &str = "windows-user-host-acceptance-v3";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionHostEdition {
    Home,
    Pro,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionCheckResult {
    Passed,
    Failed,
    NotRun,
    AcceptedKnownLimitation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionAcceptanceStage {
    R06,
    R07,
    R08,
    R09,
}

impl ExecutionAcceptanceStage {
    pub const ALL: &'static [Self] = &[Self::R06, Self::R07, Self::R08, Self::R09];

    #[must_use]
    pub const fn required_cases(self) -> &'static [ExecutionAcceptanceCase] {
        match self {
            Self::R06 => ExecutionAcceptanceCase::R06,
            Self::R07 => ExecutionAcceptanceCase::R07,
            Self::R08 => ExecutionAcceptanceCase::R08,
            Self::R09 => ExecutionAcceptanceCase::R09,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionAcceptanceCase {
    // Restricted User Offline Runner and host lifecycle.
    SourceBuildInstallBinding,
    ManagedPowerShell,
    SystemCmd,
    FixedEnvironment,
    StdinStdoutStderr,
    FileRead,
    WorkspaceWrite,
    ReadOnlyWriteDenied,
    PathEscapeDenied,
    CredentialCanaryDenied,
    RestrictedIdentity,
    ChildJobInheritance,
    CancelTreeStopped,
    OutputLimit,
    Timeout,
    RepeatedDispatchDenied,
    StaleAuthorityDenied,
    ResultUniqueTerminal,
    PostDispatchCrashUnknown,
    CoreRestartRecovery,
    SystemRebootRecovery,
    MaintenanceConfigurationProtected,
    OwnerSessionIsolation,
    UnprivilegedAfterReboot,
    DirectTcpV4Blocked,
    DirectUdpV4Blocked,
    DirectTcpV6Blocked,
    DirectUdpV6Blocked,
    DnscacheKnownLimitation,
    MainUserNetworkUnaffected,
    InstallationRepair,
    InstallationRollback,
    InstallationRemove,

    // Online, NativeApproved, and Controlled Web.
    OnlineTcpV4Allowed,
    OnlineUdpV4Allowed,
    OnlineTcpV6Allowed,
    OnlineUdpV6Allowed,
    OnlineSyntheticUpload,
    OfflineNetworkPositiveControl,
    OfflineNetworkNegativeControl,
    OfflineOnlineIdentityIsolation,
    NativeExplicitApproval,
    NativeIsolationReduced,
    OldDispatchBindingDenied,
    ControlledWebExactOrigin,
    ControlledWebRedirect,
    ControlledWebLimits,
    ControlledWebReflection,

    // Tool Core and execution orchestration.
    BuiltInRegistry,
    StableRegistrySnapshot,
    StableExecutionHistory,
    BatchAdmission,
    DependencyScheduling,
    LockScheduling,
    ToolAttemptLifecycle,
    ToolResultLifecycle,
    ToolSideEffectClassification,
    HistoryAtomicGroup,
    AgentWithoutPackageEditor,

    // Production Agent loop.
    CoreRouteBrokerToolLoop,
    CoreRouteOfflineToolLoop,
    TextOnlyCompletion,
    SequentialToolExecution,
    ParallelToolExecution,
    ToolRejection,
    NonZeroExit,
    ToolStartFailure,
    Cancellation,
    UnknownResult,
    Recovery,
    WithoutProviderContinuation,
    Compaction,
    TodoLifecycle,
    PlanLifecycle,
    GoalLifecycle,
    TaskLineageLifecycle,
    P06HistoryReconstruction,
    BackgroundQuery,
    BackgroundStop,
    BackgroundNormalCompletion,
    BackgroundCrashRecovery,
    ImmutablePagination,
    ElectronMainCoreProductionChain,
}

impl ExecutionAcceptanceCase {
    pub const R06: &'static [Self] = &[
        Self::SourceBuildInstallBinding,
        Self::ManagedPowerShell,
        Self::SystemCmd,
        Self::FixedEnvironment,
        Self::StdinStdoutStderr,
        Self::FileRead,
        Self::WorkspaceWrite,
        Self::ReadOnlyWriteDenied,
        Self::PathEscapeDenied,
        Self::CredentialCanaryDenied,
        Self::RestrictedIdentity,
        Self::ChildJobInheritance,
        Self::CancelTreeStopped,
        Self::OutputLimit,
        Self::Timeout,
        Self::RepeatedDispatchDenied,
        Self::StaleAuthorityDenied,
        Self::ResultUniqueTerminal,
        Self::PostDispatchCrashUnknown,
        Self::CoreRestartRecovery,
        Self::SystemRebootRecovery,
        Self::MaintenanceConfigurationProtected,
        Self::OwnerSessionIsolation,
        Self::UnprivilegedAfterReboot,
        Self::DirectTcpV4Blocked,
        Self::DirectUdpV4Blocked,
        Self::DirectTcpV6Blocked,
        Self::DirectUdpV6Blocked,
        Self::DnscacheKnownLimitation,
        Self::MainUserNetworkUnaffected,
        Self::InstallationRepair,
        Self::InstallationRollback,
        Self::InstallationRemove,
    ];

    pub const R07: &'static [Self] = &[
        Self::OnlineTcpV4Allowed,
        Self::OnlineUdpV4Allowed,
        Self::OnlineTcpV6Allowed,
        Self::OnlineUdpV6Allowed,
        Self::OnlineSyntheticUpload,
        Self::OfflineNetworkPositiveControl,
        Self::OfflineNetworkNegativeControl,
        Self::OfflineOnlineIdentityIsolation,
        Self::NativeExplicitApproval,
        Self::NativeIsolationReduced,
        Self::OldDispatchBindingDenied,
        Self::ControlledWebExactOrigin,
        Self::ControlledWebRedirect,
        Self::ControlledWebLimits,
        Self::ControlledWebReflection,
    ];

    pub const R08: &'static [Self] = &[
        Self::BuiltInRegistry,
        Self::StableRegistrySnapshot,
        Self::StableExecutionHistory,
        Self::BatchAdmission,
        Self::DependencyScheduling,
        Self::LockScheduling,
        Self::ToolAttemptLifecycle,
        Self::ToolResultLifecycle,
        Self::ToolSideEffectClassification,
        Self::HistoryAtomicGroup,
        Self::AgentWithoutPackageEditor,
    ];

    pub const R09: &'static [Self] = &[
        Self::CoreRouteBrokerToolLoop,
        Self::CoreRouteOfflineToolLoop,
        Self::TextOnlyCompletion,
        Self::SequentialToolExecution,
        Self::ParallelToolExecution,
        Self::ToolRejection,
        Self::NonZeroExit,
        Self::ToolStartFailure,
        Self::Cancellation,
        Self::UnknownResult,
        Self::Recovery,
        Self::WithoutProviderContinuation,
        Self::Compaction,
        Self::TodoLifecycle,
        Self::PlanLifecycle,
        Self::GoalLifecycle,
        Self::TaskLineageLifecycle,
        Self::P06HistoryReconstruction,
        Self::BackgroundQuery,
        Self::BackgroundStop,
        Self::BackgroundNormalCompletion,
        Self::BackgroundCrashRecovery,
        Self::ImmutablePagination,
        Self::ElectronMainCoreProductionChain,
    ];

    /// R06's named DNS limitation is the only non-passing result accepted by
    /// any required-case set.
    #[must_use]
    pub const fn accepts(self, result: ExecutionCheckResult) -> bool {
        match self {
            Self::DnscacheKnownLimitation => {
                matches!(result, ExecutionCheckResult::AcceptedKnownLimitation)
            }
            _ => matches!(result, ExecutionCheckResult::Passed),
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionAcceptanceObservation {
    pub case: ExecutionAcceptanceCase,
    pub result: ExecutionCheckResult,
    /// Report-relative evidence path, never a raw command or credential.
    pub evidence_path: String,
    pub evidence_sha256: ContentHash,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionHostAcceptanceReport {
    pub schema_version: aura_domain_common::ToolWireVersionV1,
    pub acceptance_version: String,
    pub assurance_version: String,
    pub edition: ExecutionHostEdition,
    pub windows_build: String,
    pub observed_at: UtcTimestamp,
    /// SHA-256 of the immutable candidate's bundle-inventory.json bytes.
    pub candidate_inventory_sha256: ContentHash,
    pub source_fingerprint: CanonicalSha256HashV1,
    pub installation_fingerprint: CanonicalSha256HashV1,
    pub core_sha256: ContentHash,
    pub runner_sha256: ContentHash,
    pub helper_sha256: ContentHash,
    pub maintainer_sha256: ContentHash,
    pub powershell_manifest_sha256: ContentHash,
    pub cmd_image: super::ExecutionFileBinding,
    pub observations: Vec<ExecutionAcceptanceObservation>,
}

impl ExecutionHostAcceptanceReport {
    /// Required observations are exact and unique within the requested stage.
    /// A duplicate is an unresolved case even if both observations say passed.
    #[must_use]
    pub fn missing_or_failed_for(
        &self,
        stage: ExecutionAcceptanceStage,
    ) -> Vec<ExecutionAcceptanceCase> {
        stage
            .required_cases()
            .iter()
            .copied()
            .filter(|case| {
                let mut found = self.observations.iter().filter(|item| item.case == *case);
                !matches!(
                    (found.next(), found.next()),
                    (Some(observation), None) if case.accepts(observation.result)
                )
            })
            .collect()
    }

    #[must_use]
    pub fn is_complete_for(&self, stage: ExecutionAcceptanceStage) -> bool {
        self.missing_or_failed_for(stage).is_empty()
    }
}
