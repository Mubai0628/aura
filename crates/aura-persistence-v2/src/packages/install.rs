//! Explicit installation checks the exact completed run and both revision domains.
use aura_domain_common::jcs::CanonicalSha256HashV1;
use aura_domain_common::{Revision, UtcTimestamp};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use aura_tool_domain::{PackageConformanceStateV1, PackageVersionBindingV1};
use rusqlite::{Transaction, params};

pub fn install(
    tx: &Transaction<'_>,
    version: &PackageVersionBindingV1,
    expected_package_revision: Revision,
    expected_report_hash: &CanonicalSha256HashV1,
    at: UtcTimestamp,
) -> StoreResult<Revision> {
    // Calling this operation is the Core's explicit authenticated install approval;
    // the import and Conformance paths never call it implicitly.
    let content = super::load_content(tx, version)?;
    let before =
        super::state(tx, version.package_id)?.ok_or(Error::Invalid("package.not_found"))?;
    if before.revision != expected_package_revision {
        return Err(Error::Conflict);
    }
    let run = super::conformance::conformance(tx, version)?
        .ok_or(Error::Invalid("package.conformance_missing"))?;
    let report = run.report.ok_or(Error::Invalid("package.conformance_incomplete"))?;
    if report.version != *version
        || report.state() != PackageConformanceStateV1::Succeeded
        || super::conformance::report_hash(&report)? != *expected_report_hash
    {
        return Err(Error::Invalid("package.conformance_not_approved"));
    }
    if at < report.completed_at {
        return Err(Error::Invalid("package.install_time"));
    }
    // D1: program tools run with the user's identity and the Native
    // exception never covers credential isolation, so their credential slots are refused.
    // HTTP slots remain supported through Core's controlled HTTP.
    if content.descriptor.manifest.tools.iter().any(|tool| {
        match tool.runtime.to_runtime_descriptor_v1() {
            aura_tool_contract::ToolRuntimeDescriptorV1::AuraStdio { credential_slot, .. } => {
                credential_slot.is_some()
            }
            aura_tool_contract::ToolRuntimeDescriptorV1::DeclarativeCommand { command } => {
                match command {
                    aura_tool_contract::DeclarativeCommandV1::Argv { credential_slot, .. }
                    | aura_tool_contract::DeclarativeCommandV1::PowershellScript {
                        credential_slot,
                        ..
                    } => credential_slot.is_some(),
                }
            }
            _ => false,
        }
    }) {
        return Err(Error::Invalid("package.program_credentials_unsupported"));
    }
    let next = before
        .revision
        .value()
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or(Error::Invalid("package.revision_exhausted"))?;
    let count = tx.execute("UPDATE tool_packages SET current_version=?2,current_fingerprint=?3,enabled=1,revision=?4 WHERE id=?1 AND revision=?5",
        params![version.package_id.to_string(),version.package_version.to_string(),version.package_fingerprint.0.hex(),next as i64,expected_package_revision.value() as i64])?;
    if count != 1 {
        return Err(Error::Conflict);
    }
    tx.execute("INSERT INTO tool_package_trust VALUES(?1,?2,?3,?4,?5) ON CONFLICT(package_id) DO UPDATE SET version=excluded.version,fingerprint=excluded.fingerprint,revision=excluded.revision,granted_at=excluded.granted_at",
        params![version.package_id.to_string(),version.package_version.to_string(),version.package_fingerprint.0.hex(),next as i64,at.to_string()])?;
    // Installed settings are exactly the ones the approved run used; any earlier
    // version's settings stop applying with its trust, and so do its session answers.
    super::settings::replace_installed(tx, version, &run.settings)?;
    crate::network_consents::revoke_package(tx, version.package_id, at)?;
    super::conformance::append(
        tx,
        "package.installed",
        &serde_json::json!({"version":version,"previous_revision":before.revision,"revision":Revision::new(next),"conformance_hash":expected_report_hash,"trusted":true,"enabled":true}),
        at,
    )?;
    Ok(Revision::new(next))
}
