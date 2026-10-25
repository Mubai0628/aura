//! Dedicated managed-Vault authority. It is never an ordinary filesystem grant.
use crate::{
    AuthorizationGrant, AuthorizationLifetime, InvocationActor, InvocationIdentity,
    OperationFingerprint, ResolvedCapability as C, ResourceDescriptor, SecuritySourceRef,
};
use aura_domain_common::{ApprovalRequestId, ContentHash, PrincipalId, Revision, SessionId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemoryVaultScope {
    Global,
    Project { workspace_id: aura_domain_common::WorkspaceId },
}

#[must_use]
pub fn built_in_memory(
    source: &SecuritySourceRef,
    action: &str,
    caps: &[C],
    resources: &[ResourceDescriptor],
) -> bool {
    if !matches!(source, SecuritySourceRef::BuiltInTool { .. })
        || !matches!(resources, [ResourceDescriptor::MemoryVault { .. }])
    {
        return false;
    }
    match action {
        "memory.search" | "memory.read" | "memory.list_recent" | "memory.validate" => {
            caps == [C::MemoryRead]
        }
        "memory.create" | "memory.update" | "memory.link" | "memory.forget" | "memory.archive" => {
            caps == [C::MemoryWrite]
        }
        "memory.delete" => caps == [C::MemoryDelete],
        "memory.repair" => caps == [C::MemoryRead] || caps == [C::MemoryWrite],
        _ => false,
    }
}
#[must_use]
pub fn has_memory_authority(action: &str, caps: &[C], resources: &[ResourceDescriptor]) -> bool {
    action.starts_with("memory.")
        || caps.iter().any(|c| matches!(c, C::MemoryRead | C::MemoryWrite | C::MemoryDelete))
        || resources.iter().any(|r| matches!(r, ResourceDescriptor::MemoryVault { .. }))
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemorySessionConsentRef {
    pub approval_id: ApprovalRequestId,
    pub session_id: SessionId,
    pub principal_id: PrincipalId,
    pub scope: MemoryVaultScope,
    pub vault_identity: ContentHash,
    pub approved_operation_fingerprint: OperationFingerprint,
    pub grant_revision: Revision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_grant_revision: Option<Revision>,
}
impl MemorySessionConsentRef {
    pub fn accepts_grant_revision(&self, revision: Revision) -> bool {
        revision == self.grant_revision || Some(revision) == self.revoked_grant_revision
    }
}

#[must_use]
pub fn memory_consent_covers(
    grant: &AuthorizationGrant,
    identity: &InvocationIdentity,
    source: &SecuritySourceRef,
    action: &str,
    caps: &[C],
    resources: &[ResourceDescriptor],
) -> bool {
    let Some(consent) = &grant.scope.memory_consent else {
        return false;
    };
    if caps != [C::MemoryWrite]
        || grant.scope.operation_fingerprint != consent.approved_operation_fingerprint
        || grant.scope.capability_limit.as_ref().is_some_and(|limit| limit != &[C::MemoryWrite])
        || grant.scope.consent.is_some()
        || grant.scope.file_consent.is_some()
        || grant.scope.network_consent.is_some()
        || !built_in_memory(source, action, caps, resources)
        || identity.actor != InvocationActor::ParentAgent
        || identity.session_id != Some(consent.session_id)
        || identity.principal_id != consent.principal_id
        || grant.revision != consent.grant_revision
        || !consent.accepts_grant_revision(grant.revision)
        || consent.revoked_grant_revision.is_some()
        || grant.revoked_at.is_some()
        || grant.consumed_at.is_some()
        || !matches!(grant.scope.lifetime,AuthorizationLifetime::Session {session_id} if session_id==consent.session_id)
    {
        return false;
    }
    matches!(resources,[ResourceDescriptor::MemoryVault {scope,vault_identity}] if *scope==consent.scope && *vault_identity==consent.vault_identity)
}
