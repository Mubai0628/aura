use aura_domain_common::{
    PermissionGrantId, PermissionSnapshotId, SessionId, UtcTimestamp, WorkspaceId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    #[default]
    ReadOnly,
    WorkspaceReadWrite,
    FullAccess,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionSnapshot {
    pub id: PermissionSnapshotId,
    pub session_id: SessionId,
    pub workspace_id: WorkspaceId,
    pub mode: PermissionMode,
    pub session_permission_revision: u64,
    pub active_grant_ids: Vec<PermissionGrantId>,
    pub created_at: UtcTimestamp,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedCapability {
    WorkspaceFileRead,
    WorkspaceFileWrite,
    WorkspaceFileCreate,
    WorkspaceFileDelete,
    WorkspaceFileMove,
    ExternalFileRead,
    ExternalFileWrite,
    ExternalFileCreate,
    ExternalFileDelete,
    ProcessExecute,
    InteractiveTerminal,
    BackgroundProcess,
    GitRead,
    GitWrite,
    GitRemoteRead,
    GitRemoteWrite,
    GitDestructive,
    /// Dedicated HTTP/Web networking. Legacy network capabilities retain their old meaning.
    OrdinaryNetwork,
    PublicNetworkRead,
    PrivateNetworkAccess,
    DataEgress,
    DownloadArtifact,
    DownloadWindowsExecutable,
    ExternalServiceRead,
    ExternalServiceMutation,
    ExternalServiceDestructiveMutation,
    BrowserRead,
    BrowserInteraction,
    BrowserSensitiveAction,
    ConfigurationRead,
    ConfigurationWrite,
    RegistryWrite,
    SystemEnvironmentWrite,
    SystemFileWrite,
    ServiceManagement,
    ScheduledTaskManagement,
    DriverManagement,
    WindowsSoftwareInstall,
    ElevatedOperation,
    ToolRegistryRead,
    ToolEphemeralExposureWrite,
    /// Task-local bookkeeping and completion gates; never workspace mutation.
    AgentTaskRead,
    AgentTaskWrite,
    MemoryRead,
    MemoryWrite,
    MemoryDelete,
    /// A call to a trusted external extension. Aura does not
    /// infer what it does: ReadOnly asks per server per turn, the other modes do not ask.
    ExternalExtensionCall,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivateNetworkClass {
    Loopback,
    Private,
    LinkLocal,
    Metadata,
    Reserved,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentVariableScope {
    Process,
    User,
    Machine,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalPath {
    pub display: String,
    pub utf16: Vec<u16>,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum OperationRiskFlag {
    UnknownSideEffect,
    OutsideWorkspaceWrite,
    OutsideWorkspaceDelete,
    SensitivePathAccess,
    DataLeavesDevice,
    PrivateOrLoopbackNetwork,
    ExecutableArtifact,
    SoftwareInstallation,
    SystemConfigurationMutation,
    Elevation,
    DriverOrServiceMutation,
    DestructiveRemoteMutation,
    SensitiveBrowserAction,
    GitBareForcePush,
    GitSequencerSkipOrQuit,
    InlineScript,
    UntrustedSource,
    /// A deletion that does not go to the Recycle Bin — a Shell command that
    /// deletes files, or `fs.delete` asked to delete for good. Asked every time, never reused;
    /// a Shell deletion in 完全访问 is not asked (its user was told when turning it on).
    PermanentFileDeletion,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionGrantLifetime {
    Once,
    TurnTask,
    Session,
    Workspace,
}

/// Stable evidence describing why P04 permitted one operation. This is kept
/// separate from `PermissionDecision` so the existing Renderer-facing
/// decision wire remains unchanged while Core can bind the exact authority to
/// a one-shot Runtime dispatch.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "basis_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PermissionAllowAuthorizationBasisV1 {
    PermissionMode { mode: PermissionMode },
    PermissionGrant { permission_grant_id: PermissionGrantId, lifetime: PermissionGrantLifetime },
}
