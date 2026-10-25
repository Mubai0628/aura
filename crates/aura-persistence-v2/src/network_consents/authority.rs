use aura_domain_common::{PermissionGrantId, UtcTimestamp};
use aura_execution_contract::SecuredOperation;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use aura_security_contract::*;
use rusqlite::{Connection, Transaction};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn reserve(
    tx: &Transaction<'_>,
    decision: &AuthorizationDecision,
    operation: &SecuredOperation,
    live_revision: u64,
    now: UtcTimestamp,
) -> StoreResult<()> {
    validate(tx, decision, operation, now, true)?;
    let AuthorizationBasis::Composite { slices } = &decision.basis else {
        return Err(Error::Conflict);
    };
    let mut grants = BTreeMap::new();
    for slice in slices {
        match &slice.authority {
            CapabilityAuthority::PermissionMode { session_permission_revision, .. }
                if *session_permission_revision != live_revision =>
            {
                return Err(Error::Conflict);
            }
            CapabilityAuthority::Grant { grant_id, revision, scope } => {
                grants.insert(
                    *grant_id,
                    AuthorizationDecision {
                        basis: AuthorizationBasis::Grant {
                            grant_id: *grant_id,
                            revision: *revision,
                        },
                        scope: *scope.clone(),
                        decided_at: decision.decided_at,
                        expires_at: decision.expires_at,
                    },
                );
            }
            _ => {}
        }
    }
    // Multiple capabilities can share one Once grant. Consume it exactly once, only after all
    // dependencies validate; any failure rolls this and the Attempt reservation back together.
    for authority in grants.values() {
        crate::security::reserve_grant(tx, authority, live_revision, now)?;
    }
    Ok(())
}

pub(crate) fn validate_dispatch(
    db: &Connection,
    decision: &AuthorizationDecision,
    operation: &SecuredOperation,
    now: UtcTimestamp,
) -> StoreResult<()> {
    validate(db, decision, operation, now, false)
}

fn validate(
    db: &Connection,
    decision: &AuthorizationDecision,
    op: &SecuredOperation,
    now: UtcTimestamp,
    reserving: bool,
) -> StoreResult<()> {
    let AuthorizationBasis::Composite { slices } = &decision.basis else {
        return Err(Error::Conflict);
    };
    let c = &op.content;
    let session = c.identity.session_id.ok_or(Error::Conflict)?;
    let owner = crate::workspace::session(db, session)?.ok_or(Error::Conflict)?;
    let expected_caller = network_caller_for(&c.action_kind);
    if c.identity.workspace_id != Some(owner.workspace_id)
        || !(ordinary_tool_network_caller(&c.source, &c.action_kind).is_some()
            && (c.capabilities.contains(&ResolvedCapability::OrdinaryNetwork)
                || built_in_shell(&c.source, &c.action_kind)
                || package_program(&c.source, &c.action_kind, &c.capabilities, &c.resources))
            || native_git_remote(&c.source, &c.action_kind, &c.capabilities)
                && git_remote_facts(&c.resources).is_some()
            // The built-in browser decides each capability as its own slice.
            || built_in_browser(&c.source, &c.action_kind, &c.capabilities, &c.resources))
        || !decision.scope.is_exact_operation()
        || decision.scope.capability_limit.is_some()
        || decision.scope.operation_fingerprint != op.fingerprint
        || !matches!(decision.scope.lifetime,AuthorizationLifetime::Once { operation_id } if operation_id == op.operation_id)
        || decision.decided_at > now
        || decision.expires_at <= now
        || slices.len() != c.capabilities.iter().collect::<BTreeSet<_>>().len()
    {
        return Err(Error::Conflict);
    }
    let mut seen = BTreeSet::new();
    let mut grants: BTreeMap<PermissionGrantId, AuthorizationScope> = BTreeMap::new();
    for slice in slices {
        if slice.identity != c.identity
            || slice.resources != c.resources
            || slice.caller != expected_caller
            || !seen.insert(slice.capability)
            || !c.capabilities.contains(&slice.capability)
        {
            return Err(Error::Conflict);
        }
        match &slice.authority {
            CapabilityAuthority::PermissionMode {
                snapshot_id,
                mode,
                session_permission_revision,
            } => {
                let snapshot =
                    crate::security::snapshot(db, *snapshot_id)?.ok_or(Error::Conflict)?;
                if c.identity.permission_snapshot_id != Some(*snapshot_id)
                    || snapshot.session_id != session
                    || c.identity.workspace_id != Some(snapshot.workspace_id)
                    || snapshot.mode != *mode
                    || snapshot.session_permission_revision != *session_permission_revision
                    || !ordinary_mode_covers_capability(
                        *mode,
                        &c.source,
                        &c.action_kind,
                        slice.capability,
                        &c.resources,
                    )
                {
                    return Err(Error::Conflict);
                }
            }
            CapabilityAuthority::Grant { grant_id, revision, scope } => {
                let grant = crate::security::grant(db, *grant_id)?.ok_or(Error::Conflict)?;
                if grant.scope.consent.is_some()
                    && !crate::security::native_session_consent(db, *grant_id)?
                        .is_some_and(|record| record.matches_grant(&grant))
                {
                    return Err(Error::Conflict);
                }
                if grant.revision != *revision
                    || grant.issued_at > now
                    || grant.revoked_at.is_some()
                    || reserving && grant.consumed_at.is_some()
                    || grant
                        .expires_at
                        .is_some_and(|expiry| now >= expiry || decision.expires_at > expiry)
                    || !crate::security::scope_authorizes(&grant.scope, scope)
                    || scope.operation_fingerprint != op.fingerprint
                    || !grant_covers_capability(
                        &grant,
                        &op.fingerprint,
                        &c.identity,
                        &c.source,
                        &c.action_kind,
                        slice.capability,
                        &c.resources,
                    )
                    || !lifetime_matches(&grant.scope.lifetime, op)
                    || grants.get(grant_id).is_some_and(|previous| previous != scope.as_ref())
                {
                    return Err(Error::Conflict);
                }
                grants.insert(*grant_id, *scope.clone());
            }
            CapabilityAuthority::ExternalGrant { grant_id, revalidation_revision } => {
                if *revalidation_revision == 0
                    || !file_capabilities_are_consentable(&[slice.capability])
                    || !c
                        .resources
                        .contains(&ResourceDescriptor::ExternalGrant { grant_id: *grant_id })
                {
                    return Err(Error::Conflict);
                }
                // This legacy authority is validated by its owning resolver, not by decoding
                // a similarly named row in the V2 database. The dispatch guard verifies the
                // exact revision again under the same Core authority barrier.
            }
        }
    }
    Ok(())
}

fn lifetime_matches(lifetime: &AuthorizationLifetime, op: &SecuredOperation) -> bool {
    match lifetime {
        AuthorizationLifetime::Once { operation_id } => *operation_id == op.operation_id,
        AuthorizationLifetime::Session { session_id } => {
            Some(*session_id) == op.content.identity.session_id
        }
        AuthorizationLifetime::TurnTask { session_id, turn_task_id } => {
            Some(*session_id) == op.content.identity.session_id
                && Some(*turn_task_id) == op.content.identity.turn_task_id
        }
        AuthorizationLifetime::Workspace { workspace_id } => {
            Some(*workspace_id) == op.content.identity.workspace_id
        }
    }
}
