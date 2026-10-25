//! Run metadata survives a disposable SQLite cache, but pending work is never replayed.
use crate::*;
use aura_domain_common::{
    ContentHash, ContentHashAlgorithm, RuntimeInstanceId, UtcTimestamp, WorkspacePath,
};
use aura_fs_tools::vault::VaultChange;
use std::path::Path;

/// Runs kept on disk per project Vault; the settings view lists the newest 100.
const KEPT_RUNS: usize = 200;
/// Pruning starts above this, so it runs once per fifty new runs, not every time.
const PRUNE_ABOVE: usize = 250;

impl crate::audit::AuditLog {
    fn run(&self, id: &str) -> MemoryResult<Option<MemoryMaintenanceRun>> {
        ContentHash::new(ContentHashAlgorithm::Blake3, id)
            .map_err(|_| MemoryError::Invalid("run identity"))?;
        let path = WorkspacePath::parse(format!("maintenance/{id}.json"))
            .map_err(|_| MemoryError::Invalid("run path"))?;
        match self.fs.read(&path) {
            Ok(read) => {
                let run: MemoryMaintenanceRun = serde_json::from_slice(&read.bytes)
                    .map_err(|_| MemoryError::Invalid("run metadata"))?;
                if run.run_id != id {
                    return Err(MemoryError::Invalid("run metadata identity"));
                }
                Ok(Some(run))
            }
            Err(aura_fs_tools::vault::VaultFileError::Io(e))
                if e.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }
    pub fn save_run(&self, run: &MemoryMaintenanceRun) -> MemoryResult<()> {
        if run.input_tokens > i64::MAX as u64 || run.output_tokens > i64::MAX as u64 {
            return Err(MemoryError::Invalid("maintenance usage overflow"));
        }
        ContentHash::new(ContentHashAlgorithm::Blake3, run.run_id.clone())
            .map_err(|_| MemoryError::Invalid("maintenance run identity"))?;
        if run.reason.as_ref().is_some_and(|r| r.len() > 2000) {
            return Err(MemoryError::Invalid("maintenance reason bound"));
        }
        let path = WorkspacePath::parse(format!("maintenance/{}.json", run.run_id))
            .map_err(|_| MemoryError::Invalid("run path"))?;
        let expected_hash = match self.fs.read(&path) {
            Ok(r) => Some(r.hash),
            Err(aura_fs_tools::vault::VaultFileError::Io(e))
                if e.kind() == std::io::ErrorKind::NotFound =>
            {
                None
            }
            Err(error) => return Err(error.into()),
        };
        self.fs.apply(&[VaultChange {
            path,
            expected_hash,
            bytes: Some(serde_json::to_vec(run).map_err(|_| MemoryError::Invalid("run metadata"))?),
        }])?;
        Ok(())
    }
    fn delete_run(&self, id: &str) -> MemoryResult<()> {
        let path = WorkspacePath::parse(format!("maintenance/{id}.json"))
            .map_err(|_| MemoryError::Invalid("run path"))?;
        let read = self.fs.read(&path)?;
        self.fs.apply(&[VaultChange { path, expected_hash: Some(read.hash), bytes: None }])?;
        Ok(())
    }
    /// Every run file; only a rebuild reads them all.
    pub fn runs(&self) -> MemoryResult<Vec<MemoryMaintenanceRun>> {
        let root = self.fs.root().join("maintenance");
        if !root.try_exists()? {
            return Ok(vec![]);
        }
        let mut runs = vec![];
        for item in std::fs::read_dir(root)? {
            let item = item?;
            if item.path().extension().is_none_or(|e| e != "json") {
                continue;
            }
            if runs.len() >= 10000 {
                return Err(MemoryError::Invalid("maintenance history bound"));
            }
            let path =
                WorkspacePath::parse(format!("maintenance/{}", item.file_name().to_string_lossy()))
                    .map_err(|_| MemoryError::Invalid("run path"))?;
            let run: MemoryMaintenanceRun = serde_json::from_slice(&self.fs.read(&path)?.bytes)
                .map_err(|_| MemoryError::Invalid("run metadata"))?;
            if Path::new(path.as_str())
                .file_stem()
                .is_none_or(|stem| stem.to_string_lossy() != run.run_id)
            {
                return Err(MemoryError::Invalid("run metadata identity"));
            }
            runs.push(run);
        }
        runs.sort_by_key(|run| std::cmp::Reverse(run.started_at));
        Ok(runs)
    }
}
impl Vault {
    pub fn maintenance_run(&self, id: &str) -> MemoryResult<Option<MemoryMaintenanceRun>> {
        self.lock().audit.run(id)
    }
    pub fn record_maintenance_run(&self, run: &MemoryMaintenanceRun) -> MemoryResult<()> {
        let mut state = self.lock();
        let old = state.audit.run(&run.run_id)?;
        if let Some(old) = &old {
            if serde_json::to_value(old).map_err(|_| MemoryError::Invalid("run metadata"))?
                == serde_json::to_value(run).map_err(|_| MemoryError::Invalid("run metadata"))?
            {
                return Ok(());
            }
            if old.runtime_instance_id != run.runtime_instance_id
                || old.session_id != run.session_id
                || old.turn_id != run.turn_id
                || !matches!(
                    old.state,
                    MemoryMaintenanceState::Queued | MemoryMaintenanceState::Running
                )
                || (old.state == MemoryMaintenanceState::Running
                    && run.state == MemoryMaintenanceState::Queued)
            {
                return Err(MemoryError::Invalid("maintenance run cannot regress or replay"));
            }
        }
        state.audit.save_run(run)?;
        state.index.upsert_maintenance_run(run)?;
        if crate::maintenance_log::finished(run.state)
            && old.as_ref().is_none_or(|old| !crate::maintenance_log::finished(old.state))
        {
            // The run file above is the record; the log repeats it for reading.
            let _ = crate::maintenance_log::append(state.audit.fs.root(), run);
        }
        if old.is_none() {
            // Bounded like the settings list; a failed prune is retried by the next run.
            let _ = prune(&mut state);
        }
        Ok(())
    }
    /// The newest runs. Unfinished runs of an earlier process are settled as
    /// interrupted here and never resumed.
    pub fn maintenance_runs(
        &self,
        runtime: RuntimeInstanceId,
    ) -> MemoryResult<Vec<MemoryMaintenanceRun>> {
        let mut state = self.lock();
        let mut runs = state.index.maintenance_runs()?;
        for run in &mut runs {
            if run.runtime_instance_id != runtime
                && matches!(
                    run.state,
                    MemoryMaintenanceState::Queued | MemoryMaintenanceState::Running
                )
            {
                run.state = MemoryMaintenanceState::Interrupted;
                run.reason = Some("已中断；重启后不重放".into());
                run.finished_at = Some(UtcTimestamp::now());
                state.audit.save_run(run)?;
                state.index.upsert_maintenance_run(run)?;
                let _ = crate::maintenance_log::append(state.audit.fs.root(), run);
            }
        }
        runs.truncate(100);
        Ok(runs)
    }
}

/// Above the bound, remove the oldest finished runs down to `KEPT_RUNS`. Queued
/// or running runs are never removed here.
fn prune(state: &mut crate::vault::VaultState) -> MemoryResult<()> {
    let runs = state.index.maintenance_runs()?;
    if runs.len() <= PRUNE_ABOVE {
        return Ok(());
    }
    let mut removed = vec![];
    for run in runs.iter().skip(KEPT_RUNS) {
        if matches!(run.state, MemoryMaintenanceState::Queued | MemoryMaintenanceState::Running) {
            continue;
        }
        state.audit.delete_run(&run.run_id)?;
        removed.push(run.run_id.clone());
    }
    state.index.delete_maintenance_runs(&removed)?;
    Ok(())
}
