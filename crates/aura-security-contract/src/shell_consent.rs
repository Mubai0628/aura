//! Distinguishes historical Native approvals from persistent session answers.
use crate::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellConsentLifetime {
    #[default]
    LegacyRuntime,
    Session,
}

#[must_use]
pub fn built_in_shell(source: &SecuritySourceRef, action: &str) -> bool {
    matches!(source, SecuritySourceRef::BuiltInTool { .. })
        && NativeShell::from_action_kind(action).is_some()
}

#[must_use]
pub fn shell_consent_covers_capability(
    grant: &AuthorizationGrant,
    identity: &InvocationIdentity,
    source: &SecuritySourceRef,
    action: &str,
    cap: ResolvedCapability,
) -> bool {
    let Some(consent) = &grant.scope.consent else {
        return false;
    };
    consent.lifetime == ShellConsentLifetime::Session
        && consent.grant_id == grant.grant_id
        && consent.grant_revision == grant.revision
        && consent.revoked_grant_revision.is_none()
        && identity.principal_id == consent.approved.principal_id
        && matches!(grant.scope.lifetime, AuthorizationLifetime::Session { session_id } if identity.session_id == Some(session_id))
        && built_in_shell(source, action)
        && NativeShell::from_action_kind(action).is_some_and(|shell| consent.approved.covers(shell))
        && cap == ResolvedCapability::ProcessExecute
        && grant.scope.capability_limit.as_ref().is_none_or(|caps| caps.contains(&cap))
}
