use aura_domain_common::{Revision, ToolPackageId};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use aura_tool_contract::PackageKey;
use rusqlite::{Connection, OptionalExtension};

#[derive(Clone, Debug)]
pub struct PackageState {
    pub package_id: ToolPackageId,
    pub package_key: PackageKey,
    pub installed: bool,
    pub enabled: bool,
    pub trusted: bool,
    pub revision: Revision,
}

pub fn state(db: &Connection, id: ToolPackageId) -> StoreResult<Option<PackageState>> {
    let row: Option<(String, bool, bool, bool, i64)> = db.query_row(
        "SELECT p.package_key,p.current_fingerprint IS NOT NULL,p.enabled,EXISTS(SELECT 1 FROM tool_package_trust t WHERE t.package_id=p.id AND t.version=p.current_version AND t.fingerprint=p.current_fingerprint),p.revision FROM tool_packages p WHERE p.id=?1",
        [id.to_string()], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)),
    ).optional()?;
    row.map(|(key, installed, enabled, trusted, revision)| {
        Ok(PackageState {
            package_id: id,
            package_key: PackageKey::new(key).map_err(|_| Error::Invalid("package key"))?,
            installed,
            enabled,
            trusted,
            revision: revision_from_sql(revision)?,
        })
    })
    .transpose()
}

pub(super) fn revision_from_sql(value: i64) -> StoreResult<Revision> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .map(Revision::new)
        .ok_or(Error::Invalid("package revision"))
}
