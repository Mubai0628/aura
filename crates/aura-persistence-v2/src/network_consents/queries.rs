use aura_domain_common::{PermissionGrantId, Revision, SessionId, ToolPackageId, UtcTimestamp};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
use aura_security_contract::{AuthorizationGrant, NetworkConsentDestinations, ResolvedCapability};
use rusqlite::{Connection, Transaction};

pub fn list(db: &Connection, session: SessionId) -> StoreResult<Vec<AuthorizationGrant>> {
    if crate::workspace::session(db, session)?.is_none() {
        return Ok(vec![]);
    }
    let mut query = db.prepare("SELECT payload FROM authorization_grants WHERE json_extract(payload,'$.scope.network_consent.session_id')=?1 ORDER BY id")?;
    query
        .query_map([session.to_string()], |row| row.get::<_, String>(0))?
        .map(|row| record::decode(&row?))
        .collect()
}

/// The sites this session's standing browser answers let the model page open. The browser host
/// lets a ReadOnly session's page navigate to them, as it does to the user's allowed sites.
pub fn browser_hosts(db: &Connection, session: SessionId) -> StoreResult<Vec<String>> {
    let mut hosts: Vec<String> = list(db, session)?
        .into_iter()
        .filter(|grant| {
            grant.revoked_at.is_none()
                && grant
                    .scope
                    .capability_limit
                    .as_ref()
                    .is_some_and(|caps| caps.contains(&ResolvedCapability::BrowserRead))
        })
        .filter_map(|grant| match grant.scope.network_consent?.destinations {
            NetworkConsentDestinations::BrowserSite { host } => Some(host),
            _ => None,
        })
        .collect();
    hosts.sort();
    hosts.dedup();
    Ok(hosts)
}

pub fn revoke(
    tx: &Transaction<'_>,
    session: SessionId,
    grant_id: PermissionGrantId,
    revision: Revision,
    now: UtcTimestamp,
) -> StoreResult<()> {
    let grant = crate::security::grant(tx, grant_id)?.ok_or(Error::Conflict)?;
    let reference = grant.scope.network_consent.as_ref().ok_or(Error::Conflict)?;
    if reference.session_id != session
        || reference.grant_id != grant_id
        || reference.grant_revision != revision
        || crate::workspace::session(tx, session)?.is_none()
    {
        return Err(Error::Conflict);
    }
    crate::security::revoke(tx, grant_id, revision, None, now)
}

/// A disable, a trust revocation, an uninstall or a newly installed version ends every
/// session answer for the package, so its next call is a new question.
pub(crate) fn revoke_package(
    tx: &Transaction<'_>,
    package: ToolPackageId,
    now: UtcTimestamp,
) -> StoreResult<usize> {
    let payloads = {
        let mut query = tx.prepare("SELECT payload FROM authorization_grants WHERE revoked=0 AND json_extract(payload,'$.scope.network_consent') IS NOT NULL ORDER BY id")?;
        query.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?
    };
    let mut revoked = 0;
    for payload in payloads {
        let grant: AuthorizationGrant = record::decode(&payload)?;
        let answers_package = grant.revoked_at.is_none()
            && grant.scope.network_consent.as_ref().is_some_and(|consent| {
                matches!(
                    &consent.destinations,
                    NetworkConsentDestinations::Package { package_id, .. } if *package_id == package
                )
            });
        if answers_package {
            crate::security::revoke(tx, grant.grant_id, grant.revision, None, now)?;
            revoked += 1;
        }
    }
    Ok(revoked)
}
