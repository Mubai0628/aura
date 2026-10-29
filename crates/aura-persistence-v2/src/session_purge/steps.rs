//! Deleting a plan's rows in short transactions on the writer, so the store's other writers wait
//! for one step at most, not for the whole purge.
use std::time::{Duration, Instant};

use aura_persistence_core::{DatabaseConnection, PersistenceCoreError as Error, StoreResult};
use rusqlite::{Transaction, params};

use super::edges::{Edges, quoted};
use super::plan::{Batch, PurgePlan};
use super::{collect, guards, scope::json};

/// Rows one delete statement takes.
pub(super) const CHUNK: usize = 512;
/// Tables whose delete guard a step lifts while it deletes from them.
const GUARDED: [&str; 2] = ["events", "execution_results"];
/// Children whose rows that point at planned rows are planned themselves (`tables::events`).
const PLANNED_CHILDREN: [&str; 1] = ["projection_outbox"];

/// Deletes the plan's next rows in one transaction that stops once `budget` has passed; a
/// statement that started still finishes. Foreign-key enforcement is off for the transaction and
/// each statement checks the keys itself (`edges`). A step that fails changes nothing, the plan
/// included.
pub fn step(
    db: &mut DatabaseConnection,
    plan: &mut PurgePlan,
    budget: Duration,
) -> StoreResult<()> {
    let saved = Saved::of(plan);
    db.connection().pragma_update(None, "foreign_keys", "OFF")?;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        db.unit_of_work(|tx| run(tx, plan, budget))
    }));
    let restored = db.connection().pragma_update(None, "foreign_keys", "ON");
    let result = match result {
        Ok(result) => result,
        Err(panic) => {
            saved.restore(plan);
            std::panic::resume_unwind(panic)
        }
    };
    if result.is_err() {
        saved.restore(plan);
    }
    restored?;
    let on: i64 = db.connection().query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
    if on != 1 {
        return Err(Error::Invalid("purge left foreign-key enforcement off"));
    }
    result
}

fn run(tx: &Transaction<'_>, plan: &mut PurgePlan, budget: Duration) -> StoreResult<()> {
    let started = Instant::now();
    // Every step moves the plan on, however short its budget.
    let mut progressed = false;
    let mut lifted = None;
    let PurgePlan { batches, edges, skipped, report, .. } = plan;
    'tables: for batch in batches.iter_mut() {
        while batch.next < batch.rows.len() {
            if lifted.is_none() && GUARDED.contains(&batch.table) {
                lifted = Some(guards::lift(tx, guards::PURGE)?);
            }
            let end = (batch.next + CHUNK).min(batch.rows.len());
            let deleted = delete(tx, batch, end, edges)?;
            if !batch.shared {
                *skipped += end - batch.next - deleted;
            }
            report.rows += deleted;
            batch.next = end;
            progressed = true;
            if started.elapsed() >= budget {
                break 'tables;
            }
        }
    }
    if let Some(lifted) = lifted {
        guards::restore(tx, lifted)?;
    }
    if plan.batches.iter().any(|batch| batch.next < batch.rows.len()) {
        return Ok(());
    }
    if !plan.released {
        collect::release(tx, &plan.held)?;
        plan.released = true;
    }
    while !plan.collected && (!progressed || started.elapsed() < budget) {
        collect::next(tx, plan)?;
        progressed = true;
    }
    Ok(())
}

/// Deletes the batch's rows up to `end` that nothing still points at; returns how many.
fn delete(tx: &Transaction<'_>, batch: &Batch, end: usize, edges: &Edges) -> StoreResult<usize> {
    let chunk = &batch.rows[batch.next..end];
    let (table, key) = (quoted(batch.table), quoted(batch.key));
    if batch.text_index {
        // A full-text table has no keys pointing at it and finds rows only by its own row ID.
        let mut statement =
            tx.prepare(&format!("DELETE FROM {table} WHERE rowid=?1 AND {key}=?2"))?;
        let mut deleted = 0;
        for (rowid, value) in chunk {
            deleted += statement.execute(params![rowid, value])?;
        }
        return Ok(deleted);
    }
    let rowids = serde_json::to_string(&chunk.iter().map(|(rowid, _)| *rowid).collect::<Vec<_>>())
        .map_err(|_| Error::Invalid("purge row IDs"))?;
    let keys = json(&chunk.iter().map(|(_, value)| value.clone()).collect::<Vec<_>>());
    // `+` keeps the key out of the lookup: rows are found by row ID alone, then their key is
    // compared, instead of probing every row ID with every key.
    let mut statement = tx.prepare(&format!(
        "DELETE FROM {table} WHERE rowid IN (SELECT value FROM json_each(?1))
         AND +{key} IN (SELECT value FROM json_each(?2)) {extra}{guards}",
        extra = batch.extra,
        guards = edges.guards(batch.table, &[], &PLANNED_CHILDREN),
    ))?;
    let mut deleted = statement.execute(params![rowids, keys])?;
    // Rows that point at each other: each pass deletes those nothing points at any more.
    while batch.nested && deleted < chunk.len() {
        let more = statement.execute(params![rowids, keys])?;
        if more == 0 {
            break;
        }
        deleted += more;
    }
    Ok(deleted)
}

/// What a step changes in the plan, put back when its transaction rolls back.
struct Saved {
    next: Vec<usize>,
    released: bool,
    collected_to: Option<String>,
    collected: bool,
    skipped: usize,
    report: super::PurgeReport,
}

impl Saved {
    fn of(plan: &PurgePlan) -> Self {
        Self {
            next: plan.batches.iter().map(|batch| batch.next).collect(),
            released: plan.released,
            collected_to: plan.collected_to.clone(),
            collected: plan.collected,
            skipped: plan.skipped,
            report: plan.report,
        }
    }

    fn restore(self, plan: &mut PurgePlan) {
        for (batch, next) in plan.batches.iter_mut().zip(self.next) {
            batch.next = next;
        }
        plan.released = self.released;
        plan.collected_to = self.collected_to;
        plan.collected = self.collected;
        plan.skipped = self.skipped;
        plan.report = self.report;
    }
}
