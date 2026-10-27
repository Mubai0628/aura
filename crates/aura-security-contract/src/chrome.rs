//! The user's own Chrome. A call acts on one tab of one share and carries Chrome capabilities
//! only; it is not the built-in browser and it is not Aura's network.
//!
//! What the session mode covers on its own, once the tab's site is confirmed for the share:
//!
//! - reading the tab (snapshot, screenshot, console, waiting, handing it to the user) — every mode;
//! - acting on it (open, navigate, click, type, keys, select, scroll, close) — WorkspaceReadWrite
//!   and FullAccess;
//! - a sensitive action — never: it asks every time, and only once may be allowed.
//!
//! A site the user has not confirmed for the share asks in every mode; Core keeps that answer on
//! the share, not as a grant. In ReadOnly an answer may cover the share for the rest of the turn
//! ([`ChromeTurnConsentRef`]). No other approval covers Chrome, and a Chrome answer covers
//! nothing else.
use crate::{
    AuthorizationGrant, AuthorizationLifetime, InvocationActor, InvocationIdentity,
    OperationFingerprint, OperationRiskFlag, PermissionMode, ResolvedCapability as C,
    ResourceDescriptor, SecuritySourceRef,
};
use aura_domain_common::{ApprovalRequestId, PrincipalId, Revision, SessionId, TurnTaskId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A built-in `chrome.*` tool, whatever its shape.
#[must_use]
pub fn chrome_tool(source: &SecuritySourceRef, action: &str) -> bool {
    matches!(source, SecuritySourceRef::BuiltInTool { .. }) && action.starts_with("chrome.")
}

#[must_use]
pub fn chrome_capability(capability: C) -> bool {
    matches!(capability, C::ChromeRead | C::ChromeInteraction | C::ChromeSensitiveAction)
}

/// Whether this risk is a Chrome action no approval can allow.
#[must_use]
pub fn chrome_hard_risk(flag: OperationRiskFlag) -> bool {
    matches!(
        flag,
        OperationRiskFlag::ChromeCredentialField
            | OperationRiskFlag::ChromeCaptcha
            | OperationRiskFlag::ChromePasskey
            | OperationRiskFlag::ChromeFileChooser
    )
}

/// `chrome://`, extension and DevTools pages, and the Chrome Web Store. Ordinary sites are not.
#[must_use]
pub fn chrome_origin_blocked(origin: &str) -> bool {
    let lower = origin.trim().to_ascii_lowercase();
    if ["chrome:", "chrome-extension:", "chrome-untrusted:", "chrome-search:", "devtools:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
    {
        return true;
    }
    let Some(rest) = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://"))
    else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("").split(':').next().unwrap_or("");
    host == "chrome.google.com" || host == "chromewebstore.google.com"
}

/// The share a Chrome call acts on.
#[must_use]
pub fn chrome_share(resources: &[ResourceDescriptor]) -> Option<&str> {
    resources.iter().find_map(|resource| match resource {
        ResourceDescriptor::ChromeShare { share_id } => Some(share_id.as_str()),
        _ => None,
    })
}

/// Whether every site the call names is one the user confirmed for its share.
#[must_use]
pub fn chrome_origin_confirmed(resources: &[ResourceDescriptor]) -> bool {
    resources.iter().all(|resource| match resource {
        ResourceDescriptor::ChromeOrigin { confirmed, .. } => *confirmed,
        _ => true,
    })
}

/// A Chrome call in its exact shape.
///
/// It names one share, one tab and at most one site, and carries Chrome capabilities only;
/// opening and navigating name the site they reach. `chrome.tabs` is not an operation: it lists
/// the session's own shares from Core's record and reaches neither Chrome nor anything else.
#[must_use]
pub fn built_in_chrome(
    source: &SecuritySourceRef,
    action: &str,
    capabilities: &[C],
    resources: &[ResourceDescriptor],
) -> bool {
    if !chrome_tool(source, action)
        || action == "chrome.tabs"
        || capabilities.is_empty()
        || !capabilities.iter().copied().all(chrome_capability)
    {
        return false;
    }
    let (mut shares, mut tabs, mut origins) = (0_usize, 0_usize, 0_usize);
    for resource in resources {
        match resource {
            ResourceDescriptor::ChromeShare { share_id } if !share_id.is_empty() => shares += 1,
            ResourceDescriptor::ChromeTab { tab_ref } if !tab_ref.is_empty() => tabs += 1,
            ResourceDescriptor::ChromeOrigin { origin, .. } if !origin.is_empty() => origins += 1,
            _ => return false,
        }
    }
    shares == 1
        && tabs == 1
        && origins <= 1
        && (origins == 1 || !matches!(action, "chrome.open" | "chrome.navigate"))
}

/// Whether the session mode already allows this Chrome capability. A site the user has not
/// confirmed for the share is never a mode default, and neither is a sensitive action.
#[must_use]
pub fn chrome_mode_covers(
    mode: PermissionMode,
    capability: C,
    resources: &[ResourceDescriptor],
) -> bool {
    chrome_origin_confirmed(resources)
        && match capability {
            C::ChromeRead => true,
            C::ChromeInteraction => mode != PermissionMode::ReadOnly,
            _ => false,
        }
}

/// A ReadOnly answer for one shared tab: reading it and acting on it ask no more for the rest
/// of the turn. It never covers a sensitive action, a site the user has not confirmed, another
/// share, or anything that is not Chrome.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChromeTurnConsentRef {
    pub approval_id: ApprovalRequestId,
    pub session_id: SessionId,
    pub turn_task_id: TurnTaskId,
    pub principal_id: PrincipalId,
    pub share_id: String,
    pub approved_operation_fingerprint: OperationFingerprint,
    pub grant_revision: Revision,
}

#[must_use]
pub fn chrome_consent_covers(
    grant: &AuthorizationGrant,
    identity: &InvocationIdentity,
    source: &SecuritySourceRef,
    action: &str,
    capabilities: &[C],
    resources: &[ResourceDescriptor],
) -> bool {
    let Some(consent) = &grant.scope.chrome_consent else {
        return false;
    };
    let scope = &grant.scope;
    built_in_chrome(source, action, capabilities, resources)
        && chrome_share(resources) == Some(consent.share_id.as_str())
        && chrome_origin_confirmed(resources)
        && capabilities
            .iter()
            .all(|capability| matches!(capability, C::ChromeRead | C::ChromeInteraction))
        && scope.operation_fingerprint == consent.approved_operation_fingerprint
        && scope.consent.is_none()
        && scope.file_consent.is_none()
        && scope.network_consent.is_none()
        && scope.memory_consent.is_none()
        && scope.mcp_consent.is_none()
        && scope.capability_limit.as_ref().is_none_or(|limit| {
            limit
                .iter()
                .all(|capability| matches!(capability, C::ChromeRead | C::ChromeInteraction))
        })
        && identity.actor == InvocationActor::ParentAgent
        && identity.session_id == Some(consent.session_id)
        && identity.turn_task_id == Some(consent.turn_task_id)
        && identity.principal_id == consent.principal_id
        && grant.revision == consent.grant_revision
        && grant.revoked_at.is_none()
        && grant.consumed_at.is_none()
        && matches!(scope.lifetime, AuthorizationLifetime::TurnTask { session_id, turn_task_id }
            if session_id == consent.session_id && turn_task_id == consent.turn_task_id)
}
