//! A purge read off a reader connection: every row it deletes, found before the first delete,
//! so the writer only ever deletes rows it already knows by row ID.
use std::collections::BTreeMap;

use aura_persistence_core::StoreResult;
use rusqlite::{Connection, params};

use super::edges::{self, Edges, quoted};
use super::tables::Spec;
use super::{PurgeReport, scope::json};

/// One table's planned rows: each row's ID and the value that made it part of the purge,
/// compared again when it is deleted.
pub(super) struct Batch {
    pub table: &'static str,
    pub key: &'static str,
    pub extra: &'static str,
    pub shared: bool,
    pub rows: Vec<(i64, String)>,
    pub next: usize,
    /// A full-text table: deleted row by row, by its own row ID.
    pub text_index: bool,
    /// Rows of this table point at other rows of it.
    pub nested: bool,
}

/// Everything one purge deletes, and how far it has got.
pub struct PurgePlan {
    pub(super) batches: Vec<Batch>,
    /// Artifacts the deleted rows held, released once the rows are gone.
    pub(super) held: Vec<String>,
    pub(super) released: bool,
    /// Where collecting unused artifacts has got to in the queue; `None` before it starts.
    pub(super) collected_to: Option<String>,
    pub(super) collected: bool,
    pub(super) edges: Edges,
    pub(super) skipped: usize,
    pub(super) report: PurgeReport,
}

impl PurgePlan {
    /// Every planned row is deleted or kept, and unused artifacts are collected.
    #[must_use]
    pub fn done(&self) -> bool {
        self.batches.iter().all(|batch| batch.next == batch.rows.len()) && self.collected
    }

    /// Rows a step kept because something outside the plan still pointed at them, or that
    /// appeared after the plan was read: planning again finds what is left.
    #[must_use]
    pub fn skipped(&self) -> usize {
        self.skipped
    }

    /// Rows planned, across every table.
    #[must_use]
    pub fn planned(&self) -> usize {
        self.batches.iter().map(|batch| batch.rows.len()).sum()
    }

    #[must_use]
    pub fn report(&self) -> PurgeReport {
        self.report
    }

    pub(super) fn new(db: &Connection, specs: &[Spec<'_>], held: Vec<String>) -> StoreResult<Self> {
        let edges = edges::read(db)?;
        let text_indexes = text_indexes(db)?;
        let mut batches = Vec::with_capacity(specs.len());
        for spec in specs {
            let mut rows = if spec.ids.is_empty() { Vec::new() } else { rows_of(db, spec)? };
            if spec.reversed {
                let order: BTreeMap<&str, usize> =
                    spec.ids.iter().enumerate().map(|(i, id)| (id.as_str(), i)).collect();
                rows.sort_by_key(|(_, key)| std::cmp::Reverse(order.get(key.as_str()).copied()));
            }
            batches.push(Batch {
                table: spec.table,
                key: spec.key,
                extra: spec.extra,
                shared: spec.shared,
                rows,
                next: 0,
                text_index: text_indexes.iter().any(|name| name == spec.table),
                nested: spec.reversed,
            });
        }
        Ok(Self {
            batches,
            held,
            released: false,
            collected_to: None,
            collected: false,
            edges,
            skipped: 0,
            report: PurgeReport::default(),
        })
    }
}

fn rows_of(db: &Connection, spec: &Spec<'_>) -> StoreResult<Vec<(i64, String)>> {
    let mut statement = db.prepare(&format!(
        "SELECT rowid, {key} FROM {table} WHERE {key} IN (SELECT value FROM json_each(?1))",
        key = quoted(spec.key),
        table = quoted(spec.table),
    ))?;
    let rows =
        statement.query_map(params![json(spec.ids)], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn text_indexes(db: &Connection) -> StoreResult<Vec<String>> {
    let mut statement = db.prepare(
        "SELECT name FROM sqlite_master WHERE type='table' AND sql LIKE 'CREATE VIRTUAL TABLE%'",
    )?;
    let names = statement.query_map([], |row| row.get(0))?;
    Ok(names.collect::<Result<_, _>>()?)
}
