//! Enable, disable, revoke trust and uninstall, each an explicit user change bound to
//! the package revision the user saw. Uninstall removes the version references and
//! collects every payload no running call or Conformance still references.
use super::{conformance::append, content, revision_from_sql};
use aura_domain_common::{ArtifactId, Revision, ToolPackageId, UtcTimestamp};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact};
use rusqlite::{OptionalExtension, Transaction, params};
use std::str::FromStr;

fn bump(
    tx: &Transaction<'_>,
    id: ToolPackageId,
    expected: Revision,
) -> StoreResult<(Revision, bool, bool)> {
    let row: Option<(i64, bool, bool)> = tx
        .query_row(
            "SELECT revision,current_fingerprint IS NOT NULL,enabled FROM tool_packages WHERE id=?1",
            [id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let (revision, installed, enabled) = row.ok_or(Error::Invalid("package.not_found"))?;
    if revision_from_sql(revision)? != expected {
        return Err(Error::Conflict);
    }
    let next = expected
        .value()
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or(Error::Invalid("package.revision_exhausted"))?;
    Ok((Revision::new(next), installed, enabled))
}

pub fn set_enabled(
    tx: &Transaction<'_>,
    id: ToolPackageId,
    expected: Revision,
    enabled: bool,
    at: UtcTimestamp,
) -> StoreResult<Revision> {
    let (next, installed, current) = bump(tx, id, expected)?;
    if !installed {
        return Err(Error::Invalid("package.not_installed"));
    }
    if enabled {
        // Enabling never confers trust: a revoked package needs a new install approval.
        let trusted: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM tool_package_trust t JOIN tool_packages p ON p.id=t.package_id AND p.current_version=t.version AND p.current_fingerprint=t.fingerprint WHERE p.id=?1)",
            [id.to_string()],
            |row| row.get(0),
        )?;
        if !trusted {
            return Err(Error::Invalid("package.not_trusted"));
        }
    }
    tx.execute(
        "UPDATE tool_packages SET enabled=?2,revision=?3 WHERE id=?1",
        params![id.to_string(), enabled, next.value() as i64],
    )?;
    if !enabled {
        crate::network_consents::revoke_package(tx, id, at)?;
    }
    append(
        tx,
        if enabled { "package.enabled" } else { "package.disabled" },
        &serde_json::json!({"package_id":id,"previous_enabled":current,"revision":next}),
        at,
    )?;
    Ok(next)
}

pub fn revoke_trust(
    tx: &Transaction<'_>,
    id: ToolPackageId,
    expected: Revision,
    at: UtcTimestamp,
) -> StoreResult<Revision> {
    let (next, installed, _) = bump(tx, id, expected)?;
    if !installed {
        return Err(Error::Invalid("package.not_installed"));
    }
    tx.execute("DELETE FROM tool_package_trust WHERE package_id=?1", [id.to_string()])?;
    tx.execute(
        "UPDATE tool_packages SET enabled=0,revision=?2 WHERE id=?1",
        params![id.to_string(), next.value() as i64],
    )?;
    crate::network_consents::revoke_package(tx, id, at)?;
    append(tx, "package.trust_revoked", &serde_json::json!({"package_id":id,"revision":next}), at)?;
    Ok(next)
}

/// Removes the installed version, trust, settings, credential bindings and every
/// version reference. Content still referenced by a running call stays until that
/// reference is released; [`collect_unreferenced`] removes the rest.
pub fn uninstall(
    tx: &Transaction<'_>,
    id: ToolPackageId,
    expected: Revision,
    at: UtcTimestamp,
) -> StoreResult<Revision> {
    let (next, _, _) = bump(tx, id, expected)?;
    let fingerprints = {
        let mut query = tx.prepare(
            "SELECT DISTINCT fingerprint FROM tool_package_versions WHERE package_id=?1 AND payload_fingerprint IS NOT NULL",
        )?;
        query
            .query_map([id.to_string()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    tx.execute("DELETE FROM tool_package_trust WHERE package_id=?1", [id.to_string()])?;
    tx.execute("DELETE FROM tool_package_configurations WHERE package_id=?1", [id.to_string()])?;
    tx.execute(
        "DELETE FROM tool_package_credential_bindings WHERE package_id=?1",
        [id.to_string()],
    )?;
    tx.execute(
        "UPDATE tool_packages SET current_version=NULL,current_fingerprint=NULL,enabled=0,revision=?2 WHERE id=?1",
        params![id.to_string(), next.value() as i64],
    )?;
    crate::network_consents::revoke_package(tx, id, at)?;
    for fingerprint in &fingerprints {
        tx.execute(
            "DELETE FROM tool_package_references WHERE fingerprint=?1 AND owner_kind='version' AND owner_id LIKE ?2",
            params![fingerprint, format!("{id}/%")],
        )?;
        tx.execute(
            "INSERT INTO tool_package_deletions VALUES(?1,?2,?3,'pending',?4,NULL)",
            params![
                format!("tpd_{}", aura_domain_common::CorrelationId::new()),
                id.to_string(),
                fingerprint,
                at.to_string()
            ],
        )?;
    }
    append(
        tx,
        "package.uninstalled",
        &serde_json::json!({"package_id":id,"revision":next,"payloads":fingerprints}),
        at,
    )?;
    Ok(next)
}

/// Collects payloads nothing references: their files, artifacts and payload rows.
/// Version rows stay for audit with no content. Returns how many payloads went.
pub fn collect_unreferenced(tx: &Transaction<'_>, at: UtcTimestamp) -> StoreResult<usize> {
    let fingerprints = {
        let mut query =
            tx.prepare("SELECT fingerprint FROM tool_package_payloads WHERE ref_count=0")?;
        query.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?
    };
    for fingerprint in &fingerprints {
        let artifacts = {
            let mut query =
                tx.prepare("SELECT artifact_id FROM tool_package_files WHERE fingerprint=?1")?;
            query
                .query_map([fingerprint], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        tx.execute("DELETE FROM tool_package_files WHERE fingerprint=?1", [fingerprint])?;
        for id in artifacts {
            let id = ArtifactId::from_str(&id)
                .map_err(|_| Error::Invalid("package artifact identity"))?;
            artifact::release_owner(tx, id, &content::owner(fingerprint))?;
        }
        tx.execute(
            "UPDATE tool_package_versions SET payload_fingerprint=NULL WHERE payload_fingerprint=?1",
            [fingerprint],
        )?;
        tx.execute("DELETE FROM tool_package_payloads WHERE fingerprint=?1", [fingerprint])?;
        tx.execute(
            "UPDATE tool_package_deletions SET state='collected',collected_at=?2 WHERE fingerprint=?1 AND state='pending'",
            params![fingerprint, at.to_string()],
        )?;
    }
    if !fingerprints.is_empty() {
        artifact::collect(tx)?;
        append(tx, "package.content_collected", &serde_json::json!({"payloads":fingerprints}), at)?;
    }
    Ok(fingerprints.len())
}
