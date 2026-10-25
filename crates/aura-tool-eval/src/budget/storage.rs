use super::{Limits, types::RequestRecord};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension};

pub(super) const COST: &str =
    "CASE WHEN state='settled' THEN input+output ELSE reserve_input+reserve_output END";

pub(super) fn initialize(db: &Connection, limits: Limits) -> Result<()> {
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
        CREATE TABLE IF NOT EXISTS policy(singleton INTEGER PRIMARY KEY CHECK(singleton=1),limits TEXT NOT NULL,halted INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS requests(id INTEGER PRIMARY KEY,task TEXT NOT NULL,kind TEXT NOT NULL,reserve_input INTEGER NOT NULL,reserve_output INTEGER NOT NULL,state TEXT NOT NULL,input INTEGER,output INTEGER,cached_input INTEGER,tool_definition_bytes INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE IF NOT EXISTS task_limits(task TEXT PRIMARY KEY,cap INTEGER NOT NULL,input_cap INTEGER NOT NULL,output_cap INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS stop_reason(singleton INTEGER PRIMARY KEY CHECK(singleton=1),reason TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS task_stops(task TEXT PRIMARY KEY,reason TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS provider_status(id INTEGER PRIMARY KEY,status INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS usage_details(id INTEGER PRIMARY KEY,reasoning_tokens INTEGER,cache_write_tokens INTEGER);
        CREATE TABLE IF NOT EXISTS request_sizes(id INTEGER PRIMARY KEY,body_bytes INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS reconciliations(id INTEGER PRIMARY KEY,explanation TEXT NOT NULL,previous_reason TEXT NOT NULL,charged_tokens INTEGER NOT NULL,created_at TEXT NOT NULL);")?;
    db.execute("INSERT OR IGNORE INTO policy VALUES(1,?1,0)", [serde_json::to_string(&limits)?])?;
    let stored: String = db.query_row("SELECT limits FROM policy", [], |r| r.get(0))?;
    let stored: Limits = serde_json::from_str(&stored)?;
    ensure!(stored.cumulative == limits.cumulative, "eval.cumulative_policy_changed");
    db.execute("UPDATE policy SET halted=1 WHERE EXISTS(SELECT 1 FROM requests WHERE state IN ('pending','unknown'))", [])?;
    db.execute("INSERT OR IGNORE INTO stop_reason SELECT 1,'usage_unknown' WHERE EXISTS(SELECT 1 FROM requests WHERE state IN ('pending','unknown'))", [])?;
    Ok(())
}

pub(super) fn total(db: &Connection, task: Option<&str>) -> Result<u64> {
    Ok(db.query_row(
        &format!("SELECT COALESCE(SUM({COST}),0) FROM requests WHERE ?1 IS NULL OR task=?1"),
        [task],
        |r| number(r, 0),
    )?)
}

pub(super) fn halt(db: &Connection, reason: &str) -> Result<()> {
    db.execute("UPDATE policy SET halted=1", [])?;
    db.execute("INSERT OR IGNORE INTO stop_reason VALUES(1,?1)", [reason])?;
    Ok(())
}

pub(super) fn task_reason(db: &Connection, task: &str) -> Result<Option<String>> {
    Ok(db
        .query_row("SELECT reason FROM task_stops WHERE task=?1", [task], |r| r.get(0))
        .optional()?)
}

pub(super) fn records(db: &Connection) -> Result<Vec<RequestRecord>> {
    let mut query = db.prepare("SELECT r.id,task,kind,reserve_input+reserve_output,input,output,cached_input,state,tool_definition_bytes,p.status,u.reasoning_tokens,u.cache_write_tokens,COALESCE(s.body_bytes,0) FROM requests r LEFT JOIN provider_status p ON r.id=p.id LEFT JOIN usage_details u ON r.id=u.id LEFT JOIN request_sizes s ON r.id=s.id ORDER BY r.id")?;
    Ok(query
        .query_map([], |r| {
            Ok(RequestRecord {
                id: r.get(0)?,
                task: r.get(1)?,
                kind: r.get(2)?,
                reserved: number(r, 3)?,
                input: optional_number(r, 4)?,
                output: optional_number(r, 5)?,
                cached_input: optional_number(r, 6)?,
                state: r.get(7)?,
                tool_definition_bytes: number(r, 8)?,
                provider_status: r.get(9)?,
                reasoning_tokens: optional_number(r, 10)?,
                cache_write_tokens: optional_number(r, 11)?,
                request_body_bytes: number(r, 12)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

pub(super) fn sql(value: u64) -> Result<i64> {
    Ok(i64::try_from(value)?)
}
pub(super) fn number(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(e),
        )
    })
}
pub(super) fn optional_number(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<Option<u64>> {
    let value: Option<i64> = row.get(index)?;
    value
        .map(|v| {
            u64::try_from(v).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    index,
                    rusqlite::types::Type::Integer,
                    Box::new(e),
                )
            })
        })
        .transpose()
}
