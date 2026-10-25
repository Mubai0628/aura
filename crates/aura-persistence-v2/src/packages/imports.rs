use super::content::{self, PackageFileBytes};
use aura_domain_common::{EventId, ToolPackageId, UtcTimestamp};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, event, record};
use aura_tool_contract::TOOL_PACKAGE_MANIFEST_NAME;
use aura_tool_domain::{PackageContentDescriptorV1, PackageVersionBindingV1};
use rusqlite::{OptionalExtension, Transaction, params};
use std::str::FromStr;

/// Core supplies facts obtained from the authoritative strict stager. This
/// transaction also verifies file bytes, stores them with P02 Artifact ownership,
/// and records the import. It never installs, trusts or enables a tool.
pub fn import_content(
    tx: &Transaction<'_>,
    preferred_id: ToolPackageId,
    descriptor: &PackageContentDescriptorV1,
    files: &[PackageFileBytes<'_>],
    at: UtcTimestamp,
) -> StoreResult<PackageVersionBindingV1> {
    let canonical = content::validate_files(descriptor, files)?;
    let fingerprint = descriptor.package_fingerprint.0.hex();
    let stored: Option<String> = tx
        .query_row(
            "SELECT descriptor FROM tool_package_payloads WHERE fingerprint=?1",
            [fingerprint],
            |row| row.get(0),
        )
        .optional()?;
    let reused = stored.is_some();
    if let Some(stored) = stored {
        let stored: PackageContentDescriptorV1 = record::decode(&stored)?;
        if stored != *descriptor {
            return Err(Error::Invalid("package fingerprint collision"));
        }
    } else {
        tx.execute(
            "INSERT INTO tool_package_payloads(fingerprint,descriptor) VALUES(?1,?2)",
            params![fingerprint, record::encode(descriptor)?],
        )?;
        content::put_file(tx, fingerprint, TOOL_PACKAGE_MANIFEST_NAME, &canonical, at)?;
        for file in files {
            content::put_file(tx, fingerprint, file.path.as_str(), file.bytes, at)?;
        }
    }
    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM tool_packages WHERE package_key=?1",
            [descriptor.manifest.package_key.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    let id = if let Some(existing) = existing {
        ToolPackageId::from_str(&existing).map_err(|_| Error::Invalid("package identity"))?
    } else {
        tx.execute(
            "INSERT INTO tool_packages(id,package_key,revision) VALUES(?1,?2,1)",
            params![preferred_id.to_string(), descriptor.manifest.package_key.as_str()],
        )?;
        preferred_id
    };
    let version = descriptor.manifest.package_version.to_string();
    let existing: Option<(i64,Option<String>)> = tx.query_row(
        "SELECT revision,payload_fingerprint FROM tool_package_versions WHERE package_id=?1 AND version=?2 AND fingerprint=?3",
        params![id.to_string(),version,fingerprint], |row| Ok((row.get(0)?,row.get(1)?)),
    ).optional()?;
    let revision = match existing {
        Some((revision, Some(payload))) => {
            if payload != fingerprint {
                return Err(Error::Invalid("package payload identity"));
            }
            super::revision_from_sql(revision)?
        }
        missing => {
            let revision = missing.map_or(Ok(1), |(revision, _)| {
                revision.checked_add(1).ok_or(Error::Invalid("package revision exhausted"))
            })?;
            tx.execute("INSERT INTO tool_package_versions VALUES(?1,?2,?3,?3,?4,'not_run',NULL,?5) ON CONFLICT(package_id,version,fingerprint) DO UPDATE SET payload_fingerprint=excluded.payload_fingerprint,revision=excluded.revision,conformance_state='not_run',conformance=NULL",
            params![id.to_string(),version,fingerprint,revision,at.to_string()])?;
            tx.execute(
                "INSERT INTO tool_package_references VALUES(?1,'version',?2)",
                params![fingerprint, format!("{id}/{version}/{fingerprint}")],
            )?;
            super::revision_from_sql(revision)?
        }
    };
    let binding = PackageVersionBindingV1 {
        package_id: id,
        package_version: descriptor.manifest.package_version.clone(),
        package_fingerprint: descriptor.package_fingerprint.clone(),
        revision,
    };
    // Reuse is an integrity operation too: a damaged object cannot hide behind
    // an existing fingerprint and become eligible for later approval.
    content::load_content(tx, &binding)?;
    event::append(
        tx,
        &event::EventDraft {
            id: EventId::new(),
            owner: "tool",
            kind: "package.imported",
            version: 1,
            payload: &serde_json::json!({"version":binding,"reused_content":reused}),
            at,
        },
        "tool_packages",
    )?;
    Ok(binding)
}
