//! The rows of a deletion that are found only by reading whole tables: projection outbox rows by
//! key or content, full-text rows by a column the index does not search. They are read on a
//! reader before the deletion writes, so the writer, which every interactive registry write
//! waits for, deletes them by row ID and reads only the rows written after.
use std::time::{Duration, Instant};

use rusqlite::{Connection, Transaction, params};

use crate::{PersistenceCoordinator, PersistenceError, writer_queue::WorkClass};

/// Rows one delete statement takes.
const CHUNK: usize = 512;
/// How long one background job may wait for the writer.
const JOB_WAIT: Duration = Duration::from_secs(120);

/// Rows found by reading whole tables, by row ID.
#[derive(Debug, Default)]
pub(crate) struct Scanned {
    /// `projection_outbox` rows. Outbox IDs only grow, so rows written after the plan are those
    /// above `outbox_newest`.
    pub outbox: Vec<i64>,
    pub outbox_newest: i64,
    /// Full-text rows: each table's row IDs with the value that made them part of the deletion.
    pub text: Vec<TextRows>,
}

#[derive(Debug)]
pub(crate) struct TextRows {
    pub table: &'static str,
    pub key: &'static str,
    pub rows: Vec<(i64, String)>,
}

/// A Session's outbox rows — those keyed by it, the workspace/session projection's history, and
/// the older published rows of any projection that names it (a projection's latest row is what
/// its readers use, so it stays) — and its full-text rows.
pub(crate) fn of_session(db: &Connection, session: &str) -> Result<Scanned, PersistenceError> {
    let snapshot = db.unchecked_transaction()?;
    let outbox = column(
        &snapshot,
        "WITH latest AS (
            SELECT projection_kind AS kind, projection_key AS key, max(outbox_id) AS newest
            FROM projection_outbox GROUP BY projection_kind, projection_key)
         SELECT o.outbox_id FROM projection_outbox o
         JOIN latest l ON l.kind=o.projection_kind AND l.key=o.projection_key
         WHERE instr(o.projection_key, ?1) > 0 OR o.projection_kind='workspace_session'
            OR (o.published_at IS NOT NULL AND instr(o.payload_json, ?1) > 0
                AND o.outbox_id < l.newest)",
        session,
    )?;
    let text = vec![
        text_rows(
            &snapshot,
            "event_search_fts",
            "event_id",
            "SELECT rowid, event_id FROM event_search_fts
             WHERE event_id IN (SELECT event_id FROM events WHERE session_id=?1)",
            session,
        )?,
        text_rows(
            &snapshot,
            "context_search_fts",
            "session_id",
            "SELECT rowid, session_id FROM context_search_fts WHERE session_id=?1",
            session,
        )?,
    ];
    Ok(Scanned { outbox, outbox_newest: newest_outbox(&snapshot)?, text })
}

/// A Workspace's full-text rows and the workspace/session projection's history.
pub(crate) fn of_workspace(db: &Connection, workspace: &str) -> Result<Scanned, PersistenceError> {
    let snapshot = db.unchecked_transaction()?;
    let outbox = column(
        &snapshot,
        "SELECT outbox_id FROM projection_outbox
         WHERE projection_kind='workspace_session' AND ?1 IS NOT NULL",
        workspace,
    )?;
    let text = vec![
        text_rows(
            &snapshot,
            "event_search_fts",
            "event_id",
            "SELECT rowid, event_id FROM event_search_fts
             WHERE event_id IN (
                SELECT event_id FROM events
                WHERE workspace_id=?1
                   OR session_id IN (SELECT session_id FROM sessions WHERE workspace_id=?1))",
            workspace,
        )?,
        text_rows(
            &snapshot,
            "workspace_content_fts",
            "workspace_id",
            "SELECT rowid, workspace_id FROM workspace_content_fts WHERE workspace_id=?1",
            workspace,
        )?,
    ];
    Ok(Scanned { outbox, outbox_newest: newest_outbox(&snapshot)?, text })
}

impl Scanned {
    /// Deletes the full-text rows in short jobs of the writer's index class, so interactive
    /// writes go first between them. They hold no record of their own: an index row left by a
    /// deletion that stops here is only a search hit fewer once its rows are gone.
    pub(crate) fn delete_text(
        &mut self,
        coordinator: &PersistenceCoordinator,
    ) -> Result<(), PersistenceError> {
        for text in std::mem::take(&mut self.text) {
            for chunk in text.rows.chunks(CHUNK) {
                let chunk = chunk.to_vec();
                let (table, key) = (text.table, text.key);
                let deadline = Instant::now() + JOB_WAIT;
                coordinator.dispatch_within(WorkClass::Index, deadline, move |connection| {
                    let tx = connection.transaction()?;
                    {
                        let mut statement = tx
                            .prepare(&format!("DELETE FROM {table} WHERE rowid=?1 AND {key}=?2"))?;
                        for (rowid, value) in &chunk {
                            statement.execute(params![rowid, value])?;
                        }
                    }
                    tx.commit()?;
                    Ok(())
                })?;
            }
        }
        Ok(())
    }

    /// Inside the deletion's transaction: the planned outbox rows by ID, and the rows written
    /// since that `fresh` selects (`?1` is the deleted Session or Workspace).
    pub(crate) fn delete_outbox(
        &self,
        tx: &Transaction<'_>,
        subject: &str,
        fresh: &str,
    ) -> Result<(), PersistenceError> {
        for chunk in self.outbox.chunks(CHUNK) {
            tx.execute(
                "DELETE FROM projection_outbox WHERE outbox_id IN (SELECT value FROM json_each(?1))",
                [serde_json::to_string(chunk)?],
            )?;
        }
        tx.execute(
            &format!("DELETE FROM projection_outbox WHERE outbox_id>?2 AND ({fresh})"),
            params![subject, self.outbox_newest],
        )?;
        Ok(())
    }
}

fn newest_outbox(db: &Connection) -> Result<i64, PersistenceError> {
    Ok(db.query_row("SELECT ifnull(max(outbox_id),0) FROM projection_outbox", [], |row| {
        row.get(0)
    })?)
}

fn column(db: &Connection, sql: &str, subject: &str) -> Result<Vec<i64>, PersistenceError> {
    let mut statement = db.prepare(sql)?;
    let rows = statement.query_map([subject], |row| row.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn text_rows(
    db: &Connection,
    table: &'static str,
    key: &'static str,
    sql: &str,
    subject: &str,
) -> Result<TextRows, PersistenceError> {
    let mut statement = db.prepare(sql)?;
    let rows = statement.query_map([subject], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(TextRows { table, key, rows: rows.collect::<Result<_, _>>()? })
}

/// Deletes an archived Workspace's index entries in short jobs of the writer's index class, so the
/// final deletion does not hold the writer while thousands of them go with the Workspace. The
/// index is rebuilt if the Workspace is restored after all.
pub(crate) fn delete_catalog(
    coordinator: &PersistenceCoordinator,
    workspace: &str,
) -> Result<(), PersistenceError> {
    loop {
        let workspace = workspace.to_owned();
        let deleted = coordinator.dispatch_within(
            WorkClass::Index,
            Instant::now() + JOB_WAIT,
            move |connection| {
                Ok(connection.execute(
                    "DELETE FROM workspace_catalog_entries WHERE rowid IN (
                        SELECT rowid FROM workspace_catalog_entries WHERE workspace_id=?1 LIMIT ?2)",
                    params![workspace, 2_000],
                )?)
            },
        )?;
        if deleted == 0 {
            return Ok(());
        }
    }
}
