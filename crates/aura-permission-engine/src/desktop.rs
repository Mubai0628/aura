//! Computer-control calls as the policy reads them. The security contract judges the shape and
//! whether the program is authorized; this module adds what the policy alone decides: the
//! refusals no approval can lift and which answers a question may offer.
use aura_execution_contract::OperationNetwork;
use aura_security_contract::{
    ApprovalDecisionKind, AuthorizationGrant, InvocationActor, PolicyDenyReason,
    ResourceDescriptor, built_in_computer, computer_tool, desktop_app_consent_covers,
    desktop_capability,
};

use crate::policy::PolicyInput;

/// A computer-control call in its exact shape. It declares no network: what the program does is
/// the program's.
pub(crate) fn shape(input: &PolicyInput<'_>) -> bool {
    let content = &input.operation.content;
    content.network == OperationNetwork::None
        && built_in_computer(
            &content.source,
            &content.action_kind,
            &content.capabilities,
            &content.resources,
        )
}

/// Why a computer-control call is refused before anything may ask: a wrong shape is not an
/// operation, and no other tool may borrow a computer-control capability or name a program; a
/// child agent never acts on the user's desktop. Listing the programs reads Aura's own catalog
/// as task state; only a computer-control capability or resource on it makes it a
/// computer-control call, and a wrong one.
pub(crate) fn refusal(input: &PolicyInput<'_>) -> Option<PolicyDenyReason> {
    let content = &input.operation.content;
    let names_desktop = (computer_tool(&content.source, &content.action_kind)
        && content.action_kind != "computer.apps")
        || content.capabilities.iter().copied().any(desktop_capability)
        || content
            .resources
            .iter()
            .any(|resource| matches!(resource, ResourceDescriptor::DesktopApp { .. }));
    if !names_desktop {
        return None;
    }
    if !shape(input) {
        return Some(PolicyDenyReason::InvalidOperation);
    }
    if matches!(content.action_kind.as_str(), "computer.act" | "computer.input")
        && content.action.get("safety").and_then(|value| value.as_str()) != Some("clear")
    {
        return Some(PolicyDenyReason::ModeDenied);
    }
    (content.identity.actor == InvocationActor::ChildAgent).then_some(PolicyDenyReason::ModeDenied)
}

/// The answers a question about a program offers: for the session, or no. Keeping the program
/// for every later session is the settings page's always-allowed list, which the question's
/// card writes before it answers for the session.
pub(crate) fn allowed_decisions(
    input: &PolicyInput<'_>,
    forced_once: bool,
) -> Option<Vec<ApprovalDecisionKind>> {
    if !shape(input) {
        return None;
    }
    Some(if forced_once {
        vec![ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny]
    } else {
        vec![ApprovalDecisionKind::AllowSession, ApprovalDecisionKind::Deny]
    })
}

/// Whether a grant that is no program's session answer covers this computer-control call: only
/// an answer for exactly this call, carrying no consent of any kind.
pub(crate) fn exact_answer_covers(grant: &AuthorizationGrant, input: &PolicyInput<'_>) -> bool {
    let scope = &grant.scope;
    scope.is_exact_operation()
        && scope.chrome_site_consent.is_none()
        && scope.desktop_app_consent.is_none()
        && scope.capability_limit.is_none()
        && scope.operation_fingerprint == input.operation.fingerprint
}

/// Whether a session answer for a program covers this call (only the call that was answered).
pub(crate) fn app_consent_covers(grant: &AuthorizationGrant, input: &PolicyInput<'_>) -> bool {
    let content = &input.operation.content;
    desktop_app_consent_covers(
        grant,
        &content.identity,
        &content.source,
        &content.action_kind,
        &content.capabilities,
        &content.resources,
    )
}
