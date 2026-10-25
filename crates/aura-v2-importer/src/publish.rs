use crate::{
    Error, ImportDisposition as D, ImportReport, StoreResult, item, record, source::PreparedImport,
};
use aura_domain_common::UtcTimestamp;
use aura_persistence_v2::V2Store;
use rusqlite::{OptionalExtension, params};
use std::{collections::BTreeMap, io::Write, path::Path};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ImportFault {
    BeforeCommit,
    BeforePublish,
}

/// Only publishes to a new target. Files and SQLite are complete in a private
/// sibling staging directory before rename; readers never see half an import.
pub fn apply(
    plan: &PreparedImport,
    target: &Path,
    fault: Option<ImportFault>,
) -> StoreResult<ImportReport> {
    if !target.is_absolute() {
        return Err(Error::Invalid("absolute import target"));
    }
    plan.verify_sources()?;
    if target.exists() {
        return existing(plan, target);
    }
    let parent = target
        .parent()
        .filter(|_| target.is_absolute())
        .ok_or(Error::Invalid("absolute import target"))?;
    let temp = tempfile::Builder::new().prefix(".aura-r02-import-").tempdir_in(parent)?;
    let stage = temp.path().join("ready");
    let mut store = V2Store::create(&stage)?;
    for (name, bytes) in &plan.configurations {
        if !crate::configurations::is_config_path(name) {
            return Err(Error::Invalid("configuration path whitelist"));
        }
        if let Some(parent) = stage.join(name).parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_new(&stage.join(name), bytes)?;
    }
    let mut report = plan.report.clone();
    report.applied = true;
    let encoded = record::encode(&report)?;
    let manifest: BTreeMap<String, String> = plan
        .configurations
        .iter()
        .map(|(name, bytes)| (name.clone(), blake3::hash(bytes).to_hex().to_string()))
        .collect();
    store.database.unit_of_work(|tx| {
        for w in &plan.workspaces {
            aura_persistence_v2::workspace::insert(tx, w)?;
        }
        for s in &plan.sessions {
            aura_persistence_v2::workspace::insert_session(tx, s)?;
        }
        for (s, p) in &plan.bindings {
            aura_persistence_v2::model::bind_session(tx, *s, *p, None)?;
        }
        tx.execute(
            "INSERT INTO import_batches VALUES(?1,?2,?3,?4)",
            params![
                record::encode(&report.source_fingerprint)?,
                encoded,
                record::encode(&manifest)?,
                UtcTimestamp::now().to_string()
            ],
        )?;
        for (ordinal, i) in report.items.iter().enumerate() {
            tx.execute(
                "INSERT INTO import_items VALUES(?1,?2,?3,?4,?5)",
                params![
                    record::encode(&report.source_fingerprint)?,
                    i.kind,
                    format!("{}:{ordinal}", i.source_key),
                    blake3::hash(record::encode(i)?.as_bytes()).to_hex().as_str(),
                    record::encode(&i.disposition)?
                ],
            )?;
        }
        if fault == Some(ImportFault::BeforeCommit) {
            return Err(Error::Invalid("injected import rollback"));
        }
        Ok(())
    })?;
    aura_persistence_core::schema::verify(
        store.database.connection(),
        aura_persistence_v2::MODULES,
    )?;
    store.database.connection().execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
    drop(store);
    write_new(&stage.join("import-report.json"), encoded.as_bytes())?;
    write_new(&stage.join("import-files.json"), record::encode(&manifest)?.as_bytes())?;
    plan.verify_sources()?;
    if fault == Some(ImportFault::BeforePublish) {
        return Err(Error::Invalid("injected publication rollback"));
    }
    resume(&stage, target)
}

fn write_new(path: &Path, bytes: &[u8]) -> StoreResult<()> {
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

/// Recover a fully committed private stage after a process crash. Partial or
/// tampered stages fail closed; no legacy database or config is ever written.
pub fn resume(stage: &Path, target: &Path) -> StoreResult<ImportReport> {
    if !stage.is_absolute() || !target.is_absolute() {
        return Err(Error::Invalid("absolute recovery stage and target"));
    }
    if target.exists() {
        return Err(Error::Conflict);
    }
    let store = V2Store::open(stage)?;
    let report: ImportReport =
        serde_json::from_slice(&std::fs::read(stage.join("import-report.json"))?)?;
    let persisted: String = store.database.connection().query_row(
        "SELECT report FROM import_batches WHERE source_hash=?1",
        [record::encode(&report.source_fingerprint)?],
        |r| r.get(0),
    )?;
    if !report.applied || record::decode::<ImportReport>(&persisted)? != report {
        return Err(Error::Conflict);
    }
    let files: BTreeMap<String, String> =
        serde_json::from_slice(&std::fs::read(stage.join("import-files.json"))?)?;
    let stored_files: String = store.database.connection().query_row(
        "SELECT file_manifest FROM import_batches WHERE source_hash=?1",
        [record::encode(&report.source_fingerprint)?],
        |r| r.get(0),
    )?;
    if record::decode::<BTreeMap<String, String>>(&stored_files)? != files
        || !files.contains_key("models.json")
        || !files.contains_key("aura.toml")
    {
        return Err(Error::Conflict);
    }
    for (name, hash) in files {
        if !crate::configurations::is_config_path(&name)
            || blake3::hash(&std::fs::read(stage.join(&name))?).to_hex().as_str() != hash
        {
            return Err(Error::Invalid("import file manifest"));
        }
    }
    drop(store);
    std::fs::rename(stage, target)?;
    Ok(report)
}

fn existing(plan: &PreparedImport, target: &Path) -> StoreResult<ImportReport> {
    let store = V2Store::open(target)?;
    let stored: Option<String> = store
        .database
        .connection()
        .query_row(
            "SELECT report FROM import_batches WHERE source_hash=?1",
            [record::encode(&plan.report.source_fingerprint)?],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(stored) = stored {
        let mut report: ImportReport = record::decode(&stored)?;
        report.already_applied = true;
        return Ok(report);
    }
    let mut report = plan.report.clone();
    report.applied = false;
    item(&mut report, "target", "target", D::Conflict, "different_import_or_existing_database");
    Ok(report)
}
