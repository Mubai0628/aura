//! What each MCP server listed when Aura last discovered it. The registry derives the
//! server's tools from this record, and only while the record was made for the server's
//! current trusted configuration.
use aura_domain_common::UtcTimestamp;
use aura_mcp::ToolCatalog;
use aura_persistence_core::{StoreResult, record};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

mod credentials;
pub use credentials::*;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct McpServerFacts {
    pub name: String,
    pub version: String,
    pub instructions: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredCatalog {
    pub server_id: String,
    /// The configuration the server was discovered with; a catalog for another one is stale.
    pub config_fingerprint: String,
    pub protocol_version: String,
    pub server: McpServerFacts,
    pub catalog: ToolCatalog,
    pub discovered_at: UtcTimestamp,
}

/// Keeps the newest discovery of one server.
pub fn save(tx: &Transaction<'_>, catalog: &StoredCatalog) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO mcp_catalogs(server_id,config_fingerprint,protocol_version,server_info,catalog,discovered_at) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(server_id) DO UPDATE SET config_fingerprint=excluded.config_fingerprint, protocol_version=excluded.protocol_version, server_info=excluded.server_info, catalog=excluded.catalog, discovered_at=excluded.discovered_at",
        params![
            catalog.server_id,
            catalog.config_fingerprint,
            catalog.protocol_version,
            record::encode(&catalog.server)?,
            record::encode(&catalog.catalog)?,
            catalog.discovered_at.to_string(),
        ],
    )?;
    Ok(())
}

fn row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(String, String, String, String, String, String)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
}

fn decode(
    (server_id, config_fingerprint, protocol_version, server, catalog, discovered_at): (
        String,
        String,
        String,
        String,
        String,
        String,
    ),
) -> StoreResult<StoredCatalog> {
    Ok(StoredCatalog {
        server_id,
        config_fingerprint,
        protocol_version,
        server: record::decode(&server)?,
        catalog: record::decode(&catalog)?,
        discovered_at: record::decode(&serde_json::to_string(&discovered_at)?)?,
    })
}

const COLUMNS: &str =
    "server_id,config_fingerprint,protocol_version,server_info,catalog,discovered_at";

pub fn load(db: &Connection, server_id: &str) -> StoreResult<Option<StoredCatalog>> {
    db.query_row(
        &format!("SELECT {COLUMNS} FROM mcp_catalogs WHERE server_id=?1"),
        [server_id],
        row,
    )
    .optional()?
    .map(decode)
    .transpose()
}

pub fn all(db: &Connection) -> StoreResult<Vec<StoredCatalog>> {
    let mut query =
        db.prepare(&format!("SELECT {COLUMNS} FROM mcp_catalogs ORDER BY server_id"))?;
    let rows = query.query_map([], row)?.collect::<Result<Vec<_>, _>>()?;
    rows.into_iter().map(decode).collect()
}

/// Forgets a removed server's tools. Tool revisions stay: a definition once stored under a
/// revision is never stored again with other content.
pub fn delete(tx: &Transaction<'_>, server_id: &str) -> StoreResult<()> {
    tx.execute("DELETE FROM mcp_catalogs WHERE server_id=?1", [server_id])?;
    Ok(())
}

/// The definition revision of one tool and when it was first assigned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolRevision {
    pub revision: u64,
    pub created_at: UtcTimestamp,
}

fn revision_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(i64, String)> {
    Ok((row.get(0)?, row.get(1)?))
}

fn decode_revision((revision, created_at): (i64, String)) -> StoreResult<ToolRevision> {
    Ok(ToolRevision {
        revision: u64::try_from(revision).map_err(|_| {
            aura_persistence_core::PersistenceCoreError::Invalid("MCP tool revision")
        })?,
        created_at: record::decode(&serde_json::to_string(&created_at)?)?,
    })
}

/// The revision assigned to `definition_key`: everything one tool's definition and instance
/// are derived from. The same key keeps its revision and creation time; a changed key gets
/// the next revision, so `(tool, revision)` always names exactly one definition.
pub fn revision_of(
    db: &Connection,
    tool_id: &str,
    definition_key: &str,
) -> StoreResult<Option<ToolRevision>> {
    db.query_row(
        "SELECT revision,created_at FROM mcp_tool_revisions WHERE tool_id=?1 AND definition_key=?2",
        params![tool_id, definition_key],
        revision_row,
    )
    .optional()?
    .map(decode_revision)
    .transpose()
}

/// [`revision_of`], assigning the next revision at `at` to a key not seen before.
pub fn tool_revision(
    tx: &Transaction<'_>,
    tool_id: &str,
    definition_key: &str,
    at: UtcTimestamp,
) -> StoreResult<ToolRevision> {
    if let Some(known) = revision_of(tx, tool_id, definition_key)? {
        return Ok(known);
    }
    let next: i64 = tx.query_row(
        "SELECT COALESCE(MAX(revision),0)+1 FROM mcp_tool_revisions WHERE tool_id=?1",
        [tool_id],
        |row| row.get(0),
    )?;
    tx.execute(
        "INSERT INTO mcp_tool_revisions(tool_id,definition_key,revision,created_at) VALUES(?1,?2,?3,?4)",
        params![tool_id, definition_key, next, at.to_string()],
    )?;
    decode_revision((next, at.to_string()))
}

/// Whether the user denied one of this server's calls in this turn. A ReadOnly denial covers
/// the server for the rest of the turn, so it is not asked again. The Agent's
/// approval facts name the exact operation each question was about; the answer is the
/// approval's current state.
pub fn denied_in_turn(
    db: &Connection,
    run: aura_domain_common::AgentRunId,
    turn: aura_domain_common::TurnTaskId,
    server_id: &str,
) -> StoreResult<bool> {
    let mut query = db.prepare(
        "SELECT payload FROM events WHERE owner=?1 AND kind='agent.execution_approval' ORDER BY sequence",
    )?;
    let views = query
        .query_map([run.to_string()], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for raw in views {
        let view: aura_execution_contract::ExecutionApprovalView = serde_json::from_str(&raw)?;
        let content = &view.operation.content;
        if content.identity.turn_task_id == Some(turn)
            && matches!(&content.source,
                aura_security_contract::SecuritySourceRef::McpServer { server_id: named } if named == server_id)
            && crate::security_approval::load(db, view.approval.approval_id)?.state
                == aura_security_contract::ApprovalState::Denied
        {
            return Ok(true);
        }
    }
    Ok(false)
}
