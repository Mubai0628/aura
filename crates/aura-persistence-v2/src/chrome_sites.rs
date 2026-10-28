//! The sites a session's user confirmed for every Chrome tab of the session.
use aura_domain_common::SessionId;
use aura_persistence_core::{StoreResult, record};
use aura_security_contract::AuthorizationGrant;
use rusqlite::Connection;
use std::collections::BTreeSet;

/// The confirmed sites, from the session's live site answers.
pub fn confirmed(connection: &Connection, session: SessionId) -> StoreResult<BTreeSet<String>> {
    let mut query = connection.prepare(
        "SELECT payload FROM authorization_grants \
         WHERE json_extract(payload,'$.scope.chrome_site_consent.session_id')=?1",
    )?;
    let mut sites = BTreeSet::new();
    for payload in query.query_map([session.to_string()], |row| row.get::<_, String>(0))? {
        let grant: AuthorizationGrant = record::decode(&payload?)?;
        if let Some(consent) = &grant.scope.chrome_site_consent
            && consent.session_id == session
            && grant.revoked_at.is_none()
            && grant.revision == consent.grant_revision
        {
            sites.insert(consent.origin.clone());
        }
    }
    Ok(sites)
}
