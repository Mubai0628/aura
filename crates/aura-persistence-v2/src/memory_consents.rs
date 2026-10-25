//! A ReadOnly session's "allow for this session" on one memory Vault: listing and withdrawal.
use aura_domain_common::{PermissionGrantId, Revision, SessionId, UtcTimestamp};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
use aura_security_contract::AuthorizationGrant;
use rusqlite::{Connection, Transaction};

pub fn list(db: &Connection, session: SessionId) -> StoreResult<Vec<AuthorizationGrant>> {
    // A deleted session keeps no reusable authority even if its audit remains.
    if crate::workspace::session(db, session)?.is_none() {
        return Ok(vec![]);
    }
    let mut query = db.prepare("SELECT payload FROM authorization_grants WHERE json_extract(payload,'$.scope.memory_consent.session_id')=?1 ORDER BY id")?;
    query
        .query_map([session.to_string()], |row| row.get::<_, String>(0))?
        .map(|row| record::decode(&row?))
        .collect()
}

pub fn revoke(
    tx: &Transaction<'_>,
    session: SessionId,
    grant: PermissionGrantId,
    expected: Revision,
    now: UtcTimestamp,
) -> StoreResult<()> {
    let current = crate::security::grant(tx, grant)?.ok_or(Error::Conflict)?;
    if current.scope.memory_consent.as_ref().is_none_or(|c| c.session_id != session) {
        return Err(Error::Conflict);
    }
    crate::security::revoke(tx, grant, expected, None, now)
}
