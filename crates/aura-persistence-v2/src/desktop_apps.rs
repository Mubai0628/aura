//! The programs a session's user authorized for the session, from the session's live answers in
//! the approval records.
use aura_domain_common::{SessionId, UtcTimestamp};
use aura_persistence_core::{StoreResult, record};
use aura_security_contract::AuthorizationGrant;
use rusqlite::{Connection, Transaction};
use std::collections::BTreeSet;

fn answers(connection: &Connection, session: SessionId) -> StoreResult<Vec<AuthorizationGrant>> {
    let mut query = connection.prepare(
        "SELECT payload FROM authorization_grants \
         WHERE json_extract(payload,'$.scope.desktop_app_consent.session_id')=?1 ORDER BY id",
    )?;
    let mut live = Vec::new();
    for payload in query.query_map([session.to_string()], |row| row.get::<_, String>(0))? {
        let grant: AuthorizationGrant = record::decode(&payload?)?;
        if let Some(consent) = &grant.scope.desktop_app_consent
            && consent.session_id == session
            && grant.revoked_at.is_none()
            && grant.revision == consent.grant_revision
        {
            live.push(grant);
        }
    }
    Ok(live)
}

/// The catalog IDs of the programs the session authorized.
pub fn authorized(connection: &Connection, session: SessionId) -> StoreResult<BTreeSet<String>> {
    Ok(answers(connection, session)?
        .into_iter()
        .filter_map(|grant| grant.scope.desktop_app_consent.map(|consent| consent.app_id))
        .collect())
}

/// Withdraws the session's answers for one program; returns how many there were.
pub fn revoke(
    tx: &Transaction<'_>,
    session: SessionId,
    app_id: &str,
    now: UtcTimestamp,
) -> StoreResult<usize> {
    let mut revoked = 0;
    for grant in answers(tx, session)? {
        if grant.scope.desktop_app_consent.as_ref().is_some_and(|consent| consent.app_id == app_id)
        {
            crate::security::revoke(tx, grant.grant_id, grant.revision, None, now)?;
            revoked += 1;
        }
    }
    Ok(revoked)
}
