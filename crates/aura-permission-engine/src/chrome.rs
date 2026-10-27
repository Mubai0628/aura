//! Chrome calls as the policy reads them. The security contract judges the shape and what the
//! mode covers; this module adds what the policy alone decides: the refusals no approval can
//! lift, which answers a question may offer, and the turn answer's reach.
use aura_execution_contract::OperationNetwork;
use aura_security_contract::{
    ApprovalDecisionKind, AuthorizationGrant, InvocationActor, PolicyDenyReason,
    ResourceDescriptor, built_in_chrome, chrome_capability, chrome_consent_covers,
    chrome_hard_risk, chrome_origin_blocked, chrome_origin_confirmed, chrome_tool,
};

use crate::policy::PolicyInput;

/// A Chrome call in its exact shape. It declares no network: the page's traffic is Chrome's.
pub(crate) fn shape(input: &PolicyInput<'_>) -> bool {
    let content = &input.operation.content;
    content.network == OperationNetwork::None
        && built_in_chrome(
            &content.source,
            &content.action_kind,
            &content.capabilities,
            &content.resources,
        )
}

/// Why a Chrome call is refused before anything may ask: a wrong shape is not an operation, and
/// no other tool may borrow a Chrome capability or name a share; credential, code, CAPTCHA,
/// passkey and file-chooser actions and Chrome's own pages are beyond any approval; and a child
/// agent never acts in the user's browser. Listing the shares reads Core's own record as task
/// state; only a Chrome capability or resource on it makes it a Chrome call, and a wrong one.
pub(crate) fn refusal(input: &PolicyInput<'_>) -> Option<PolicyDenyReason> {
    let content = &input.operation.content;
    let names_chrome = (chrome_tool(&content.source, &content.action_kind)
        && content.action_kind != "chrome.tabs")
        || content.capabilities.iter().copied().any(chrome_capability)
        || content.resources.iter().any(|resource| {
            matches!(
                resource,
                ResourceDescriptor::ChromeShare { .. }
                    | ResourceDescriptor::ChromeTab { .. }
                    | ResourceDescriptor::ChromeOrigin { .. }
            )
        });
    if !names_chrome {
        return None;
    }
    if !shape(input) {
        return Some(PolicyDenyReason::InvalidOperation);
    }
    let blocked_page = content.resources.iter().any(|resource| {
        matches!(resource, ResourceDescriptor::ChromeOrigin { origin, .. } if chrome_origin_blocked(origin))
    });
    if blocked_page || content.risks.iter().copied().any(chrome_hard_risk) {
        return Some(PolicyDenyReason::HardBoundary);
    }
    (content.identity.actor == InvocationActor::ChildAgent).then_some(PolicyDenyReason::ModeDenied)
}

/// The answers a Chrome question offers. A sensitive action and a site the user has not
/// confirmed are answered once (Core keeps a confirmed site on the share); acting in ReadOnly
/// may also be allowed for the rest of the turn. Never a session or workspace answer.
pub(crate) fn allowed_decisions(
    input: &PolicyInput<'_>,
    forced_once: bool,
) -> Option<Vec<ApprovalDecisionKind>> {
    if !shape(input) {
        return None;
    }
    Some(if forced_once || !chrome_origin_confirmed(&input.operation.content.resources) {
        vec![ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny]
    } else {
        vec![
            ApprovalDecisionKind::AllowOnce,
            ApprovalDecisionKind::AllowForCurrentTurn,
            ApprovalDecisionKind::Deny,
        ]
    })
}

/// Whether a turn answer for one shared tab covers this call.
pub(crate) fn consent_covers(grant: &AuthorizationGrant, input: &PolicyInput<'_>) -> bool {
    let content = &input.operation.content;
    chrome_consent_covers(
        grant,
        &content.identity,
        &content.source,
        &content.action_kind,
        &content.capabilities,
        &content.resources,
    )
}
