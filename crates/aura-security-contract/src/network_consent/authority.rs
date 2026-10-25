use crate::{
    AuthorizationScope, InvocationIdentity, NetworkCallerKind, PermissionMode, ResolvedCapability,
    ResourceDescriptor,
};
use aura_domain_common::{ExternalGrantId, PermissionGrantId, PermissionSnapshotId, Revision};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Each slice binds exactly one capability and the immutable operation's caller and targets.
/// It is dispatch evidence for this operation, never a new reusable grant.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityAuthorization {
    pub capability: ResolvedCapability,
    pub identity: InvocationIdentity,
    pub caller: NetworkCallerKind,
    pub resources: Vec<ResourceDescriptor>,
    pub authority: CapabilityAuthority,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "basis", rename_all = "snake_case", deny_unknown_fields)]
pub enum CapabilityAuthority {
    PermissionMode {
        snapshot_id: PermissionSnapshotId,
        mode: PermissionMode,
        session_permission_revision: u64,
    },
    Grant {
        grant_id: PermissionGrantId,
        revision: Revision,
        scope: Box<AuthorizationScope>,
    },
    /// Existing external-object authority is rechecked by its owning Store and resolver
    /// under Core's permission barrier, including immediately before file publication.
    ExternalGrant {
        grant_id: ExternalGrantId,
        revalidation_revision: u64,
    },
}

/// Trusted facts supplied by the external-object resolver, independently of the model payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedExternalCapability {
    pub capability: ResolvedCapability,
    pub grant_id: ExternalGrantId,
    pub revalidation_revision: u64,
}
