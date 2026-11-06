//! Programs on the user's own desktop. A call reads or acts on one program's windows and carries
//! computer-control capabilities only; it is neither a file, a process Aura runs, nor the network.
//!
//! The permission is the program's, whatever the session mode: a program the session has
//! authorized is read and acted on without asking, in every mode; any other program asks in
//! every mode. The session authorizes a program when the user answers a question for the session
//! (the answer is kept in the approval record, so it outlasts a restart:
//! [`DesktopAppConsentRef`]) or when the program is on the always-allowed list of the computer
//! settings and the session has not stopped it since. No other approval covers computer control,
//! and a computer-control answer covers nothing else.
use crate::{
    AuthorizationGrant, InvocationActor, InvocationIdentity, OperationFingerprint,
    ResolvedCapability as C, ResourceDescriptor, SecuritySourceRef,
};
use aura_domain_common::{ApprovalRequestId, PrincipalId, Revision, SessionId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A built-in `computer.*` tool, whatever its shape.
#[must_use]
pub fn computer_tool(source: &SecuritySourceRef, action: &str) -> bool {
    matches!(source, SecuritySourceRef::BuiltInTool { .. }) && action.starts_with("computer.")
}

#[must_use]
pub fn desktop_capability(capability: C) -> bool {
    matches!(capability, C::DesktopRead | C::DesktopInput)
}

/// The program a computer-control call is for.
#[must_use]
pub fn desktop_app(resources: &[ResourceDescriptor]) -> Option<&str> {
    resources.iter().find_map(|resource| match resource {
        ResourceDescriptor::DesktopApp { app_id, .. } => Some(app_id.as_str()),
        _ => None,
    })
}

/// Whether every program the call names is one the session has authorized.
#[must_use]
pub fn desktop_app_authorized(resources: &[ResourceDescriptor]) -> bool {
    resources.iter().all(|resource| match resource {
        ResourceDescriptor::DesktopApp { authorized, .. } => *authorized,
        _ => true,
    })
}

/// A computer-control call in its exact shape: one program and computer-control capabilities
/// only. `computer.apps` is not an operation: it lists Aura's own catalog of programs and reaches
/// no program.
#[must_use]
pub fn built_in_computer(
    source: &SecuritySourceRef,
    action: &str,
    capabilities: &[C],
    resources: &[ResourceDescriptor],
) -> bool {
    computer_tool(source, action)
        && action != "computer.apps"
        && !capabilities.is_empty()
        && capabilities.iter().copied().all(desktop_capability)
        && matches!(resources, [ResourceDescriptor::DesktopApp { app_id, .. }] if !app_id.is_empty())
}

/// Whether the session already allows this capability on the call's program: only on a program
/// the session has authorized, and then in every mode.
#[must_use]
pub fn desktop_mode_covers(capability: C, resources: &[ResourceDescriptor]) -> bool {
    desktop_capability(capability) && desktop_app_authorized(resources)
}

/// A program the user authorized for one session: the session reads and acts on it without
/// asking again, also after Aura restarts, until the user stops it. The Grant itself only stands
/// for the call that was answered; later calls count the program as authorized when Core
/// resolves them.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DesktopAppConsentRef {
    pub approval_id: ApprovalRequestId,
    pub session_id: SessionId,
    pub principal_id: PrincipalId,
    pub app_id: String,
    pub approved_operation_fingerprint: OperationFingerprint,
    pub grant_revision: Revision,
}

/// Whether a session answer for a program covers this call: only the call that was answered.
#[must_use]
pub fn desktop_app_consent_covers(
    grant: &AuthorizationGrant,
    identity: &InvocationIdentity,
    source: &SecuritySourceRef,
    action: &str,
    capabilities: &[C],
    resources: &[ResourceDescriptor],
) -> bool {
    let Some(consent) = &grant.scope.desktop_app_consent else {
        return false;
    };
    let scope = &grant.scope;
    built_in_computer(source, action, capabilities, resources)
        && desktop_app(resources) == Some(consent.app_id.as_str())
        && scope.operation_fingerprint == consent.approved_operation_fingerprint
        && scope.consent.is_none()
        && scope.file_consent.is_none()
        && scope.network_consent.is_none()
        && scope.memory_consent.is_none()
        && scope.mcp_consent.is_none()
        && scope.chrome_consent.is_none()
        && scope.chrome_site_consent.is_none()
        && scope
            .capability_limit
            .as_ref()
            .is_none_or(|limit| limit.iter().copied().all(desktop_capability))
        && identity.actor == InvocationActor::ParentAgent
        && identity.session_id == Some(consent.session_id)
        && identity.principal_id == consent.principal_id
        && grant.revision == consent.grant_revision
        && grant.revoked_at.is_none()
}
