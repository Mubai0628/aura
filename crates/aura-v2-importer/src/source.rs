use crate::{Error, ImportDisposition as D, ImportReport, StoreResult, item};
use aura_domain_common::{
    ContentHash, FileIdentity, Revision, SessionId, ToolWireVersionV1, WorkspaceId,
};
use aura_persistence_v2::workspace::SessionMetadata;
use aura_workspace_domain::{CatalogFreshness, WorkspaceState, WorkspaceSummary};
use rusqlite::{Connection, OpenFlags};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct ImportInput {
    /// A closed, checkpointed Schema 42 snapshot, never an active WAL database.
    pub database: PathBuf,
    pub include_sessions: bool,
    pub models: Option<PathBuf>,
    pub settings: Option<PathBuf>,
    pub memory_vault: Option<PathBuf>,
    pub config_root: Option<PathBuf>,
}
pub struct PreparedImport {
    pub(crate) input: ImportInput,
    pub(crate) source_files: BTreeMap<PathBuf, Vec<u8>>,
    pub(crate) workspaces: Vec<WorkspaceSummary>,
    pub(crate) sessions: Vec<SessionMetadata>,
    pub(crate) configurations: BTreeMap<String, Vec<u8>>,
    pub(crate) bindings: Vec<(SessionId, aura_domain_common::ModelProfileId)>,
    pub(crate) report: ImportReport,
}

pub(crate) fn immutable(path: &Path) -> StoreResult<Connection> {
    if !path.is_absolute() || !path.is_file() {
        return Err(Error::Invalid("absolute legacy snapshot file"));
    }
    for suffix in ["-wal", "-journal"] {
        if PathBuf::from(format!("{}{suffix}", path.display())).exists() {
            return Err(Error::Invalid("legacy snapshot must be checkpointed and closed"));
        }
    }
    let mut uri = url::Url::from_file_path(path).map_err(|_| Error::Invalid("snapshot URI"))?;
    uri.set_query(Some("mode=ro&immutable=1"));
    let c = Connection::open_with_flags(
        uri.as_str(),
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    c.pragma_update(None, "query_only", "ON")?;
    aura_persistence_core::schema::integrity(&c)?;
    let version: u32 =
        c.query_row("SELECT max(version) FROM schema_migrations", [], |r| r.get(0))?;
    if version != 42 {
        return Err(Error::Invalid("only explicit Schema 42 snapshots can be imported"));
    }
    Ok(c)
}

pub fn backup_legacy(source: &Path, destination: &Path) -> StoreResult<()> {
    if !destination.is_absolute() {
        return Err(Error::Invalid("absolute new backup destination"));
    }
    let reader = immutable(source)?;
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(destination)?;
    drop(file);
    let mut target = Connection::open(destination)?;
    let backup = rusqlite::backup::Backup::new(&reader, &mut target)?;
    backup.run_to_completion(128, std::time::Duration::from_millis(5), None)?;
    drop(backup);
    aura_persistence_core::schema::integrity(&target)
}

pub fn preview(mut input: ImportInput) -> StoreResult<PreparedImport> {
    if let Some(root) = &input.config_root {
        if !root.is_absolute() || !root.is_dir() {
            return Err(Error::Invalid("absolute config input directory"));
        }
        if input.models.is_none() && root.join("models.json").is_file() {
            input.models = Some(root.join("models.json"));
        }
        if input.settings.is_none() && root.join("aura.toml").is_file() {
            input.settings = Some(root.join("aura.toml"));
        }
    }
    let c = immutable(&input.database)?;
    let mut files = BTreeMap::new();
    for path in
        std::iter::once(&input.database).chain(input.models.iter()).chain(input.settings.iter())
    {
        if !path.is_absolute() {
            return Err(Error::Invalid("absolute import source files"));
        }
        files.insert(path.clone(), std::fs::read(path)?);
    }
    if let Some(root) = &input.config_root {
        for module in crate::configurations::SETTINGS_MODULES {
            let path = root.join(module.relative_path());
            if path.is_file() {
                files.insert(path.clone(), std::fs::read(path)?);
            }
        }
    }
    let hash = ContentHash::blake3(
        &crate::record::encode(&(input.include_sessions, &input.memory_vault, &files))?
            .into_bytes(),
    );
    let report = ImportReport {
        schema_version: ToolWireVersionV1,
        source_fingerprint: hash,
        applied: false,
        already_applied: false,
        items: vec![],
    };
    let mut plan = PreparedImport {
        input,
        source_files: files,
        workspaces: vec![],
        sessions: vec![],
        configurations: BTreeMap::new(),
        bindings: vec![],
        report,
    };
    let mut q = c.prepare(
        "SELECT workspace_id,display_name,selected_root FROM workspaces ORDER BY workspace_id",
    )?;
    let rows = q.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
    })?;
    for row in rows {
        let (key, name, root) = row?;
        let Ok(id) = key.parse::<WorkspaceId>() else {
            item(&mut plan.report, "workspace", &key, D::Invalid, "invalid_id");
            continue;
        };
        let info = aura_windows_platform::inspect_existing_path(Path::new(&root), false);
        let Ok(info) = info else {
            item(&mut plan.report, "workspace", &key, D::RequiresUserAction, "root_unavailable");
            continue;
        };
        if !Path::new(&root).is_absolute()
            || !info.is_directory
            || info.is_reparse_point
            || name.trim().is_empty()
        {
            item(&mut plan.report, "workspace", &key, D::Invalid, "root_or_name_invalid");
            continue;
        }
        let canonical = info.final_path.to_string_lossy().into_owned();
        let identity = FileIdentity {
            volume_serial_number: info.identity.volume_serial_number.to_string(),
            file_id: info.identity.file_id.iter().map(|b| format!("{b:02x}")).collect(),
        };
        if plan.workspaces.iter().any(|w| {
            w.root_identity.as_ref() == Some(&identity)
                || w.canonical_root.eq_ignore_ascii_case(&canonical)
        }) {
            item(&mut plan.report, "workspace", &key, D::Conflict, "duplicate_root");
            continue;
        }
        plan.workspaces.push(WorkspaceSummary {
            workspace_id: id,
            display_name: name,
            selected_root: root,
            canonical_root: canonical,
            root_identity: Some(identity),
            state: WorkspaceState::Offline,
            revision: Revision::new(1),
            catalog_freshness: CatalogFreshness::Unavailable,
            authorization_epoch: Revision::new(1),
        });
        item(&mut plan.report, "workspace", &key, D::Imported, "root_revalidated_offline");
    }
    if plan.input.include_sessions {
        let mut q=c.prepare("SELECT session_id,workspace_id,title,created_at,updated_at FROM sessions ORDER BY session_id")?;
        for row in q.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })? {
            let (key, workspace, title, created, updated) = row?;
            let parsed = (
                key.parse::<SessionId>(),
                workspace.parse::<WorkspaceId>(),
                created.parse(),
                updated.parse(),
            );
            let (Ok(id), Ok(workspace_id), Ok(created_at), Ok(updated_at)) = parsed else {
                item(&mut plan.report, "session", &key, D::Invalid, "invalid_metadata");
                continue;
            };
            if title.trim().is_empty() || updated_at < created_at {
                item(&mut plan.report, "session", &key, D::Invalid, "invalid_metadata");
                continue;
            }
            if !plan.workspaces.iter().any(|w| w.workspace_id == workspace_id) {
                item(
                    &mut plan.report,
                    "session",
                    &key,
                    D::RequiresUserAction,
                    "workspace_not_imported",
                );
                continue;
            }
            plan.sessions.push(SessionMetadata { id, workspace_id, title, created_at, updated_at });
            item(&mut plan.report, "session", &key, D::Imported, "metadata_only_idle");
        }
    }
    crate::configurations::prepare(&mut plan, &c)?;
    let mut tables=c.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?;
    for name in tables.query_map([], |r| r.get::<_, String>(0))? {
        let name = name?;
        if name == "workspaces"
            || name == "sessions" && plan.input.include_sessions
            || name == "session_model_bindings"
        {
            continue;
        }
        // Only a table name from sqlite_master enters an identifier; quote it.
        let count: i64 = c.query_row(
            &format!("SELECT count(*) FROM \"{}\"", name.replace('"', "\"\"")),
            [],
            |r| r.get(0),
        )?;
        if count > 0 {
            item(
                &mut plan.report,
                "legacy_table",
                &name,
                if name.contains("credential") { D::RequiresUserAction } else { D::Skipped },
                if name.contains("credential") {
                    "credential_rebind_required"
                } else {
                    "legacy_state_not_imported"
                },
            );
            if let Some(last) = plan.report.items.last_mut() {
                last.record_count = count as u64;
            }
        }
    }
    plan.report.source_fingerprint = ContentHash::blake3(
        crate::record::encode(&(
            &plan.report.source_fingerprint,
            &plan.workspaces,
            &plan.configurations,
            &plan.bindings,
        ))?
        .as_bytes(),
    );
    plan.verify_sources()?;
    Ok(plan)
}

impl PreparedImport {
    pub fn report(&self) -> &ImportReport {
        &self.report
    }
    pub fn verify_sources(&self) -> StoreResult<()> {
        let _ = immutable(&self.input.database)?;
        for (path, bytes) in &self.source_files {
            if std::fs::read(path)? != *bytes {
                return Err(Error::Conflict);
            }
        }
        for w in &self.workspaces {
            let info =
                aura_windows_platform::inspect_existing_path(Path::new(&w.selected_root), false)?;
            let id = FileIdentity {
                volume_serial_number: info.identity.volume_serial_number.to_string(),
                file_id: info.identity.file_id.iter().map(|b| format!("{b:02x}")).collect(),
            };
            if info.is_reparse_point
                || !info.is_directory
                || w.root_identity.as_ref() != Some(&id)
                || info.final_path.to_string_lossy() != w.canonical_root
            {
                return Err(Error::Conflict);
            }
        }
        if let Some(bytes) = self.configurations.get("memory-sources.json") {
            let reference: serde_json::Value = serde_json::from_slice(bytes)?;
            let root = reference["root"].as_str().ok_or(Error::Invalid("memory root reference"))?;
            let info = aura_windows_platform::inspect_existing_path(Path::new(root), false)?;
            let file_id: String =
                info.identity.file_id.iter().map(|b| format!("{b:02x}")).collect();
            if info.is_reparse_point
                || !info.is_directory
                || reference["volume_serial_number"].as_str().and_then(|v| v.parse::<u64>().ok())
                    != Some(info.identity.volume_serial_number)
                || reference["file_id"] != file_id
            {
                return Err(Error::Conflict);
            }
        }
        Ok(())
    }
}
