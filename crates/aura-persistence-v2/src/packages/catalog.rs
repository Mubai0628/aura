//! Read-only package catalog for settings pages and the Agent registry.
use super::revision_from_sql;
use aura_domain_common::{CanonicalSemVer, ToolPackageId, UtcTimestamp};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
use aura_tool_contract::{PackageFingerprintV1, PackageKey};
use aura_tool_domain::{
    PackageConformanceStateV1, PackageContentDescriptorV1, PackageVersionBindingV1,
};
use rusqlite::{Connection, OptionalExtension, params};
use std::str::FromStr;

#[derive(Clone, Debug)]
pub struct CatalogVersion {
    pub binding: PackageVersionBindingV1,
    pub conformance: PackageConformanceStateV1,
    pub created_at: UtcTimestamp,
    /// `None` once the payload was collected; the version row stays for audit.
    pub descriptor: Option<PackageContentDescriptorV1>,
}

#[derive(Clone, Debug)]
pub struct CatalogEntry {
    pub package_id: ToolPackageId,
    pub package_key: PackageKey,
    pub revision: aura_domain_common::Revision,
    pub enabled: bool,
    /// Trust names exactly the current version and fingerprint.
    pub trusted: bool,
    /// The package revision the current trust was granted at; changes only on install.
    pub trust_revision: Option<aura_domain_common::Revision>,
    /// The installed version, when one is current.
    pub installed: Option<PackageVersionBindingV1>,
    /// Newest first.
    pub versions: Vec<CatalogVersion>,
}

impl CatalogEntry {
    #[must_use]
    pub fn installed_version(&self) -> Option<&CatalogVersion> {
        let installed = self.installed.as_ref()?;
        self.versions.iter().find(|version| version.binding == *installed)
    }
    /// Installed, trusted, enabled and its content still present.
    #[must_use]
    pub fn eligible(&self) -> bool {
        self.enabled
            && self.trusted
            && self.installed_version().is_some_and(|version| version.descriptor.is_some())
    }
}

pub fn catalog(db: &Connection) -> StoreResult<Vec<CatalogEntry>> {
    let ids = {
        let mut query = db.prepare("SELECT id FROM tool_packages ORDER BY package_key")?;
        query.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?
    };
    let mut entries = Vec::with_capacity(ids.len());
    for id in ids {
        let id = ToolPackageId::from_str(&id).map_err(|_| Error::Invalid("package identity"))?;
        if let Some(entry) = package(db, id)? {
            entries.push(entry);
        }
    }
    Ok(entries)
}

pub fn package(db: &Connection, id: ToolPackageId) -> StoreResult<Option<CatalogEntry>> {
    type Row = (String, Option<String>, Option<String>, bool, i64, Option<i64>);
    let row: Option<Row> = db
        .query_row(
            "SELECT p.package_key,p.current_version,p.current_fingerprint,p.enabled,p.revision,(SELECT t.revision FROM tool_package_trust t WHERE t.package_id=p.id AND t.version=p.current_version AND t.fingerprint=p.current_fingerprint) FROM tool_packages p WHERE p.id=?1",
            [id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        )
        .optional()?;
    let Some((key, current_version, current_fingerprint, enabled, revision, trust_revision)) = row
    else {
        return Ok(None);
    };
    let versions = versions(db, id)?;
    let installed = match (current_version, current_fingerprint) {
        (Some(version), Some(fingerprint)) => Some(
            versions
                .iter()
                .find(|candidate| {
                    candidate.binding.package_version.to_string() == version
                        && candidate.binding.package_fingerprint.0.hex() == fingerprint
                })
                .map(|candidate| candidate.binding.clone())
                .ok_or(Error::Invalid("package.current_version_missing"))?,
        ),
        _ => None,
    };
    Ok(Some(CatalogEntry {
        package_id: id,
        package_key: PackageKey::new(key).map_err(|_| Error::Invalid("package key"))?,
        revision: revision_from_sql(revision)?,
        enabled,
        trusted: trust_revision.is_some(),
        trust_revision: trust_revision.map(revision_from_sql).transpose()?,
        installed,
        versions,
    }))
}

fn versions(db: &Connection, id: ToolPackageId) -> StoreResult<Vec<CatalogVersion>> {
    type Row = (String, String, i64, String, String, Option<String>);
    let mut query = db.prepare(
        "SELECT v.version,v.fingerprint,v.revision,v.conformance_state,v.created_at,p.descriptor FROM tool_package_versions v LEFT JOIN tool_package_payloads p ON p.fingerprint=v.payload_fingerprint WHERE v.package_id=?1 ORDER BY v.created_at DESC, v.version DESC",
    )?;
    let rows = query
        .query_map(params![id.to_string()], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
        })?
        .collect::<Result<Vec<Row>, _>>()?;
    rows.into_iter()
        .map(|(version, fingerprint, revision, state, created_at, descriptor)| {
            let descriptor: Option<PackageContentDescriptorV1> =
                descriptor.map(|value| record::decode(&value)).transpose()?;
            let binding = PackageVersionBindingV1 {
                package_id: id,
                package_version: CanonicalSemVer::from_str(&version)
                    .map_err(|_| Error::Invalid("package version"))?,
                package_fingerprint: PackageFingerprintV1(
                    aura_domain_common::jcs::CanonicalSha256HashV1::new(fingerprint)
                        .map_err(|_| Error::Invalid("package fingerprint"))?,
                ),
                revision: revision_from_sql(revision)?,
            };
            if descriptor.as_ref().is_some_and(|value| {
                value.package_fingerprint != binding.package_fingerprint
                    || value.manifest.package_version != binding.package_version
            }) {
                return Err(Error::Invalid("package version identity"));
            }
            Ok(CatalogVersion {
                binding,
                conformance: serde_json::from_value(serde_json::Value::String(state))
                    .map_err(|_| Error::Invalid("package conformance state"))?,
                created_at: created_at
                    .parse()
                    .map_err(|_| Error::Invalid("package import time"))?,
                descriptor,
            })
        })
        .collect()
}
