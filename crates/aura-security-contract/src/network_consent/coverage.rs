//! Pure grant coverage, shared by policy and the transactional authority owner.
use crate::*;

/// Validates one capability against its original approval. The surrounding caller still
/// verifies liveness, session existence, exact source/configuration and backend guarantees.
#[must_use]
pub fn grant_covers_capability(
    grant: &AuthorizationGrant,
    fingerprint: &OperationFingerprint,
    identity: &InvocationIdentity,
    source: &SecuritySourceRef,
    action: &str,
    capability: ResolvedCapability,
    resources: &[ResourceDescriptor],
) -> bool {
    if grant.scope.memory_consent.is_some() {
        return crate::memory_consent_covers(
            grant,
            identity,
            source,
            action,
            &[capability],
            resources,
        );
    }
    if shell_consent_covers_capability(grant, identity, source, action, capability) {
        return true;
    }
    if let Some(consent) = &grant.scope.network_consent
        && consent.covers(grant, identity, source, action, capability, resources)
    {
        return true;
    }
    if let Some(file) = &grant.scope.file_consent {
        let targets: Vec<_> = resources.iter().filter_map(file_consent_target_of).collect();
        if grant.revision == file.grant_revision
            && file.revoked_grant_revision.is_none()
            && identity.principal_id == file.principal_id
            && identity.session_id == Some(file.session_id)
            && matches!(grant.scope.lifetime, AuthorizationLifetime::Session { session_id } if session_id == file.session_id)
            && file.capabilities.contains(&capability)
            && !targets.is_empty()
            && targets.iter().all(|target| {
                file.targets.iter().any(|approved| file_consent_target_covers(approved, target))
            })
        {
            return true;
        }
    }
    // A reusable consent is never an alternative spelling of an exact unrestricted grant.
    grant.scope.is_exact_operation()
        && grant.scope.operation_fingerprint == *fingerprint
        && grant.scope.capability_limit.as_ref().is_none_or(|caps| caps.contains(&capability))
}

/// Defaults used for an individual slice. Network never taints a file slice with its own
/// Ask, and a protected file never taints the otherwise authorized network slice.
#[must_use]
pub fn ordinary_mode_covers_capability(
    mode: PermissionMode,
    source: &SecuritySourceRef,
    action: &str,
    capability: ResolvedCapability,
    resources: &[ResourceDescriptor],
) -> bool {
    // The built-in browser's own defaults; ordinary network approval is not part of them.
    if crate::browser_tool(source, action) {
        return crate::browser_mode_covers(mode, capability, resources);
    }
    if matches!(source, SecuritySourceRef::BuiltInTool { .. })
        && match action {
            "git.sync" => matches!(
                capability,
                ResolvedCapability::GitRemoteRead | ResolvedCapability::GitWrite
            ),
            "git.push" => matches!(
                capability,
                ResolvedCapability::GitRemoteWrite | ResolvedCapability::GitDestructive
            ),
            _ => false,
        }
    {
        return crate::native_git_mode_covers(mode, capability);
    }
    if built_in_shell(source, action) {
        return match capability {
            ResolvedCapability::ProcessExecute => mode != PermissionMode::ReadOnly,
            ResolvedCapability::OrdinaryNetwork => mode == PermissionMode::FullAccess,
            _ => false,
        };
    }
    // A package program runs as the user like the terminal, but the owner's rule
    // is stricter by default: Workspace asks once per package per session, ReadOnly
    // asks every time, FullAccess does not ask. Its network follows the Shell.
    if crate::package_tool_target(source, action, resources)
        .is_some_and(|(_, _, runtime)| runtime == crate::PackageRuntimeClass::Program)
    {
        return matches!(
            capability,
            ResolvedCapability::ProcessExecute | ResolvedCapability::OrdinaryNetwork
        ) && mode == PermissionMode::FullAccess;
    }
    if capability == ResolvedCapability::OrdinaryNetwork {
        return ordinary_tool_network_caller(source, action).is_some()
            && mode != PermissionMode::ReadOnly;
    }
    if !file_capabilities_are_consentable(&[capability]) {
        return false;
    }
    if resources.iter().any(|r| {
        matches!(
            r,
            ResourceDescriptor::SystemPath { .. } | ResourceDescriptor::SensitiveUserPath { .. }
        )
    }) {
        return false;
    }
    match capability {
        ResolvedCapability::WorkspaceFileRead => true,
        ResolvedCapability::WorkspaceFileWrite
        | ResolvedCapability::WorkspaceFileCreate
        | ResolvedCapability::WorkspaceFileDelete
        | ResolvedCapability::WorkspaceFileMove => mode != PermissionMode::ReadOnly,
        _ => mode == PermissionMode::FullAccess,
    }
}
