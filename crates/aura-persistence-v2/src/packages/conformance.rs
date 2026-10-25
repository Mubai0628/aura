//! Atomic run records. Only a terminal from the actual runtime may be finalized.
use aura_domain_common::jcs::{CanonicalSha256HashV1, canonical_sha256};
use aura_domain_common::{EventId, Revision, ToolConformanceRunId, UtcTimestamp};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, event, record};
use aura_tool_domain::{
    PackageConformanceReportV1, PackageConformanceStateV1, PackageToolSettingsV1,
    PackageVersionBindingV1, canonical_package_settings_v1, package_settings_hash_v1,
};
use rusqlite::{Connection, Transaction, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageRunRecord {
    pub run_id: ToolConformanceRunId,
    pub version: PackageVersionBindingV1,
    pub started_at: UtcTimestamp,
    pub state: PackageConformanceStateV1,
    pub report: Option<PackageConformanceReportV1>,
    /// Canonical settings every case of this run used; install copies exactly these.
    #[serde(default)]
    pub settings: Vec<PackageToolSettingsV1>,
}

pub fn begin_conformance(
    tx: &Transaction<'_>,
    binding: &PackageVersionBindingV1,
    run_id: ToolConformanceRunId,
    settings: &[PackageToolSettingsV1],
    at: UtcTimestamp,
) -> StoreResult<PackageVersionBindingV1> {
    let content = super::load_content(tx, binding)?;
    if canonical_package_settings_v1(&content.descriptor.manifest, settings)
        .map_err(|_| Error::Invalid("package.settings_invalid"))?
        != settings
    {
        return Err(Error::Invalid("package.settings_not_canonical"));
    }
    let _ = conformance(tx, binding)?;
    let current = version_revision(tx, binding)?;
    if current != binding.revision {
        return Err(Error::Conflict);
    }
    let unresolved: i64 = tx.query_row(
        "SELECT count(*) FROM tool_package_references WHERE fingerprint=?1 AND owner_kind='execution' AND owner_id LIKE 'package-conformance:%'",
        [binding.package_fingerprint.0.hex()], |row| row.get(0),
    )?;
    if unresolved != 0 {
        return Err(Error::Invalid("package.conformance_owner_unsettled"));
    }
    let next = current
        .value()
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or(Error::Invalid("package.revision_exhausted"))?;
    let version = PackageVersionBindingV1 { revision: Revision::new(next), ..binding.clone() };
    let run = PackageRunRecord {
        run_id,
        version: version.clone(),
        started_at: at,
        state: PackageConformanceStateV1::Running,
        report: None,
        settings: settings.to_vec(),
    };
    let changed = tx.execute("UPDATE tool_package_versions SET revision=?4,conformance_state='running',conformance=?5 WHERE package_id=?1 AND version=?2 AND fingerprint=?3 AND revision=?6 AND conformance_state!='running' AND payload_fingerprint IS NOT NULL",
        params![binding.package_id.to_string(),binding.package_version.to_string(),binding.package_fingerprint.0.hex(),next as i64,record::encode(&run)?,current.value() as i64])?;
    if changed != 1 {
        return Err(Error::Conflict);
    }
    tx.execute(
        "INSERT INTO tool_package_references VALUES(?1,'execution',?2)",
        params![binding.package_fingerprint.0.hex(), format!("package-conformance:{run_id}")],
    )?;
    append(
        tx,
        "package.conformance_started",
        &serde_json::json!({"version":version,"run_id":run_id}),
        at,
    )?;
    Ok(version)
}

pub fn conformance(
    db: &Connection,
    binding: &PackageVersionBindingV1,
) -> StoreResult<Option<PackageRunRecord>> {
    if version_revision(db, binding)? != binding.revision {
        return Err(Error::Conflict);
    }
    let (state,json): (String,Option<String>) = db.query_row("SELECT conformance_state,conformance FROM tool_package_versions WHERE package_id=?1 AND version=?2 AND fingerprint=?3",
        params![binding.package_id.to_string(),binding.package_version.to_string(),binding.package_fingerprint.0.hex()], |row| Ok((row.get(0)?,row.get(1)?)))?;
    let run = json.map(|value| record::decode::<PackageRunRecord>(&value)).transpose()?;
    match &run {
        Some(run)
            if run.version == *binding
                && serde_json::to_value(run.state)?.as_str() == Some(state.as_str()) =>
        {
            match &run.report {
                Some(report)
                    if report.run_id == run.run_id
                        && report.version == *binding
                        && report.started_at == run.started_at
                        && serde_json::to_value(report.state())?.as_str()
                            == Some(state.as_str()) => {}
                None if matches!(state.as_str(), "running" | "interrupted") => {}
                _ => return Err(Error::Invalid("package.conformance_binding")),
            }
        }
        None if state == "not_run" => {}
        _ => return Err(Error::Invalid("package.conformance_binding")),
    }
    Ok(run)
}

pub fn finish_conformance(
    tx: &Transaction<'_>,
    report: &PackageConformanceReportV1,
) -> StoreResult<()> {
    let run =
        conformance(tx, &report.version)?.ok_or(Error::Invalid("package.conformance_missing"))?;
    if run.run_id != report.run_id
        || run.report.is_some()
        || run.started_at != report.started_at
        || report.completed_at < report.started_at
        || package_settings_hash_v1(&run.settings)
            .map_err(|_| Error::Invalid("package.settings_hash"))?
            != report.settings_hash
    {
        return Err(Error::Conflict);
    }
    verify_report(tx, report)?;
    let state = serde_json::to_value(report.state())?
        .as_str()
        .ok_or(Error::Invalid("package.conformance_state"))?
        .to_owned();
    let completed = PackageRunRecord { report: Some(report.clone()), state: report.state(), ..run };
    let changed = tx.execute("UPDATE tool_package_versions SET conformance_state=?4,conformance=?5 WHERE package_id=?1 AND version=?2 AND fingerprint=?3 AND revision=?6 AND conformance_state='running'",
        params![report.version.package_id.to_string(),report.version.package_version.to_string(),report.version.package_fingerprint.0.hex(),state,record::encode(&completed)?,report.version.revision.value() as i64])?;
    if changed != 1 {
        return Err(Error::Conflict);
    }
    let reference_count: i64 = tx.query_row(
        "SELECT count(*) FROM tool_package_references WHERE fingerprint=?1 AND owner_kind='execution' AND owner_id=?2",
        params![report.version.package_fingerprint.0.hex(),format!("package-conformance:{}",report.run_id)], |row| row.get(0),
    )?;
    if reference_count != 1 {
        return Err(Error::Invalid("package.conformance_reference_missing"));
    }
    if report.cleanup_verified {
        tx.execute("DELETE FROM tool_package_references WHERE fingerprint=?1 AND owner_kind='execution' AND owner_id=?2",
            params![report.version.package_fingerprint.0.hex(),format!("package-conformance:{}",report.run_id)])?;
    }
    append(
        tx,
        "package.conformance_finished",
        &serde_json::json!({"version":report.version,"run_id":report.run_id,"state":report.state(),"report_hash":report_hash(report)?,"execution_reference_retained":!report.cleanup_verified}),
        report.completed_at,
    )
}

pub fn report_hash(report: &PackageConformanceReportV1) -> StoreResult<CanonicalSha256HashV1> {
    canonical_sha256(report).map_err(|_| Error::Invalid("package.conformance_hash"))
}

/// Called once by the new Core package service on startup, after its old process
/// and kill-on-close Jobs are gone. A crash never becomes a successful test.
pub fn interrupt_conformance(tx: &Transaction<'_>, at: UtcTimestamp) -> StoreResult<usize> {
    let rows = {
        let mut query = tx.prepare(
            "SELECT conformance FROM tool_package_versions WHERE conformance_state='running'",
        )?;
        query.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?
    };
    for row in &rows {
        let mut run: PackageRunRecord = record::decode(row)?;
        let previous = run.version.revision;
        let next = previous
            .value()
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or(Error::Invalid("package.revision_exhausted"))?;
        run.version.revision = Revision::new(next);
        run.state = PackageConformanceStateV1::Interrupted;
        run.report = None;
        let count=tx.execute("UPDATE tool_package_versions SET revision=?4,conformance_state='interrupted',conformance=?5 WHERE package_id=?1 AND version=?2 AND fingerprint=?3 AND revision=?6 AND conformance_state='running'",
            params![run.version.package_id.to_string(),run.version.package_version.to_string(),run.version.package_fingerprint.0.hex(),next as i64,record::encode(&run)?,previous.value() as i64])?;
        if count != 1 {
            return Err(Error::Conflict);
        }
        tx.execute("DELETE FROM tool_package_references WHERE fingerprint=?1 AND owner_kind='execution' AND owner_id=?2",
            params![run.version.package_fingerprint.0.hex(),format!("package-conformance:{}",run.run_id)])?;
        append(
            tx,
            "package.conformance_interrupted",
            &serde_json::json!({"version":run.version,"run_id":run.run_id,"automatic_replay":false}),
            at,
        )?;
    }
    // Terminal reports with an unconfirmed stop/cleanup also retain content.
    // Only startup, after the previous Core and its Jobs are gone, may release
    // these references. The original Interrupted report remains unchanged.
    let terminals = {
        let mut query = tx.prepare("SELECT conformance FROM tool_package_versions WHERE conformance_state!='running' AND conformance IS NOT NULL")?;
        query.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?
    };
    for json in terminals {
        let run: PackageRunRecord = record::decode(&json)?;
        if run.report.as_ref().is_some_and(|report| !report.cleanup_verified) {
            let count = tx.execute("DELETE FROM tool_package_references WHERE fingerprint=?1 AND owner_kind='execution' AND owner_id=?2",
                params![run.version.package_fingerprint.0.hex(),format!("package-conformance:{}",run.run_id)])?;
            if count != 0 {
                append(
                    tx,
                    "package.conformance_reference_recovered",
                    &serde_json::json!({"version":run.version,"run_id":run.run_id,"automatic_replay":false}),
                    at,
                )?;
            }
        }
    }
    Ok(rows.len())
}

pub(super) fn version_revision(
    db: &Connection,
    binding: &PackageVersionBindingV1,
) -> StoreResult<Revision> {
    let revision = db.query_row("SELECT revision FROM tool_package_versions WHERE package_id=?1 AND version=?2 AND fingerprint=?3",
        params![binding.package_id.to_string(),binding.package_version.to_string(),binding.package_fingerprint.0.hex()],|row| row.get::<_,i64>(0))?;
    super::revision_from_sql(revision)
}

fn verify_report(db: &Connection, report: &PackageConformanceReportV1) -> StoreResult<()> {
    let content = super::load_content(db, &report.version)?;
    let mut expected = content
        .descriptor
        .manifest
        .tools
        .iter()
        .flat_map(|tool| {
            let location = if matches!(
                tool.runtime,
                aura_tool_domain::CanonicalToolPackageRuntimeV1::DeclarativeHttp { .. }
            ) {
                aura_tool_domain::PackageExecutionLocationV1::ControlledHttp
            } else {
                aura_tool_domain::PackageExecutionLocationV1::UserJob
            };
            tool.conformance_cases
                .iter()
                .map(move |case| (tool.tool_key.as_str(), case.name.as_str(), location))
        })
        .collect::<Vec<_>>();
    let mut actual = report
        .cases
        .iter()
        .map(|case| (case.tool_key.as_str(), case.case_name.as_str(), case.execution_location))
        .collect::<Vec<_>>();
    expected.sort_by_key(|value| (value.0, value.1));
    actual.sort_by_key(|value| (value.0, value.1));
    if expected.is_empty() || actual != expected {
        return Err(Error::Invalid("package.conformance_case_set"));
    }
    Ok(())
}

pub(super) fn append(
    tx: &Transaction<'_>,
    kind: &'static str,
    payload: &serde_json::Value,
    at: UtcTimestamp,
) -> StoreResult<()> {
    event::append(
        tx,
        &event::EventDraft { id: EventId::new(), owner: "tool", kind, version: 1, payload, at },
        "tool_packages",
    )?;
    Ok(())
}
