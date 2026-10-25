//! Installed per-tool settings. Written only by install, read by execution.
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
use aura_tool_domain::{PackageToolSettingsV1, PackageVersionBindingV1};
use rusqlite::{Connection, Transaction, params};

pub(super) fn replace_installed(
    tx: &Transaction<'_>,
    version: &PackageVersionBindingV1,
    settings: &[PackageToolSettingsV1],
) -> StoreResult<()> {
    tx.execute(
        "DELETE FROM tool_package_configurations WHERE package_id=?1",
        [version.package_id.to_string()],
    )?;
    for entry in settings {
        tx.execute(
            "INSERT INTO tool_package_configurations VALUES(?1,?2,?3,?4,?5)",
            params![
                version.package_id.to_string(),
                version.package_version.to_string(),
                version.package_fingerprint.0.hex(),
                entry.tool_key.as_str(),
                record::encode(entry)?
            ],
        )?;
    }
    Ok(())
}

/// Settings of the installed version, in tool-key order. A row for any other version
/// than the one named is an integrity failure, never a fallback.
pub fn installed_settings(
    db: &Connection,
    version: &PackageVersionBindingV1,
) -> StoreResult<Vec<PackageToolSettingsV1>> {
    let mut query = db.prepare(
        "SELECT version,fingerprint,tool_key,settings FROM tool_package_configurations WHERE package_id=?1 ORDER BY tool_key",
    )?;
    let rows = query
        .query_map([version.package_id.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut settings = Vec::with_capacity(rows.len());
    for (stored_version, fingerprint, tool_key, json) in rows {
        let entry: PackageToolSettingsV1 = record::decode(&json)?;
        if stored_version != version.package_version.to_string()
            || fingerprint != version.package_fingerprint.0.hex()
            || entry.tool_key.as_str() != tool_key
        {
            return Err(Error::Invalid("package.settings_binding"));
        }
        settings.push(entry);
    }
    Ok(settings)
}
