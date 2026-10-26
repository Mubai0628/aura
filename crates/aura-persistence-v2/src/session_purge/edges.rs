//! The foreign keys that point at each table, read from the schema.
//!
//! A purge deletes in short transactions with SQLite's own foreign-key enforcement off, because
//! enforcing it means reading a whole child table for every deleted row wherever the pointing
//! columns have no index. Each delete statement carries these keys as conditions instead: a row is
//! deleted only while nothing points at it, so the store never holds a dangling reference, and a
//! key added to the schema later is checked without anyone listing it here.
use std::collections::{BTreeMap, BTreeSet};

use aura_persistence_core::StoreResult;
use rusqlite::Connection;

pub(super) struct Edge {
    pub child: String,
    pub columns: Vec<String>,
    pub parent_columns: Vec<String>,
    /// The child's rows go with the parent (`ON DELETE CASCADE`). With enforcement off they
    /// do not: a purge keeps such a parent like any other, or deletes the children itself.
    pub cascade: bool,
    /// An index of the child starts with the pointing columns.
    pub indexed: bool,
    /// The child's largest row ID when the plan was read, for a child whose IDs only grow
    /// (`AUTOINCREMENT`).
    pub newest: Option<i64>,
}

/// Every foreign key, by the table it points at.
pub(super) struct Edges(BTreeMap<String, Vec<Edge>>);

/// One foreign key as SQLite lists it, a row per column.
struct Key {
    id: i64,
    parent: String,
    columns: Vec<String>,
    to: Vec<Option<String>>,
    on_delete: String,
}

pub(super) fn read(db: &Connection) -> StoreResult<Edges> {
    let mut edges: BTreeMap<String, Vec<Edge>> = BTreeMap::new();
    for (child, sql) in tables(db)? {
        let growing = sql.to_ascii_uppercase().contains("AUTOINCREMENT");
        let newest = if growing {
            Some(db.query_row(
                &format!("SELECT ifnull(max(rowid),0) FROM {}", quoted(&child)),
                [],
                |r| r.get(0),
            )?)
        } else {
            None
        };
        let leading = leading_columns(db, &child)?;
        let mut statement = db.prepare(
            "SELECT id, \"table\", \"from\", \"to\", on_delete FROM pragma_foreign_key_list(?1)
             ORDER BY id, seq",
        )?;
        let mut keys: Vec<Key> = Vec::new();
        for row in statement.query_map([&child], |row| {
            Ok(Key {
                id: row.get(0)?,
                parent: row.get(1)?,
                columns: vec![row.get(2)?],
                to: vec![row.get(3)?],
                on_delete: row.get(4)?,
            })
        })? {
            let key = row?;
            match keys.last_mut() {
                Some(last) if last.id == key.id => {
                    last.columns.extend(key.columns);
                    last.to.extend(key.to);
                }
                _ => keys.push(key),
            }
        }
        for Key { parent, columns, to, on_delete, .. } in keys {
            let parent_columns = if to.iter().all(Option::is_some) {
                to.into_iter().flatten().collect()
            } else {
                primary_key(db, &parent)?
            };
            let wanted: BTreeSet<&String> = columns.iter().collect();
            let indexed = leading.iter().any(|lead| {
                lead.len() >= columns.len()
                    && lead[..columns.len()].iter().collect::<BTreeSet<_>>() == wanted
            });
            edges.entry(parent).or_default().push(Edge {
                child: child.clone(),
                columns,
                parent_columns,
                cascade: on_delete.eq_ignore_ascii_case("CASCADE"),
                indexed,
                newest,
            });
        }
    }
    Ok(Edges(edges))
}

impl Edges {
    /// Conditions, appended to a `DELETE FROM parent WHERE …`, that keep every row something
    /// still points at. Rows of the children named in `going` are deleted with the parent by
    /// the caller instead. `planned` names children whose rows pointing at the planned rows the
    /// plan deletes itself: only rows written after the plan was read are looked at for them.
    pub fn guards(&self, parent: &str, going: &[&str], planned: &[&str]) -> String {
        let mut sql = String::new();
        for edge in self.0.get(parent).into_iter().flatten() {
            if going.contains(&edge.child.as_str()) {
                continue;
            }
            let child = quoted(&edge.child);
            if edge.indexed {
                let matched = edge
                    .columns
                    .iter()
                    .zip(&edge.parent_columns)
                    .map(|(c, p)| format!("c.{}={}.{}", quoted(c), quoted(parent), quoted(p)))
                    .collect::<Vec<_>>()
                    .join(" AND ");
                sql.push_str(&format!(" AND NOT EXISTS(SELECT 1 FROM {child} c WHERE {matched})"));
                continue;
            }
            let present = edge
                .columns
                .iter()
                .map(|c| format!("{} IS NOT NULL", quoted(c)))
                .collect::<Vec<_>>()
                .join(" AND ");
            let newer = match edge.newest {
                Some(newest) if planned.contains(&edge.child.as_str()) => {
                    format!(" AND rowid>{newest}")
                }
                _ => String::new(),
            };
            sql.push_str(&format!(
                " AND {} NOT IN (SELECT {} FROM {child} WHERE {present}{newer})",
                row_value(parent, &edge.parent_columns),
                list(&edge.columns),
            ));
        }
        sql
    }

    /// The key from `child` to `parent`, when there is exactly one.
    pub fn edge(&self, parent: &str, child: &str) -> Option<&Edge> {
        let mut found = self.0.get(parent).into_iter().flatten().filter(|e| e.child == child);
        let edge = found.next()?;
        found.next().is_none().then_some(edge)
    }
}

fn tables(db: &Connection) -> StoreResult<Vec<(String, String)>> {
    let mut statement = db.prepare(
        "SELECT name, sql FROM sqlite_master
         WHERE type='table' AND substr(name,1,7)<>'sqlite_' AND sql NOT LIKE 'CREATE VIRTUAL%'
         ORDER BY name",
    )?;
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The column lists a lookup can start with: each full index without expressions, and the
/// primary key.
fn leading_columns(db: &Connection, table: &str) -> StoreResult<Vec<Vec<String>>> {
    let mut lists = Vec::new();
    let mut indexes = db.prepare("SELECT name FROM pragma_index_list(?1) WHERE partial=0")?;
    let names = indexes.query_map([table], |row| row.get::<_, String>(0))?;
    for name in names {
        let mut statement = db.prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")?;
        let columns = statement
            .query_map([name?], |row| row.get::<_, Option<String>>(0))?
            .collect::<Result<Option<Vec<String>>, _>>()?;
        if let Some(columns) = columns {
            lists.push(columns);
        }
    }
    lists.push(primary_key(db, table)?);
    Ok(lists)
}

fn primary_key(db: &Connection, table: &str) -> StoreResult<Vec<String>> {
    let mut statement =
        db.prepare("SELECT name FROM pragma_table_info(?1) WHERE pk>0 ORDER BY pk")?;
    let columns = statement.query_map([table], |row| row.get(0))?;
    Ok(columns.collect::<Result<_, _>>()?)
}

fn row_value(table: &str, columns: &[String]) -> String {
    let qualified =
        columns.iter().map(|c| format!("{}.{}", quoted(table), quoted(c))).collect::<Vec<_>>();
    if qualified.len() == 1 { qualified[0].clone() } else { format!("({})", qualified.join(",")) }
}

fn list(columns: &[String]) -> String {
    columns.iter().map(|c| quoted(c)).collect::<Vec<_>>().join(",")
}

pub(super) fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
