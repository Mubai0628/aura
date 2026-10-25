use aura_domain_common::{
    ApprovalRequestId, CancellationId, ContentHash, ContractVersion, ElevationLaunchTicketId,
    FileIdentity, HostPreparationOperationId, NetworkGrantId, PermissionSnapshotId,
    RuntimeInstanceId, SandboxAclJournalId, SandboxExecutionId, SecretNonce, UtcTimestamp,
};
use aura_runtime_contract::ReleaseDescriptor;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ApprovalView, SecuritySource};
use aura_security_contract::{ApprovalDecisionKind, CanonicalPath, InvocationIdentity};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeSandboxMode {
    CompatibilityAppContainer,
    StrictAppContainer,
    ObservedNative,
    ObservedNativeNetworked,
    ElevatedBrokered,
}

/// Frozen requirement profile for an uninstalled Draft Conformance process.
/// The selected P04 implementation must satisfy this profile exactly; there
/// is no ObservedNative or networked fallback.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConformanceSandboxProfileV1 {
    HermeticNoNetworkV1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalIsolationMode {
    CompatibilityManagedUser,
    StrictAppContainer,
    NativeManagedUser,
    ManagedUserNetworked,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkAccessRequirement {
    #[default]
    NotRequired,
    Required,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkEnforcementKind {
    CoreControlPlane,
    AppContainerNetworkDenied,
    ObservedNativeNotProxyEnforced,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxNetworkPolicyErrorCode {
    NetworkAccessDeclarationRequired,
    AppContainerNetworkDenied,
    NetworkGrantUnavailableInSelectedMode,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "mode_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkExecutionMode {
    Native { mode: NativeSandboxMode },
    External { mode: ExternalIsolationMode },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxNetworkPolicyError {
    pub code: SandboxNetworkPolicyErrorCode,
    pub declared_access: NetworkAccessRequirement,
    pub selected_mode: NetworkExecutionMode,
    pub required_declaration: NetworkAccessRequirement,
    pub allowed_networked_modes: Vec<NetworkExecutionMode>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalExecutableRef {
    pub path: CanonicalPath,
    pub identity: FileIdentity,
    pub content_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsOsString {
    pub utf16: Vec<u16>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SanitizedEnvironmentEntry {
    pub name: String,
    pub value: WindowsOsString,
}

/// The attempt-local environment entries Core owns, bound to one attempt's own
/// directory.
///
/// `TEMP`/`TMP` keep a Tool runtime's scratch inside the attempt directory. The
/// AppContainer launcher additionally reads `LOCALAPPDATA` **from the block it is
/// handed** while it creates the child — the block replaces the parent environment
/// entirely — so a plan without it cannot start a child at all
/// (`ERROR_ENVVAR_NOT_FOUND`, 203) and a package must not be able to choose it.
/// Both the authorizing runtime path and Aura's conformance adapter bind these the
/// same way, from this one function.
#[must_use]
pub fn attempt_local_environment(directory: &str) -> [SanitizedEnvironmentEntry; 3] {
    let value = WindowsOsString { utf16: directory.encode_utf16().collect() };
    [
        SanitizedEnvironmentEntry { name: "TEMP".to_owned(), value: value.clone() },
        SanitizedEnvironmentEntry { name: "TMP".to_owned(), value: value.clone() },
        SanitizedEnvironmentEntry { name: "LOCALAPPDATA".to_owned(), value },
    ]
}

/// Whether an entry is one of the attempt-local names above. They are compared
/// against the attempt directory instead of the frozen persistent environment.
#[must_use]
pub fn is_attempt_local_environment_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("TEMP")
        || name.eq_ignore_ascii_case("TMP")
        || name.eq_ignore_ascii_case("LOCALAPPDATA")
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxFilesystemAccess {
    Read,
    ReadWrite,
    Create,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxFilesystemGrant {
    pub path: CanonicalPath,
    pub identity: Option<FileIdentity>,
    pub access: SandboxFilesystemAccess,
    pub recursive: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessOutputLimits {
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub combined_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxExecutionPlan {
    pub execution_id: SandboxExecutionId,
    pub identity: InvocationIdentity,
    pub permission_snapshot_id: PermissionSnapshotId,
    pub mode: NativeSandboxMode,
    pub network_access: NetworkAccessRequirement,
    pub network_enforcement: NetworkEnforcementKind,
    pub executable: CanonicalExecutableRef,
    pub args: Vec<WindowsOsString>,
    pub working_directory: CanonicalPath,
    pub environment: Vec<SanitizedEnvironmentEntry>,
    pub filesystem_grants: Vec<SandboxFilesystemGrant>,
    pub network_grant_ids: Vec<NetworkGrantId>,
    pub timeout_ms: u64,
    pub output_limits: ProcessOutputLimits,
    pub cancellation_id: CancellationId,
    pub plan_hash: ContentHash,
}

/// Frozen launch contract for a trusted external process. It deliberately uses
/// `ExternalIsolationMode` instead of the native Tool sandbox enum: external
/// compatibility means managed standard-user execution, never Compatibility
/// AppContainer.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalProcessLaunchPlan {
    pub execution_id: SandboxExecutionId,
    pub identity: InvocationIdentity,
    pub permission_snapshot_id: PermissionSnapshotId,
    pub source: SecuritySource,
    pub source_fingerprint: ContentHash,
    pub isolation: ExternalIsolationMode,
    pub network_access: NetworkAccessRequirement,
    pub network_enforcement: NetworkEnforcementKind,
    pub executable: CanonicalExecutableRef,
    pub args: Vec<WindowsOsString>,
    pub working_directory: CanonicalPath,
    pub environment: Vec<SanitizedEnvironmentEntry>,
    pub timeout_ms: u64,
    pub output_limits: ProcessOutputLimits,
    pub cancellation_id: CancellationId,
    pub plan_hash: ContentHash,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxExecutionState {
    Planned,
    Starting,
    Running,
    Exited,
    Cancelled,
    TimedOut,
    PolicyViolation,
    CleanupFailed,
    StateUnknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxAclJournalState {
    Prepared,
    Applied,
    CleanupPending,
    Cleaned,
    CleanupFailed,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxAclJournalEntry {
    pub id: SandboxAclJournalId,
    pub execution_id: SandboxExecutionId,
    pub target_identity: Option<FileIdentity>,
    pub target_path: CanonicalPath,
    pub original_dacl_sddl: String,
    pub original_dacl_hash: ContentHash,
    pub appcontainer_profile_name: String,
    pub appcontainer_sid: String,
    pub access: SandboxFilesystemAccess,
    pub recursive: bool,
    pub state: SandboxAclJournalState,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
    pub error_code: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxExecutionRecord {
    pub plan: SandboxExecutionPlan,
    pub state: SandboxExecutionState,
    pub process_id: Option<u32>,
    pub exit_code: Option<u32>,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
    pub terminal_at: Option<UtcTimestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CancelSandboxExecutionInput {
    pub cancellation_id: CancellationId,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "event_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SandboxHostEvent {
    #[serde(rename = "sandbox.started")]
    Started { execution_id: SandboxExecutionId },
    #[serde(rename = "sandbox.process_started")]
    ProcessStarted { execution_id: SandboxExecutionId, process_id: u32 },
    #[serde(rename = "sandbox.stdout_chunk")]
    StdoutChunk { execution_id: SandboxExecutionId, sequence: u64, bytes: Vec<u8> },
    #[serde(rename = "sandbox.stderr_chunk")]
    StderrChunk { execution_id: SandboxExecutionId, sequence: u64, bytes: Vec<u8> },
    #[serde(rename = "sandbox.exit")]
    Exit { execution_id: SandboxExecutionId, exit_code: u32 },
    #[serde(rename = "sandbox.cancelled")]
    Cancelled { execution_id: SandboxExecutionId },
    #[serde(rename = "sandbox.timeout")]
    Timeout { execution_id: SandboxExecutionId },
    #[serde(rename = "sandbox.policy_violation")]
    PolicyViolation { execution_id: SandboxExecutionId, code: String },
    #[serde(rename = "sandbox.cleanup_completed")]
    CleanupCompleted { execution_id: SandboxExecutionId },
    #[serde(rename = "sandbox.cleanup_failed")]
    CleanupFailed { execution_id: SandboxExecutionId, code: String },
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeComponentState {
    #[default]
    NotStarted,
    Starting,
    Ready,
    Degraded,
    Unavailable,
    Crashed,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostPreparationSummary {
    pub appcontainer_available: bool,
    pub nested_jobs_available: bool,
    pub standard_user_token_available: bool,
    pub system_drive_compatibility_ready: bool,
    pub nul_compatibility_ready: bool,
    pub residual_acl_count: u32,
    pub residual_firewall_rule_count: u32,
    pub residual_appcontainer_profile_count: u32,
    pub restart_required: bool,
    pub last_error_code: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostPreparationAction {
    Prepare,
    Repair,
    Remove,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostPreparationOperationState {
    Prepared,
    AwaitingElevation,
    Applying,
    Completed,
    RollbackPending,
    RolledBack,
    Denied,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostPreparationOperationView {
    pub operation_id: HostPreparationOperationId,
    pub operation_revision: u64,
    pub action: HostPreparationAction,
    pub state: HostPreparationOperationState,
    pub host_preparation_revision: u64,
    pub runtime_instance_id: RuntimeInstanceId,
    pub release_descriptor: ReleaseDescriptor,
    pub plan_hash: ContentHash,
    pub expires_at: UtcTimestamp,
    pub requires_elevation: bool,
    pub add_all_application_packages_ace: bool,
    pub add_all_restricted_application_packages_ace: bool,
    pub update_null_device_security: bool,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
    pub error_code: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostPreparationStatus {
    pub summary: HostPreparationSummary,
    pub revision: u64,
    pub active_operation: Option<HostPreparationOperationView>,
}

/// Main-only result for a trusted Host Preparation request. Renderer receives
/// only `status`; Electron Main uses the read-only ApprovalView to render the
/// native confirmation dialog.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostPreparationBeginResult {
    pub status: HostPreparationStatus,
    pub approval: Option<ApprovalView>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostPreparationMutationInput {
    pub expected_revision: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolveHostPreparationInput {
    pub approval_id: ApprovalRequestId,
    pub decision: ApprovalDecisionKind,
}

/// Core-issued, short-lived authorization for one Elevated Helper launch.
///
/// The plaintext nonce is never exposed through the Renderer or Electron Main
/// API. Core moves the ticket directly into the authenticated Helper command.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ElevationLaunchTicket {
    pub ticket_id: ElevationLaunchTicketId,
    pub approval_id: ApprovalRequestId,
    pub operation_id: HostPreparationOperationId,
    pub plan_hash: ContentHash,
    pub runtime_instance_id: RuntimeInstanceId,
    pub release_descriptor: ReleaseDescriptor,
    pub issued_at: UtcTimestamp,
    pub expires_at: UtcTimestamp,
    pub ticket_nonce: SecretNonce,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostPreparationOwnership {
    pub system_drive_root: Option<CanonicalPath>,
    pub all_application_packages_ace_owned: bool,
    pub all_restricted_application_packages_ace_owned: bool,
    pub original_null_device_sddl: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostPreparationBrokerPlan {
    pub operation_id: HostPreparationOperationId,
    pub action: HostPreparationAction,
    pub host_preparation_revision: u64,
    pub runtime_instance_id: RuntimeInstanceId,
    pub release_descriptor: ReleaseDescriptor,
    pub plan_nonce_hash: ContentHash,
    pub expires_at: UtcTimestamp,
    pub system_drive_root: CanonicalPath,
    pub ownership_before: HostPreparationOwnership,
    pub add_all_application_packages_ace: bool,
    pub add_all_restricted_application_packages_ace: bool,
    pub update_null_device_security: bool,
    pub plan_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostPreparationBrokerResult {
    pub operation_id: HostPreparationOperationId,
    pub succeeded: bool,
    pub ownership_after: HostPreparationOwnership,
    pub error_code: Option<String>,
    /// The approved system-drive control-field difference observed during this
    /// successful write: `SE_DACL_AUTO_INHERITED` was lost. Revoke does not restore
    /// the marker. This does not describe NUL or reconstruct earlier operations.
    ///
    /// The helper reports it so the operation evidence can state the difference; it is
    /// never a licence to ignore any other descriptor change, and `false` means the
    /// verified system-drive writes left its control field as they found it (or a
    /// proven pre-write refusal performed no writes). A failed/unknown write is not
    /// represented by a default `false`. Core and Helper update together in one
    /// manifest/hash-bound release; older terminal payloads missing this field fail
    /// deserialization rather than inventing an observation.
    pub approved_auto_inherited_marker_lost: bool,
}

/// The error codes an Elevated Helper may report for a refusal that provably
/// precedes every resource write.
///
/// A closed set on purpose: Core keeps reporting `sandbox.elevated_state_unknown`
/// for every other helper failure, so a helper cannot turn a real uncertainty
/// into a definite "nothing was written" by inventing a code.
pub const HOST_PREPARATION_PRE_WRITE_REFUSAL_CODES: &[&str] =
    &["sandbox.host_preparation_plan_invalid", "sandbox.host_preparation_capability_conflict"];

/// Whether `code` is one of the pre-write refusals above.
///
/// Returns the matching known code as a `&'static str`, so a caller maps the wire
/// text into the closed set instead of echoing whatever a helper sent.
#[must_use]
pub fn host_preparation_pre_write_refusal_code(code: &str) -> Option<&'static str> {
    HOST_PREPARATION_PRE_WRITE_REFUSAL_CODES.iter().copied().find(|known| *known == code)
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ElevatedHelperHello {
    pub protocol_version: ContractVersion,
    pub parent_process_id: u32,
    pub helper_process_id: u32,
    pub parent_runtime_instance_id: RuntimeInstanceId,
    pub helper_runtime_instance_id: RuntimeInstanceId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ElevatedHelperChallenge {
    pub bootstrap_nonce: SecretNonce,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ElevatedHelperChallengeProof {
    pub bootstrap_nonce: SecretNonce,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ElevatedHelperCommand {
    pub protocol_version: ContractVersion,
    pub release_descriptor: ReleaseDescriptor,
    pub runtime_instance_id: RuntimeInstanceId,
    pub elevation_launch_ticket: ElevationLaunchTicket,
    pub plan_nonce: SecretNonce,
    pub plan: HostPreparationBrokerPlan,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ElevatedHelperTerminal {
    pub protocol_version: ContractVersion,
    pub plan_hash: ContentHash,
    pub result: HostPreparationBrokerResult,
}

#[cfg(test)]
mod broker_result_serde_tests {
    use super::*;

    fn terminal(marker_lost: bool) -> ElevatedHelperTerminal {
        ElevatedHelperTerminal {
            protocol_version: ContractVersion::CURRENT,
            plan_hash: ContentHash::blake3(b"broker result serde fixture"),
            result: HostPreparationBrokerResult {
                operation_id: HostPreparationOperationId::new(),
                succeeded: true,
                ownership_after: HostPreparationOwnership::default(),
                error_code: None,
                approved_auto_inherited_marker_lost: marker_lost,
            },
        }
    }

    #[test]
    fn helper_terminal_round_trip_preserves_true_and_false_observations() {
        for marker_lost in [false, true] {
            let expected = terminal(marker_lost);
            let bytes = serde_json::to_vec(&expected).expect("serialize terminal");
            let actual: ElevatedHelperTerminal =
                serde_json::from_slice(&bytes).expect("read terminal");
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn absent_or_invalid_marker_evidence_is_not_defaulted_to_false() {
        let original = serde_json::to_value(terminal(true)).expect("terminal value");
        let mut missing = original.clone();
        missing["result"]
            .as_object_mut()
            .expect("serialized result is an object")
            .remove("approved_auto_inherited_marker_lost");
        let error = serde_json::from_value::<ElevatedHelperTerminal>(missing)
            .expect_err("old payload lacks an observation");
        assert!(error.to_string().contains("approved_auto_inherited_marker_lost"));
        for invalid in [serde_json::Value::Null, serde_json::json!("false"), serde_json::json!(0)] {
            let mut value = original.clone();
            value["result"]["approved_auto_inherited_marker_lost"] = invalid;
            assert!(serde_json::from_value::<ElevatedHelperTerminal>(value).is_err());
        }
    }
}

#[cfg(test)]
mod attempt_local_environment_tests {
    use super::*;

    fn value_of(entry: &SanitizedEnvironmentEntry) -> String {
        String::from_utf16_lossy(&entry.value.utf16)
    }

    #[test]
    fn the_attempt_local_binding_is_exactly_temp_tmp_and_local_app_data() {
        let entries = attempt_local_environment(r"C:\Aura\attempt-7");
        let names = entries.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>();
        assert_eq!(names, ["TEMP", "TMP", "LOCALAPPDATA"]);
        for entry in &entries {
            assert_eq!(value_of(entry), r"C:\Aura\attempt-7");
        }
    }

    #[test]
    fn two_attempts_bind_their_own_directory() {
        let first = attempt_local_environment(r"C:\Aura\attempt-1");
        let second = attempt_local_environment(r"C:\Aura\attempt-2");
        assert_ne!(value_of(&first[2]), value_of(&second[2]));
        assert_eq!(value_of(&first[2]), r"C:\Aura\attempt-1");
        assert_eq!(value_of(&second[2]), r"C:\Aura\attempt-2");
    }

    #[test]
    fn attempt_local_names_are_recognised_regardless_of_case() {
        for name in ["TEMP", "temp", "Tmp", "LOCALAPPDATA", "localappdata", "LocalAppData"] {
            assert!(is_attempt_local_environment_name(name), "{name}");
        }
        for name in ["LOCALAPPDATAX", "APPDATA", "USERPROFILE", "PATH", "AURA_TOOL_CALL_ID"] {
            assert!(!is_attempt_local_environment_name(name), "{name}");
        }
    }
}
