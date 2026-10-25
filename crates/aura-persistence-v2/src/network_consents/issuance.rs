use aura_domain_common::{ApprovalRequestId, PermissionGrantId, Revision, ToolWireVersionV1};
use aura_execution_contract::SecuredOperation;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use aura_security_contract::*;

/// Adds only the capabilities in the stored question. Already-covered slices never become
/// part of this new reusable grant. The original grant records remain independent.
pub(crate) fn attach(
    scope: &mut AuthorizationScope,
    operation: &SecuredOperation,
    approval: ApprovalRequestId,
    grant: PermissionGrantId,
) -> StoreResult<()> {
    if !matches!(scope.lifetime, AuthorizationLifetime::Session { .. }) {
        return Ok(());
    }
    let Some(caps) = scope.capability_limit.as_ref() else {
        return Ok(());
    };
    let c = &operation.content;
    if caps.is_empty() || caps.iter().any(|cap| !c.capabilities.contains(cap)) {
        return Err(Error::Conflict);
    }
    // A session answer for a package tool names that package version only. It
    // covers the asked slices (running and, for a networked program, its network).
    if let Some((package_id, package_fingerprint, _)) =
        package_tool_target(&c.source, &c.action_kind, &c.resources)
    {
        if caps.iter().any(|cap| {
            !matches!(cap, ResolvedCapability::ProcessExecute | ResolvedCapability::OrdinaryNetwork)
        }) {
            return Err(Error::Conflict);
        }
        scope.network_consent = Some(NetworkSessionConsentRef {
            version: ToolWireVersionV1,
            approval_id: approval,
            grant_id: grant,
            grant_revision: Revision::INITIAL,
            workspace_id: c.identity.workspace_id.ok_or(Error::Conflict)?,
            session_id: c.identity.session_id.ok_or(Error::Conflict)?,
            principal_id: c.identity.principal_id,
            caller: NetworkConsentCaller::PackageTools,
            destinations: NetworkConsentDestinations::Package {
                package_id: *package_id,
                package_fingerprint: package_fingerprint.clone(),
            },
            approved_operation_fingerprint: operation.fingerprint.clone(),
            revoked_grant_revision: None,
        });
        return Ok(());
    }
    // A session answer in the built-in browser names the site the call was on, and covers
    // the asked slices (opening it, acting on it) for this session only.
    if browser_tool(&c.source, &c.action_kind) {
        if caps.iter().any(|cap| {
            !matches!(cap, ResolvedCapability::BrowserRead | ResolvedCapability::BrowserInteraction)
        }) {
            return Err(Error::Conflict);
        }
        let (host, _) = browser_site(&c.resources).ok_or(Error::Conflict)?;
        scope.network_consent = Some(NetworkSessionConsentRef {
            version: ToolWireVersionV1,
            approval_id: approval,
            grant_id: grant,
            grant_revision: Revision::INITIAL,
            workspace_id: c.identity.workspace_id.ok_or(Error::Conflict)?,
            session_id: c.identity.session_id.ok_or(Error::Conflict)?,
            principal_id: c.identity.principal_id,
            caller: NetworkConsentCaller::AuraBrowser,
            destinations: NetworkConsentDestinations::BrowserSite { host: host.to_owned() },
            approved_operation_fingerprint: operation.fingerprint.clone(),
            revoked_grant_revision: None,
        });
        return Ok(());
    }
    if caps.contains(&ResolvedCapability::OrdinaryNetwork)
        || caps.contains(&ResolvedCapability::GitRemoteWrite)
    {
        let (caller, destinations) = if caps.contains(&ResolvedCapability::GitRemoteWrite) {
            let target =
                git_push_consent_target(&c.source, &c.action_kind, &c.capabilities, &c.resources)
                    .ok_or(Error::Conflict)?;
            (NetworkConsentCaller::Git, NetworkConsentDestinations::Git { target: target.clone() })
        } else {
            (
                ordinary_tool_network_caller(&c.source, &c.action_kind).ok_or(Error::Conflict)?,
                NetworkConsentDestinations::AllOrdinaryDestinations,
            )
        };
        scope.network_consent = Some(NetworkSessionConsentRef {
            version: ToolWireVersionV1,
            approval_id: approval,
            grant_id: grant,
            grant_revision: Revision::INITIAL,
            workspace_id: c.identity.workspace_id.ok_or(Error::Conflict)?,
            session_id: c.identity.session_id.ok_or(Error::Conflict)?,
            principal_id: c.identity.principal_id,
            caller,
            destinations,
            approved_operation_fingerprint: operation.fingerprint.clone(),
            revoked_grant_revision: None,
        });
    }
    let files: Vec<_> =
        caps.iter().copied().filter(|cap| file_capabilities_are_consentable(&[*cap])).collect();
    if !files.is_empty() {
        let mut targets: Vec<_> = c.resources.iter().filter_map(file_consent_target_of).collect();
        targets.sort();
        targets.dedup();
        if targets.is_empty() {
            return Err(Error::Conflict);
        }
        scope.file_consent = Some(FileSessionConsentRef {
            approval_id: approval,
            session_id: c.identity.session_id.ok_or(Error::Conflict)?,
            principal_id: c.identity.principal_id,
            capabilities: file_consent_capabilities(&files),
            targets,
            approved_operation_fingerprint: operation.fingerprint.clone(),
            grant_revision: Revision::INITIAL,
            revoked_grant_revision: None,
        });
    }
    Ok(())
}

/// One dialog can answer both questions, but withdrawing either answer must leave the other
/// intact. Both records retain the same approval audit identity and the exact missing slices.
pub(crate) fn split(grant: &mut AuthorizationGrant) -> Option<AuthorizationGrant> {
    if (grant.scope.file_consent.is_none() && grant.scope.consent.is_none())
        || grant.scope.network_consent.is_none()
    {
        return None;
    }
    let mut network = grant.scope.network_consent.take()?;
    network.grant_id = PermissionGrantId::new();
    let mut separate = grant.clone();
    separate.grant_id = network.grant_id;
    separate.scope.file_consent = None;
    separate.scope.consent = None;
    separate.scope.network_consent = Some(network);
    separate.scope.capability_limit = Some(vec![ResolvedCapability::OrdinaryNetwork]);
    if let Some(caps) = grant.scope.capability_limit.as_mut() {
        caps.retain(|cap| *cap != ResolvedCapability::OrdinaryNetwork);
    }
    Some(separate)
}
