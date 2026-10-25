//! Lookup indexes a purge needs and the store does not keep.
//!
//! Before deleting a row SQLite looks for rows that still point at it. Where the pointing columns
//! have no index, that is a read of the whole table for every deleted row, and a purge deletes
//! tens of thousands of rows: the outbox points at events without an index, and a real
//! test (55 000 events beside 300 000 outbox rows) would have run for about 40 minutes with the
//! store locked. Only a purge deletes from these tables in bulk, so the store keeps no such
//! indexes. A purge builds one for every foreign key no index covers, inside its own transaction,
//! and drops them before it commits: the schema the store verifies on open is unchanged, and if
//! the purge fails the transaction rolls the indexes back with everything else.
use std::collections::BTreeSet;

use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, Transaction};

const PREFIX: &str = "aura_purge_lookup_";

pub(super) struct Built(Vec<String>);

pub(super) fn build(tx: &Transaction<'_>) -> StoreResult<Built> {
    let mut built = Vec::new();
    for (table, columns) in uncovered(tx)? {
        let name = format!("{PREFIX}{}", built.len());
        let columns = columns.iter().map(|column| quoted(column)).collect::<Vec<_>>().join(",");
        tx.execute_batch(&format!("CREATE INDEX {name} ON {}({columns})", quoted(&table)))?;
        built.push(name);
    }
    Ok(Built(built))
}

pub(super) fn remove(tx: &Transaction<'_>, built: Built) -> StoreResult<()> {
    for name in built.0 {
        tx.execute_batch(&format!("DROP INDEX {name}"))?;
    }
    let left: i64 = tx.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='index' AND substr(name, 1, length(?1))=?1",
        [PREFIX],
        |row| row.get(0),
    )?;
    if left != 0 {
        return Err(Error::Invalid("purge lookup index was not dropped"));
    }
    Ok(())
}

/// Every foreign key, as its table and columns, that no index of its table starts with.
fn uncovered(db: &Connection) -> StoreResult<Vec<(String, Vec<String>)>> {
    let mut found = Vec::new();
    for table in strings(
        db,
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'
         ORDER BY name",
        [],
    )? {
        let covers = leading_columns(db, &table)?;
        let mut statement =
            db.prepare("SELECT id, \"from\" FROM pragma_foreign_key_list(?1) ORDER BY id, seq")?;
        let mut keys: Vec<(i64, Vec<String>)> = Vec::new();
        for row in statement.query_map([&table], |row| Ok((row.get(0)?, row.get(1)?)))? {
            let (id, column): (i64, String) = row?;
            match keys.last_mut() {
                Some((last, columns)) if *last == id => columns.push(column),
                _ => keys.push((id, vec![column])),
            }
        }
        for (_, columns) in keys {
            let wanted: BTreeSet<&String> = columns.iter().collect();
            let covered = covers.iter().any(|leading| {
                leading.len() >= columns.len()
                    && leading[..columns.len()].iter().collect::<BTreeSet<_>>() == wanted
            });
            if !covered && !found.iter().any(|(t, c)| t == &table && c == &columns) {
                found.push((table.clone(), columns));
            }
        }
    }
    Ok(found)
}

/// The column lists a lookup can start with: each full index (not partial, no expressions) and
/// the primary key.
fn leading_columns(db: &Connection, table: &str) -> StoreResult<Vec<Vec<String>>> {
    let mut lists = Vec::new();
    for index in strings(db, "SELECT name FROM pragma_index_list(?1) WHERE partial=0", [table])? {
        let mut statement = db.prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")?;
        let columns = statement
            .query_map([&index], |row| row.get::<_, Option<String>>(0))?
            .collect::<Result<Option<Vec<String>>, _>>()?;
        if let Some(columns) = columns {
            lists.push(columns);
        }
    }
    lists.push(strings(
        db,
        "SELECT name FROM pragma_table_info(?1) WHERE pk>0 ORDER BY pk",
        [table],
    )?);
    Ok(lists)
}

fn strings<P: rusqlite::Params>(db: &Connection, sql: &str, params: P) -> StoreResult<Vec<String>> {
    let mut statement = db.prepare(sql)?;
    let values = statement.query_map(params, |row| row.get(0))?;
    Ok(values.collect::<Result<_, _>>()?)
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
