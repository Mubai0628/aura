use crate::policy::{PolicyAssessment, PolicyInput, lifetime_matches, required_for_backend};
use aura_execution_contract::BackendDescriptor;
use aura_security_contract::*;

pub fn is_ordinary(input: &PolicyInput<'_>) -> bool {
    let c = &input.operation.content;
    let package = ordinary_tool_network_caller(&c.source, &c.action_kind)
        == Some(NetworkConsentCaller::PackageTools);
    // A package tool is ordinary only in its exact shape; anything else borrows
    // neither the package answer nor the generic ordinary-network path.
    if package {
        return input.network_caller == NetworkCallerKind::AuraTool
            && (package_program(&c.source, &c.action_kind, &c.capabilities, &c.resources)
                || package_http(&c.source, &c.action_kind, &c.capabilities, &c.resources));
    }
    (c.capabilities.contains(&ResolvedCapability::OrdinaryNetwork)
        || built_in_shell(&c.source, &c.action_kind))
        && ordinary_tool_network_caller(&c.source, &c.action_kind).is_some()
        && input.network_caller
            == if c.action_kind.starts_with("web.") {
                NetworkCallerKind::WebRuntime
            } else {
                NetworkCallerKind::AuraTool
            }
        && c.capabilities.iter().all(|cap| {
            *cap == ResolvedCapability::OrdinaryNetwork
                || built_in_shell(&c.source, &c.action_kind)
                    && *cap == ResolvedCapability::ProcessExecute
                || !built_in_shell(&c.source, &c.action_kind)
                    && file_capabilities_are_consentable(&[*cap])
        })
}

/// ReadOnly asks a package program every time: no
/// session answer applies there and none is offered.
fn package_program_read_only(input: &PolicyInput<'_>) -> bool {
    let c = &input.operation.content;
    input.snapshot.mode == PermissionMode::ReadOnly
        && package_program(&c.source, &c.action_kind, &c.capabilities, &c.resources)
}

/// Git shares capability slices and the existing consent store, not the HTTP gateway.
pub fn uses_capability_slices(input: &PolicyInput<'_>) -> bool {
    let c = &input.operation.content;
    is_ordinary(input)
        // Each browser capability is its own slice: a site answer covers opening or acting.
        || crate::browser::shape(input)
        || native_git_remote(&c.source, &c.action_kind, &c.capabilities)
            && input.network_caller == NetworkCallerKind::AuraTool
            && git_remote_facts(&c.resources).is_some()
}

pub fn capability_authority(
    input: &PolicyInput<'_>,
    cap: ResolvedCapability,
    allow_consumed: bool,
) -> Option<CapabilityAuthority> {
    capability_authority_inner(input, cap, allow_consumed, false)
}
fn capability_authority_inner(
    input: &PolicyInput<'_>,
    cap: ResolvedCapability,
    allow_consumed: bool,
    exact_once: bool,
) -> Option<CapabilityAuthority> {
    let op = input.operation;
    let c = &op.content;
    if !exact_once
        && ordinary_mode_covers_capability(
            input.snapshot.mode,
            &c.source,
            &c.action_kind,
            cap,
            &c.resources,
        )
    {
        return Some(CapabilityAuthority::PermissionMode {
            snapshot_id: input.snapshot.id,
            mode: input.snapshot.mode,
            session_permission_revision: input.snapshot.session_permission_revision,
        });
    }
    if !exact_once
        && let Some(external) = input
            .verified_external_grants
            .iter()
            .find(|v| v.capability == cap && v.revalidation_revision > 0)
    {
        return Some(CapabilityAuthority::ExternalGrant {
            grant_id: external.grant_id,
            revalidation_revision: external.revalidation_revision,
        });
    }
    let exact_once = exact_once || package_program_read_only(input);
    input.grants.iter().find_map(|g| {
        let live = g.is_live_at(input.now)
            || allow_consumed
                && g.scope.is_exact_operation()
                && g.issued_at <= input.now
                && g.revoked_at.is_none()
                && g.expires_at.is_none_or(|expiry| input.now < expiry);
        (live
            && (!exact_once || g.scope.is_exact_operation() && matches!(g.scope.lifetime, AuthorizationLifetime::Once { operation_id } if operation_id == op.operation_id))
            && (!c.capabilities.contains(&ResolvedCapability::GitDestructive) || g.scope.network_consent.is_none())
            && lifetime_matches(&g.scope.lifetime, op)
            && grant_covers_capability(
                g,
                &op.fingerprint,
                &c.identity,
                &c.source,
                &c.action_kind,
                cap,
                &c.resources,
            )
            && g.expires_at.is_none_or(|expiry| allow_consumed || input.expires_at <= expiry))
        .then(|| CapabilityAuthority::Grant {
            grant_id: g.grant_id,
            revision: g.revision,
            scope: Box::new(
                AuthorizationScope::derive_for(&g.scope, &op.fingerprint)
                    .expect("coverage requires exact identity or an explicit consent"),
            ),
        })
    })
}

pub fn permitted_now(input: &PolicyInput<'_>) -> bool {
    uses_capability_slices(input)
        && input.operation.content.capabilities.iter().all(|cap| {
            capability_authority_inner(
                input,
                *cap,
                true,
                input.operation.content.capabilities.contains(&ResolvedCapability::GitDestructive),
            )
            .is_some()
        })
}

pub fn authorize(
    input: &PolicyInput<'_>,
    backend: &BackendDescriptor,
    assessment: &PolicyAssessment,
) -> PolicyAuthorization {
    let op = input.operation;
    let mut required = required_for_backend(assessment, op, backend.class);
    required.guarantees.extend(backend.guarantees.iter().copied());
    required.allowed_classes = [backend.class].into();
    let mut scope = AuthorizationScope {
        operation_fingerprint: op.fingerprint.clone(),
        profile: input.profile,
        backend_requirement: required,
        lifetime: AuthorizationLifetime::Once { operation_id: op.operation_id },
        consent: None,
        memory_consent: None,
        mcp_consent: None,
        file_consent: None,
        network_consent: None,
        capability_limit: None,
    };
    let mut slices = Vec::new();
    let mut missing = Vec::new();
    let caps: std::collections::BTreeSet<_> = op.content.capabilities.iter().copied().collect();
    for capability in caps {
        let authority =
            capability_authority_inner(input, capability, false, assessment.forced_once).filter(
                |authority| match authority {
                    CapabilityAuthority::Grant { scope, .. } => {
                        backend.supports_operation(&scope.backend_requirement, op)
                    }
                    _ => true,
                },
            );
        if let Some(authority) = authority {
            slices.push(CapabilityAuthorization {
                capability,
                identity: op.content.identity.clone(),
                caller: input.network_caller,
                resources: op.content.resources.clone(),
                authority,
            });
        } else {
            missing.push(capability);
        }
    }
    if !missing.is_empty() {
        if missing.is_empty() {
            missing = op.content.capabilities.clone();
        }
        scope.capability_limit = Some(missing.clone());
        return PolicyAuthorization::Ask {
            proposal: AuthorizationProposal {
                scope,
                requested_capabilities: Some(missing.clone()),
                native_session_consent: if missing.contains(&ResolvedCapability::ProcessExecute) {
                    native_session_consent_scope(
                        &op.content.source,
                        &op.content.action_kind,
                        op.content.identity.principal_id,
                    )
                    .map(|scope| NativeSessionConsentFact {
                        shell: scope.approved_shell,
                        shells: scope.shells,
                        principal_id: scope.principal_id,
                        session_permission_revision: input.snapshot.session_permission_revision,
                    })
                } else {
                    None
                },
                allowed_decisions: if assessment.forced_once
                    || op.content.action_kind == "git.sync"
                    || package_program_read_only(input)
                    // A page with no site (blank, a workspace file) has nothing to name.
                    || crate::browser::shape(input)
                        && aura_security_contract::browser_site(&op.content.resources).is_none()
                {
                    vec![ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny]
                } else {
                    vec![
                        ApprovalDecisionKind::AllowOnce,
                        ApprovalDecisionKind::AllowSession,
                        ApprovalDecisionKind::Deny,
                    ]
                },
                reasons: vec!["policy.approval_required".into()],
            },
        };
    }
    PolicyAuthorization::Allow {
        authorization: AuthorizationDecision {
            basis: AuthorizationBasis::Composite { slices },
            scope,
            decided_at: input.now,
            expires_at: input.expires_at,
        },
    }
}
