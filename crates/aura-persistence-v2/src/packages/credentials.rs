//! The user's explicit choice of credential for one HTTP package tool slot (D3).
//! The binding names a credential; the credential store keeps the record and the secret,
//! and every use re-checks state, revision and allowed websites.
use super::revision_from_sql;
use aura_domain_common::{CredentialId, CredentialRevision, Revision, ToolPackageId, UtcTimestamp};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
use aura_tool_contract::{CredentialSlotKey, ToolKey};
use aura_tool_domain::PackageCredentialBindingV1;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredBinding {
    credential_id: CredentialId,
    credential_revision: CredentialRevision,
    bound_at: UtcTimestamp,
}

pub fn binding(
    db: &Connection,
    package: ToolPackageId,
    tool: &ToolKey,
    slot: &CredentialSlotKey,
) -> StoreResult<Option<PackageCredentialBindingV1>> {
    let row: Option<(i64, String)> = db
        .query_row(
            "SELECT revision,binding FROM tool_package_credential_bindings WHERE package_id=?1 AND tool_key=?2 AND slot_key=?3",
            params![package.to_string(), tool.as_str(), slot.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    row.map(|(revision, json)| {
        let stored: StoredBinding = record::decode(&json)?;
        Ok(PackageCredentialBindingV1 {
            credential_id: stored.credential_id,
            credential_revision: stored.credential_revision,
            revision: revision_from_sql(revision)?,
            bound_at: stored.bound_at,
        })
    })
    .transpose()
}

/// Binds or replaces. Core has already checked that the tool declares this slot and
/// that the credential exists for it; this only enforces the revision the user saw.
pub fn bind(
    tx: &Transaction<'_>,
    request: &aura_tool_domain::PackageCredentialBindRequestV1,
    at: UtcTimestamp,
) -> StoreResult<PackageCredentialBindingV1> {
    let package = request.package_id;
    if super::state(tx, package)?.is_none() {
        return Err(Error::Invalid("package.not_found"));
    }
    let current = binding(tx, package, &request.tool_key, &request.slot_key)?;
    if current.as_ref().map(|value| value.revision) != request.expected_binding_revision {
        return Err(Error::Conflict);
    }
    let next = request
        .expected_binding_revision
        .map_or(Some(1), |value| value.value().checked_add(1))
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or(Error::Invalid("package.binding_revision_exhausted"))?;
    let stored = StoredBinding {
        credential_id: request.credential_id,
        credential_revision: request.credential_revision,
        bound_at: at,
    };
    tx.execute(
        "INSERT INTO tool_package_credential_bindings VALUES(?1,?2,?3,?4,?5) ON CONFLICT(package_id,tool_key,slot_key) DO UPDATE SET revision=excluded.revision,binding=excluded.binding",
        params![
            package.to_string(),
            request.tool_key.as_str(),
            request.slot_key.as_str(),
            next as i64,
            record::encode(&stored)?
        ],
    )?;
    super::conformance::append(
        tx,
        "package.credential_bound",
        &serde_json::json!({"package_id":package,"tool_key":request.tool_key,"slot_key":request.slot_key,"credential_id":request.credential_id,"credential_revision":request.credential_revision,"revision":next}),
        at,
    )?;
    Ok(PackageCredentialBindingV1 {
        credential_id: request.credential_id,
        credential_revision: request.credential_revision,
        revision: Revision::new(next),
        bound_at: at,
    })
}

pub fn clear(
    tx: &Transaction<'_>,
    package: ToolPackageId,
    tool: &ToolKey,
    slot: &CredentialSlotKey,
    expected: Revision,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let changed = tx.execute(
        "DELETE FROM tool_package_credential_bindings WHERE package_id=?1 AND tool_key=?2 AND slot_key=?3 AND revision=?4",
        params![package.to_string(), tool.as_str(), slot.as_str(), expected.value() as i64],
    )?;
    if changed != 1 {
        return Err(Error::Conflict);
    }
    super::conformance::append(
        tx,
        "package.credential_cleared",
        &serde_json::json!({"package_id":package,"tool_key":tool,"slot_key":slot,"revision":expected}),
        at,
    )
}
