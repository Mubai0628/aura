//! File consent lookup, withdrawal and receipt-bound target updates in the V2 authority store.
use aura_domain_common::{
    ApprovalRequestId, PermissionGrantId, PrincipalId, Revision, SessionId, UtcTimestamp,
    WorkspaceId,
};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
use aura_security_contract::{AuthorizationGrant, FileConsentProtection, FileConsentTarget};
use rusqlite::{Connection, Transaction, params};

pub fn list(db: &Connection, session: SessionId) -> StoreResult<Vec<AuthorizationGrant>> {
    // Deletion of the session removes its reusable authority even if historical audit remains.
    if crate::workspace::session(db, session)?.is_none() {
        return Ok(vec![]);
    }
    let mut query = db.prepare("SELECT payload FROM authorization_grants WHERE json_extract(payload,'$.scope.file_consent.session_id')=?1 ORDER BY id")?;
    query
        .query_map([session.to_string()], |row| row.get::<_, String>(0))?
        .map(|row| record::decode(&row?))
        .collect()
}

pub fn for_approval(
    db: &Connection,
    approval: ApprovalRequestId,
) -> StoreResult<Vec<AuthorizationGrant>> {
    let mut query = db.prepare("SELECT payload FROM authorization_grants WHERE json_extract(payload,'$.scope.file_consent.approval_id')=?1 ORDER BY id")?;
    query
        .query_map([approval.to_string()], |row| row.get::<_, String>(0))?
        .map(|row| record::decode(&row?))
        .collect()
}

/// A committed own publication may follow the approved object onto the replacement it made.
/// The supplied before-target is from the immutable Operation; the after-key is a native receipt.
pub fn published(
    tx: &Transaction<'_>,
    session: SessionId,
    principal: PrincipalId,
    before: &FileConsentTarget,
    after_key: &str,
    entry_key: &str,
    now: UtcTimestamp,
) -> StoreResult<()> {
    let grants = list(tx, session)?
        .into_iter()
        .filter(|grant| {
            grant
                .scope
                .file_consent
                .as_ref()
                .is_some_and(|consent| consent.principal_id == principal)
        })
        .collect();
    publish_grants(
        tx,
        grants,
        Publication {
            before_key: &before.identity_key,
            after_key,
            entry_key,
            protection: Some(before.protection),
        },
        now,
    )
}

/// An Aura editor publication follows existing approval in every live session of this workspace.
/// Only the same verified object at the same native entry can follow; no scope is expanded.
pub fn published_in_workspace(
    tx: &Transaction<'_>,
    workspace: WorkspaceId,
    before_key: &str,
    after_key: &str,
    entry_key: &str,
    now: UtcTimestamp,
) -> StoreResult<()> {
    let mut query = tx.prepare("SELECT g.payload FROM authorization_grants g JOIN sessions s ON s.id=json_extract(g.payload,'$.scope.file_consent.session_id') WHERE s.workspace_id=?1 ORDER BY g.id")?;
    let grants = query
        .query_map([workspace.to_string()], |row| row.get::<_, String>(0))?
        .map(|row| record::decode(&row?))
        .collect::<StoreResult<Vec<AuthorizationGrant>>>()?;
    publish_grants(
        tx,
        grants,
        Publication { before_key, after_key, entry_key, protection: None },
        now,
    )
}

struct Publication<'a> {
    before_key: &'a str,
    after_key: &'a str,
    entry_key: &'a str,
    protection: Option<FileConsentProtection>,
}

fn publish_grants(
    tx: &Transaction<'_>,
    grants: Vec<AuthorizationGrant>,
    publication: Publication<'_>,
    now: UtcTimestamp,
) -> StoreResult<()> {
    for mut grant in grants {
        if !grant.is_live_at(now) {
            continue;
        }
        let Some(consent) = grant.scope.file_consent.as_mut() else { continue };
        if consent.grant_revision != grant.revision {
            continue;
        }
        let mut changed = false;
        for target in &mut consent.targets {
            if target.identity_key == publication.before_key
                && publication.protection.is_none_or(|protection| target.protection == protection)
                && target.entry_key == publication.entry_key
            {
                target.identity_key = publication.after_key.to_owned();
                changed = true;
            }
        }
        if !changed {
            continue;
        }
        let previous = grant.revision;
        grant.revision = Revision::new(previous.value().checked_add(1).ok_or(Error::Conflict)?);
        consent.grant_revision = grant.revision;
        record::changed(tx.execute("UPDATE authorization_grants SET revision=?2,payload=?3 WHERE id=?1 AND revision=?4 AND revoked=0", params![
            grant.grant_id.to_string(), record::sql_u64(grant.revision.value())?, record::encode(&grant)?, record::sql_u64(previous.value())?])?)?;
        crate::security::audit(
            tx,
            &grant.grant_id.to_string(),
            "file_consent.own_publication",
            &grant,
            now,
        )?;
    }
    Ok(())
}

pub fn revoke(
    tx: &Transaction<'_>,
    session: SessionId,
    grant: PermissionGrantId,
    expected: Revision,
    now: UtcTimestamp,
) -> StoreResult<()> {
    let current = crate::security::grant(tx, grant)?.ok_or(Error::Conflict)?;
    if current.scope.file_consent.as_ref().is_none_or(|c| c.session_id != session) {
        return Err(Error::Conflict);
    }
    crate::security::revoke(tx, grant, expected, None, now)
}
