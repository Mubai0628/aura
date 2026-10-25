//! Calling a trusted MCP server's tool. Aura does not infer what the tool does
//!: the call has one coarse capability, names the server and the configuration the
//! user trusted, and in ReadOnly an answer covers that server for the rest of the turn only.
use crate::{
    AuthorizationGrant, AuthorizationLifetime, InvocationActor, InvocationIdentity,
    OperationFingerprint, ResolvedCapability as C, ResourceDescriptor, SecuritySourceRef,
};
use aura_domain_common::{
    ApprovalRequestId, ContentHash, PrincipalId, Revision, SessionId, TurnTaskId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The server a call names when it is an MCP call in its exact shape: an MCP server source,
/// an `mcp.<server>.<tool>` action for that server, exactly one matching `McpServer` resource
/// and the one coarse capability. Anything else borrows nothing from the MCP rules.
#[must_use]
pub fn mcp_server_call<'a>(
    source: &SecuritySourceRef,
    action: &str,
    caps: &[C],
    resources: &'a [ResourceDescriptor],
) -> Option<(&'a str, &'a ContentHash)> {
    let SecuritySourceRef::McpServer { server_id } = source else {
        return None;
    };
    let [ResourceDescriptor::McpServer { server_id: named, config_fingerprint }] = resources else {
        return None;
    };
    let tool = action.strip_prefix("mcp.")?.strip_prefix(server_id.as_str())?.strip_prefix('.')?;
    (named == server_id && !tool.is_empty() && caps == [C::ExternalExtensionCall])
        .then_some((named.as_str(), config_fingerprint))
}

/// A ReadOnly answer for one server: the server's later calls in the same turn, made with the
/// configuration the user trusted, ask no more.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpTurnConsentRef {
    pub approval_id: ApprovalRequestId,
    pub session_id: SessionId,
    pub turn_task_id: TurnTaskId,
    pub principal_id: PrincipalId,
    pub server_id: String,
    pub config_fingerprint: ContentHash,
    pub approved_operation_fingerprint: OperationFingerprint,
    pub grant_revision: Revision,
}

#[must_use]
pub fn mcp_consent_covers(
    grant: &AuthorizationGrant,
    identity: &InvocationIdentity,
    source: &SecuritySourceRef,
    action: &str,
    caps: &[C],
    resources: &[ResourceDescriptor],
) -> bool {
    let Some(consent) = &grant.scope.mcp_consent else {
        return false;
    };
    let Some((server_id, config_fingerprint)) = mcp_server_call(source, action, caps, resources)
    else {
        return false;
    };
    grant.scope.operation_fingerprint == consent.approved_operation_fingerprint
        && grant.scope.consent.is_none()
        && grant.scope.file_consent.is_none()
        && grant.scope.network_consent.is_none()
        && grant.scope.memory_consent.is_none()
        && grant
            .scope
            .capability_limit
            .as_ref()
            .is_none_or(|limit| limit == &[C::ExternalExtensionCall])
        && identity.actor == InvocationActor::ParentAgent
        && identity.session_id == Some(consent.session_id)
        && identity.turn_task_id == Some(consent.turn_task_id)
        && identity.principal_id == consent.principal_id
        && grant.revision == consent.grant_revision
        && grant.revoked_at.is_none()
        && grant.consumed_at.is_none()
        && matches!(grant.scope.lifetime, AuthorizationLifetime::TurnTask { session_id, turn_task_id }
            if session_id == consent.session_id && turn_task_id == consent.turn_task_id)
        && server_id == consent.server_id
        && *config_fingerprint == consent.config_fingerprint
}
