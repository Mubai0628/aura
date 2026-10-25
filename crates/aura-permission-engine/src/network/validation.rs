use crate::policy::{PolicyInput, lifetime_matches};
use aura_execution_contract::BackendDescriptor;
use aura_security_contract::*;
use std::collections::BTreeSet;

/// Revalidate the originally selected authorities. An unrelated live grant cannot replace a
/// revoked slice, even if a fresh policy decision would choose it for a new operation.
pub fn validate_composite(
    input: &PolicyInput<'_>,
    backend: &BackendDescriptor,
    decision: &AuthorizationDecision,
) -> bool {
    let AuthorizationBasis::Composite { slices } = &decision.basis else { return false };
    let op = input.operation;
    let c = &op.content;
    if !super::uses_capability_slices(input)
        || decision.scope.profile != input.profile
        || decision.scope.operation_fingerprint != op.fingerprint
        || !decision.scope.is_exact_operation()
        || !matches!(decision.scope.lifetime,AuthorizationLifetime::Once { operation_id } if operation_id == op.operation_id)
        || !backend.supports_operation(&decision.scope.backend_requirement, op)
        || slices.len() != c.capabilities.iter().collect::<BTreeSet<_>>().len()
    {
        return false;
    }
    let mut seen = BTreeSet::new();
    slices.iter().all(|slice| {
        slice.identity == c.identity
            && slice.resources == c.resources
            && slice.caller == input.network_caller
            && c.capabilities.contains(&slice.capability)
            && seen.insert(slice.capability)
            && match &slice.authority {
                CapabilityAuthority::PermissionMode {
                    snapshot_id,
                    mode,
                    session_permission_revision,
                } => {
                    *snapshot_id == input.snapshot.id
                        && *mode == input.snapshot.mode
                        && *session_permission_revision
                            == input.snapshot.session_permission_revision
                        && ordinary_mode_covers_capability(
                            *mode,
                            &c.source,
                            &c.action_kind,
                            slice.capability,
                            &c.resources,
                        )
                }
                CapabilityAuthority::Grant { grant_id, revision, scope } => {
                    input.grants.iter().any(|g| {
                        g.grant_id == *grant_id
                            && g.revision == *revision
                            && g.is_live_at(input.now)
                            && lifetime_matches(&g.scope.lifetime, op)
                            && g.expires_at.is_none_or(|expiry| decision.expires_at <= expiry)
                            && AuthorizationScope::derive_for(&g.scope, &op.fingerprint).as_ref()
                                == Some(scope.as_ref())
                            && grant_covers_capability(
                                g,
                                &op.fingerprint,
                                &c.identity,
                                &c.source,
                                &c.action_kind,
                                slice.capability,
                                &c.resources,
                            )
                            && backend.supports_operation(&g.scope.backend_requirement, op)
                    })
                }
                CapabilityAuthority::ExternalGrant { grant_id, revalidation_revision } => {
                    input.verified_external_grants.iter().any(|g| {
                        g.capability == slice.capability
                            && g.grant_id == *grant_id
                            && g.revalidation_revision == *revalidation_revision
                            && *revalidation_revision > 0
                    })
                }
            }
    })
}
