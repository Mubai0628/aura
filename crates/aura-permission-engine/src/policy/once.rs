//! The operations answered by one approval that is never reused.
use aura_execution_contract::SecuredOperation;
use aura_security_contract::{
    OperationRiskFlag, PermissionMode, ResolvedCapability, SecuritySourceRef,
};

use super::{deletion::deletes_for_good, outside_writes::writes_outside};

/// Whether `op` is answered by one approval that is never reused: one of the risky operations
/// below, a deletion that skips the Recycle Bin, or a command whose text writes outside the
/// workspace.
pub(super) fn forced_once(op: &SecuredOperation, mode: PermissionMode) -> bool {
    risky_once(op) || deletes_for_good(op, mode) || writes_outside(op, mode)
}

/// The risky operations that are answered by one approval and never reused.
///
/// # Why an ordinary external delete is deliberately absent
///
/// Deleting, overwriting or renaming inside a write scope the user already granted is an
/// ordinary write operation, not a second decision: the scope's approval is what covers it,
/// and it may be a session or workspace term like any other write in that scope. Forcing
/// these to a single use made a granted directory unusable for ordinary work and asked a
/// second time for an object the user had already named.
///
/// What still forces one use is the set below: operations that change the machine, install
/// or elevate software, destroy a remote, or act on the user's browser session — things no
/// directory grant can express, and where reuse is not something the user can have meant.
/// A protected location is not in this list either: it is decided by its own capability
/// (which asks in every mode) and may be approved once **or** for the session, exactly as
/// the confirmed decision requires. The Aura control data and credential boundaries are not
/// reachable through an ordinary file approval at all — they are governed by the trusted
/// settings and maintenance entries — so no rule here has to re-express that.
fn risky_once(op: &SecuredOperation) -> bool {
    // A workspace command is text offered to the user later; each save or removal is the
    // user's answer, in every mode, and never becomes reusable authority.
    (matches!(op.content.source, SecuritySourceRef::BuiltInTool { .. })
        && matches!(op.content.action_kind.as_str(), "commands.save" | "commands.delete"))
        || op.content.risks.iter().any(|r| {
            matches!(
                r,
                OperationRiskFlag::ExecutableArtifact
                    | OperationRiskFlag::SoftwareInstallation
                    | OperationRiskFlag::SystemConfigurationMutation
                    | OperationRiskFlag::Elevation
                    | OperationRiskFlag::DriverOrServiceMutation
                    | OperationRiskFlag::DestructiveRemoteMutation
                    | OperationRiskFlag::SensitiveBrowserAction
                    | OperationRiskFlag::GitBareForcePush
                    | OperationRiskFlag::GitSequencerSkipOrQuit
            )
        })
        || op.content.capabilities.iter().any(|c| {
            matches!(
                c,
                ResolvedCapability::DownloadWindowsExecutable
                    | ResolvedCapability::MemoryDelete
                    | ResolvedCapability::ExternalServiceDestructiveMutation
                    | ResolvedCapability::GitDestructive
                    | ResolvedCapability::RegistryWrite
                    | ResolvedCapability::SystemEnvironmentWrite
                    | ResolvedCapability::SystemFileWrite
                    | ResolvedCapability::ServiceManagement
                    | ResolvedCapability::ScheduledTaskManagement
                    | ResolvedCapability::DriverManagement
                    | ResolvedCapability::WindowsSoftwareInstall
                    | ResolvedCapability::ChromeSensitiveAction
                    | ResolvedCapability::ElevatedOperation
            )
        })
}
