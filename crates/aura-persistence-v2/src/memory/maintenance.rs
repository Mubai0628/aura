use super::MemoryIndex;
use aura_memory_domain::{MemoryMaintenanceRun, MemoryMaintenanceState};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, params};

/// The run records in the derived cache. The files beside it stay the durable
/// truth: a rebuild replaces these rows, and a save writes the file first.
impl MemoryIndex {
    pub fn replace_maintenance_runs(&mut self, runs: &[MemoryMaintenanceRun]) -> StoreResult<()> {
        let tx = self.connection.transaction()?;
        tx.execute("DELETE FROM memory_maintenance", [])?;
        for run in runs {
            put(&tx, run)?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn upsert_maintenance_run(&mut self, run: &MemoryMaintenanceRun) -> StoreResult<()> {
        let tx = self.connection.transaction()?;
        tx.execute("DELETE FROM memory_maintenance WHERE run_id=?1", [&run.run_id])?;
        put(&tx, run)?;
        tx.commit()?;
        Ok(())
    }
    pub fn delete_maintenance_runs(&mut self, ids: &[String]) -> StoreResult<()> {
        let tx = self.connection.transaction()?;
        for id in ids {
            tx.execute("DELETE FROM memory_maintenance WHERE run_id=?1", [id])?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Every cached run, newest first. Ordering is by timestamp value, not text.
    pub fn maintenance_runs(&self) -> StoreResult<Vec<MemoryMaintenanceRun>> {
        let mut s = self.connection.prepare("SELECT record_json FROM memory_maintenance")?;
        let mut runs = s
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|raw| Ok(serde_json::from_str::<MemoryMaintenanceRun>(&raw?)?))
            .collect::<StoreResult<Vec<_>>>()?;
        runs.sort_by(|a, b| b.started_at.cmp(&a.started_at).then_with(|| a.run_id.cmp(&b.run_id)));
        Ok(runs)
    }
}

fn put(tx: &Connection, run: &MemoryMaintenanceRun) -> StoreResult<()> {
    let input_tokens = i64::try_from(run.input_tokens)
        .map_err(|_| Error::Invalid("maintenance input usage overflow"))?;
    let output_tokens = i64::try_from(run.output_tokens)
        .map_err(|_| Error::Invalid("maintenance output usage overflow"))?;
    let state = match run.state {
        MemoryMaintenanceState::Queued => "queued",
        MemoryMaintenanceState::Running => "running",
        MemoryMaintenanceState::Completed => "completed",
        MemoryMaintenanceState::Skipped => "skipped",
        MemoryMaintenanceState::Failed => "failed",
        MemoryMaintenanceState::Interrupted => "interrupted",
    };
    tx.execute(
        "INSERT INTO memory_maintenance VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![
            run.run_id,
            run.runtime_instance_id.to_string(),
            run.session_id.to_string(),
            run.turn_id,
            state,
            run.reason,
            run.model_profile_id.map(|id| id.to_string()),
            input_tokens,
            output_tokens,
            run.started_at.to_string(),
            run.finished_at.map(|at| at.to_string()),
            serde_json::to_string(run)?
        ],
    )?;
    Ok(())
}
