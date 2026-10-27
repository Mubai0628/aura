//! Strict append-only schema upgrades; identity and structure checked before writing.
use crate::MODULES;
use aura_persistence_core::{
    PersistenceCoreError as Error, StoreResult,
    schema::{self, SchemaModule},
};
use std::path::Path;

/// Moves all supported predecessor modules forward in one atomic transaction.
///
/// The database is identified read-only first: application id, user version, every
/// other module digest, and an explicit predecessor of the module that changed. An
/// unknown digest or a database that is not this application is refused before any
/// write. A supported predecessor applies its missing statements and the new digest
/// in one transaction, so a failure leaves the previous digest and tables.
///
/// A predecessor is never inferred from the schema version and never migrated "as
/// much as fits". Only the exact frozen digests listed below are accepted, and each
/// one is verified against the live `sqlite_master` text before a writer is opened,
/// so a database that merely resembles a predecessor fails closed instead of
/// acquiring tables nobody reviewed.
pub(crate) fn migrate(path: &Path) -> StoreResult<()> {
    let planned = plan_migration(path)?;
    let Some(planned) = planned else {
        return Ok(());
    };
    let mut connection = rusqlite::Connection::open(path)?;
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    apply_plan(&transaction, &planned)?;
    transaction.commit()?;
    Ok(())
}

/// Whether this build would migrate the store at `path`, decided read-only exactly as [`migrate`]
/// decides it. A missing store is a new one, which nothing needs to protect.
pub fn migration_pending(path: &Path) -> StoreResult<bool> {
    if !path.is_file() {
        return Ok(false);
    }
    Ok(plan_migration(path)?.is_some())
}

fn apply_plan(
    transaction: &rusqlite::Transaction<'_>,
    planned: &PlannedMigrations,
) -> StoreResult<()> {
    // Every module the plan moves is re-read here, in the writing transaction, and
    // the whole database is compared against the structure those predecessors
    // together create. A concurrent write, a different predecessor or a structure
    // that merely looks similar all fail before a statement runs.
    for migration in &planned.migrations {
        let live_digest: String = transaction.query_row(
            "SELECT digest FROM v2_schema_metadata WHERE owner=?1 AND version=1",
            [migration.owner],
            |row| row.get(0),
        )?;
        if live_digest != migration.digest {
            return Err(Error::Invalid("unsupported schema digest"));
        }
    }
    if live_structure(transaction)? != expected_combined_structure(&planned.migrations)? {
        return Err(Error::Invalid("unsupported schema structure"));
    }
    for migration in &planned.migrations {
        transaction.execute_batch(migration.statements)?;
        transaction.execute(
            "UPDATE v2_schema_metadata SET digest=?1 WHERE owner=?2 AND version=1",
            rusqlite::params![
                blake3::hash(migration.current_sql.as_bytes()).to_hex().to_string(),
                migration.owner,
            ],
        )?;
    }
    // One transaction for every module. A failure in the second leaves the first
    // one's tables and digest exactly as they were, so a database is never left
    // half-migrated across two modules.
    Ok(())
}

/// One module's move, from a frozen predecessor to the current schema.
struct PlannedMigration {
    owner: &'static str,
    statements: &'static str,
    predecessor_sql: &'static str,
    current_sql: &'static str,
    digest: String,
}

/// The set of moves a database needs, verified against its live structure.
struct PlannedMigrations {
    migrations: Vec<PlannedMigration>,
}

/// Reads a database read-only and decides what it needs.
///
/// Identity, the module inventory and — for every module that is out of date — an
/// exactly supported predecessor, all before a writer is opened. `None` means the
/// database is already current.
fn plan_migration(path: &Path) -> StoreResult<Option<PlannedMigrations>> {
    let reader = aura_persistence_core::open_reader(path)?;
    let application_id: i32 = reader.query_row("PRAGMA application_id", [], |row| row.get(0))?;
    let user_version: i32 = reader.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if application_id != schema::APPLICATION_ID || user_version != 1 {
        return Err(Error::Invalid("unsupported database identity"));
    }
    let mut statement =
        reader.prepare("SELECT owner, digest FROM v2_schema_metadata WHERE version=1")?;
    let rows =
        statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    let mut digests = std::collections::BTreeMap::new();
    for row in rows {
        let (owner, digest) = row?;
        if digests.insert(owner, digest).is_some() {
            return Err(Error::Invalid("schema owner inventory"));
        }
    }
    drop(statement);
    let expected = expected_module_digests();
    if digests.len() != expected.len() {
        return Err(Error::Invalid("schema owner inventory"));
    }
    // Every module that changed since this database was created is one this build
    // has to move, and every module that did not must already match exactly. Two
    // outdated modules are as supported as one: a database created before both the
    // agent index and the consent table existed is a database this build has a
    // reviewed path from.
    let outdated = digests
        .iter()
        .filter(|(owner, digest)| expected.get(owner.as_str()).is_none_or(|known| known != *digest))
        .map(|(owner, digest)| (owner.clone(), digest.clone()))
        .collect::<Vec<_>>();
    if outdated.is_empty() {
        return Ok(None);
    }
    let mut migrations = Vec::with_capacity(outdated.len());
    for (owner, digest) in &outdated {
        migrations.push(planned_predecessor(owner, digest)?);
    }
    // The whole set is checked against the live structure before anything is
    // written, exactly as a single-module migration was.
    if live_structure(&reader)? != expected_combined_structure(&migrations)?
        || !database_is_intact(&reader)?
    {
        return Err(Error::Invalid("unsupported schema structure"));
    }
    Ok(Some(PlannedMigrations { migrations }))
}

fn expected_module_digests() -> std::collections::BTreeMap<&'static str, String> {
    std::iter::once(SchemaModule { owner: "core", sql: schema::CORE_SCHEMA })
        .chain(MODULES.iter().copied())
        .map(|module| (module.owner, blake3::hash(module.sql.as_bytes()).to_hex().to_string()))
        .collect()
}

/// The SQL that takes one frozen module schema to the current module.
///
/// The digest selects the predecessor: only the exact frozen SQL listed below is
/// accepted, so a database that merely resembles a predecessor fails closed instead
/// of acquiring tables nobody reviewed.
fn planned_predecessor(owner: &str, digest: &str) -> StoreResult<PlannedMigration> {
    let (predecessors, current_sql) = match owner {
        "agent" => (AGENT_PREDECESSORS, include_str!("agent.sql")),
        "security" => (SECURITY_PREDECESSORS, include_str!("security.sql")),
        "tool" => (TOOL_PREDECESSORS, include_str!("tool.sql")),
        _ => return Err(Error::Invalid("unsupported schema digest")),
    };
    let owner: &'static str = MODULES
        .iter()
        .find(|module| module.owner == owner)
        .map(|module| module.owner)
        .ok_or(Error::Invalid("unsupported schema digest"))?;
    for marker in predecessors {
        let (predecessor, statements) = split_module_schema(current_sql, marker)?;
        if blake3::hash(predecessor.as_bytes()).to_hex().as_str() != digest {
            continue;
        }
        return Ok(PlannedMigration {
            owner,
            statements,
            predecessor_sql: Box::leak(predecessor.into_boxed_str()),
            current_sql,
            digest: digest.to_owned(),
        });
    }
    Err(Error::Invalid("unsupported schema digest"))
}

/// Newest supported predecessor first. Each marker is the first statement the
/// predecessor does not have yet; the newest lacks only the session and turn indexes.
const AGENT_PREDECESSORS: &[&str] = &[
    "\nCREATE INDEX turn_tasks_session",
    "\nCREATE TABLE agent_visible_repair",
    "\nCREATE TABLE agent_visible_delta",
    "\nCREATE TABLE agent_visible_index",
];

/// Supported `security.sql` predecessors, newest first.
///
/// The only security change so far is the reusable Native session consent table.
/// A database created before it exists has no consent records, and the migration
/// adds the table only: it never rewrites a stored Grant, so a historical
/// `allow_session` row keeps its exact-operation meaning.
const SECURITY_PREDECESSORS: &[&str] = &["\nCREATE TABLE native_session_consents"];
/// Supported `tool.sql` predecessors, newest first, each named by what it lacks: no MCP secrets or
/// OAuth sessions, no MCP catalogs, no Hook records, package content without installed
/// settings or credential bindings, no package tables, and the schema before Agent
/// terminals.
const TOOL_PREDECESSORS: &[&str] = &[
    "\nCREATE TABLE mcp_secrets",
    "\nCREATE TABLE mcp_catalogs",
    "\nCREATE TABLE hook_invocations",
    "\nCREATE TABLE tool_package_configurations",
    "\nCREATE TABLE tool_package_payloads",
    "\nCREATE TABLE agent_terminals",
];

fn live_structure(connection: &rusqlite::Connection) -> StoreResult<Vec<(String, String)>> {
    let mut query = connection.prepare(
        "SELECT name, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' AND sql IS NOT NULL ORDER BY name",
    )?;
    Ok(query.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<Result<Vec<_>, _>>()?)
}

/// The structure the planned predecessors together create, with every other module
/// at its current revision.
///
/// Each planned module is substituted for *itself*. Substituting one module's
/// predecessor for another's would make `live_structure` and this function agree
/// while both described something nobody reviewed — which is the check silently
/// passing instead of testing anything.
fn expected_combined_structure(
    migrations: &[PlannedMigration],
) -> StoreResult<Vec<(String, String)>> {
    let connection = rusqlite::Connection::open_in_memory()?;
    connection.execute_batch(schema::CORE_SCHEMA)?;
    for module in MODULES {
        let sql = migrations
            .iter()
            .find(|migration| migration.owner == module.owner)
            .map_or(module.sql, |migration| migration.predecessor_sql);
        connection.execute_batch(sql)?;
    }
    live_structure(&connection)
}

fn database_is_intact(connection: &rusqlite::Connection) -> StoreResult<bool> {
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Ok(false);
    }
    Ok(connection.prepare("PRAGMA foreign_key_check")?.query([])?.next()?.is_none())
}

fn split_module_schema(sql: &'static str, marker: &str) -> StoreResult<(String, &'static str)> {
    let Some((head, _)) = sql.split_once(marker) else {
        return Err(Error::Invalid("unsupported schema digest"));
    };
    if head.len() + 1 > sql.len() {
        return Err(Error::Invalid("unsupported schema digest"));
    }
    Ok((format!("{head}\n"), &sql[head.len() + 1..]))
}

#[cfg(test)]
mod tests;
