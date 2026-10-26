//! Structural eligibility only. Core policy and Store validate each live slice.
use crate::SecuredOperation;
use aura_security_contract::*;
use std::collections::BTreeSet;

pub(super) fn valid(authority: &AuthorizationDecision, op: &SecuredOperation) -> bool {
    let c = &op.content;
    let git = built_in_native_git(&c.source, &c.action_kind, &c.capabilities);
    match &authority.basis {
        AuthorizationBasis::Grant { .. } => {
            git || matches!(
                authority.scope.lifetime,
                AuthorizationLifetime::Once { .. } | AuthorizationLifetime::Session { .. }
            )
        }
        AuthorizationBasis::PermissionMode { mode, .. } => {
            git && authority.scope.is_exact_operation()
                && authority.scope.capability_limit.is_none()
                && matches!(authority.scope.lifetime, AuthorizationLifetime::Once { operation_id } if operation_id == op.operation_id)
                && c.capabilities.iter().all(|cap| native_git_mode_covers(*mode, *cap))
        }
        AuthorizationBasis::Composite { slices } => {
            let shell = built_in_shell(&c.source, &c.action_kind)
                && c.capabilities.contains(&ResolvedCapability::ProcessExecute)
                && c.capabilities.iter().all(|cap| {
                    matches!(
                        cap,
                        ResolvedCapability::ProcessExecute
                            | ResolvedCapability::OrdinaryNetwork
                            | ResolvedCapability::DownloadArtifact
                    )
                });
            // The trusted package program exception, in its exact shape only.
            let package = package_program(&c.source, &c.action_kind, &c.capabilities, &c.resources);
            if !(shell || git || package)
                || !authority.scope.is_exact_operation()
                || authority.scope.capability_limit.is_some()
                || !matches!(authority.scope.lifetime,AuthorizationLifetime::Once { operation_id } if operation_id==op.operation_id)
                || slices.len() != c.capabilities.iter().collect::<BTreeSet<_>>().len()
            {
                return false;
            }
            let mut seen = BTreeSet::new();
            slices.iter().all(|slice| {
                slice.identity == c.identity
                    && slice.resources == c.resources
                    && slice.caller == NetworkCallerKind::AuraTool
                    && c.capabilities.contains(&slice.capability)
                    && seen.insert(slice.capability)
                    && match &slice.authority {
                        CapabilityAuthority::PermissionMode { mode, .. } => {
                            if git {
                                native_git_mode_covers(*mode, slice.capability)
                            } else {
                                ordinary_mode_covers_capability(
                                    *mode,
                                    &c.source,
                                    &c.action_kind,
                                    slice.capability,
                                    &c.resources,
                                )
                            }
                        }
                        CapabilityAuthority::Grant { revision, scope, .. } => {
                            revision.value() > 0 && scope.operation_fingerprint == op.fingerprint
                        }
                        CapabilityAuthority::ExternalGrant { .. } => false,
                    }
            })
        }
        _ => false,
    }
}
