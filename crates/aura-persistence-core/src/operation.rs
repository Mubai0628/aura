use crate::{PersistenceCoreError as Error, StoreResult, record};
use aura_domain_common::{SecurityOperationId, UtcTimestamp};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Prepared,
    Running,
    WaitingExternal,
    CancelRequested,
    Succeeded,
    Failed,
    Cancelled,
    Unknown,
}
impl OperationState {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled | Self::Unknown)
    }
    fn sql(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Running => "running",
            Self::WaitingExternal => "waiting_external",
            Self::CancelRequested => "cancel_requested",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
        }
    }
}

pub struct OperationDraft<'a> {
    pub id: SecurityOperationId,
    pub domain: &'a str,
    pub kind: &'a str,
    pub request_key: &'a str,
    pub version: u32,
    pub payload: &'a Value,
    pub at: UtcTimestamp,
}
#[derive(Debug, PartialEq)]
pub enum BeginOperation {
    Created,
    Existing { operation_id: SecurityOperationId, state: OperationState },
}

/// Identical duplicate requests return identity/current state, never dispatch.
pub fn begin(tx: &Transaction<'_>, d: &OperationDraft<'_>) -> StoreResult<BeginOperation> {
    if d.domain.is_empty() || d.kind.is_empty() || d.request_key.is_empty() || d.version == 0 {
        return Err(Error::Invalid("operation header"));
    }
    let payload = record::encode(d.payload)?;
    let hash = blake3::hash(record::encode(&(d.domain, d.kind, d.version, d.payload))?.as_bytes())
        .to_hex()
        .to_string();
    let previous:Option<(String,String,String)>=tx.query_row("SELECT i.content_hash,o.id,o.state FROM operation_idempotency i JOIN operations o ON o.id=i.operation_id WHERE i.domain=?1 AND i.request_key=?2",params![d.domain,d.request_key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    if let Some((old, id, state)) = previous {
        if old != hash {
            return Err(Error::Conflict);
        }
        return Ok(BeginOperation::Existing {
            operation_id: id.parse().map_err(|_| Error::Invalid("operation ID"))?,
            state: record::decode(&format!("\"{state}\""))?,
        });
    }
    tx.execute(
        "INSERT INTO operations VALUES(?1,?2,?3,?4,?5,?6,1,'prepared',?7,?7)",
        params![d.id.to_string(), d.domain, d.kind, hash, d.version, payload, d.at.to_string()],
    )?;
    tx.execute(
        "INSERT INTO operation_idempotency VALUES(?1,?2,?3,?4)",
        params![d.domain, d.request_key, hash, d.id.to_string()],
    )?;
    journal(tx, d.id, "prepared", d.payload, d.at)?;
    Ok(BeginOperation::Created)
}

pub fn journal(
    tx: &Transaction<'_>,
    id: SecurityOperationId,
    kind: &str,
    payload: &Value,
    at: UtcTimestamp,
) -> StoreResult<()> {
    tx.execute("INSERT INTO operation_journal_entries SELECT ?1,COALESCE(MAX(ordinal),0)+1,?2,?3,?4 FROM operation_journal_entries WHERE operation_id=?1",params![id.to_string(),kind,record::encode(payload)?,at.to_string()])?;
    Ok(())
}

pub fn transition(
    tx: &Transaction<'_>,
    id: SecurityOperationId,
    revision: i64,
    next: OperationState,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let previous: String =
        tx.query_row("SELECT state FROM operations WHERE id=?1", [id.to_string()], |r| r.get(0))?;
    let old: OperationState = record::decode(&format!("\"{previous}\""))?;
    let allowed = match old {
        OperationState::Prepared => matches!(
            next,
            OperationState::Running | OperationState::Cancelled | OperationState::Failed
        ),
        OperationState::Running | OperationState::WaitingExternal => {
            next != OperationState::Prepared
        }
        OperationState::CancelRequested => next.terminal(),
        _ => false,
    };
    if !allowed {
        return Err(Error::Conflict);
    }
    record::changed(tx.execute("UPDATE operations SET state=?3,revision=revision+1,updated_at=?4 WHERE id=?1 AND revision=?2",params![id.to_string(),revision,next.sql(),at.to_string()])?)?;
    journal(tx, id, next.sql(), &Value::Null, at)
}

pub fn acquire_lease(
    tx: &Transaction<'_>,
    id: SecurityOperationId,
    owner: &str,
    now: UtcTimestamp,
    expires: UtcTimestamp,
) -> StoreResult<i64> {
    if owner.is_empty() || expires <= now {
        return Err(Error::Invalid("lease lifetime"));
    }
    let generation:Option<i64>=tx.query_row("INSERT INTO operation_leases VALUES(?1,?2,1,?3) ON CONFLICT(operation_id) DO UPDATE SET owner=excluded.owner,generation=generation+1,expires_at=excluded.expires_at WHERE operation_leases.expires_at<=?4 RETURNING generation",params![id.to_string(),owner,record::sql_time(expires),record::sql_time(now)],|r|r.get(0)).optional()?;
    generation.ok_or(Error::Conflict)
}

pub fn renew_lease(
    tx: &Transaction<'_>,
    id: SecurityOperationId,
    owner: &str,
    generation: i64,
    now: UtcTimestamp,
    expires: UtcTimestamp,
) -> StoreResult<()> {
    if expires <= now {
        return Err(Error::Invalid("lease lifetime"));
    }
    record::changed(tx.execute("UPDATE operation_leases SET expires_at=?5 WHERE operation_id=?1 AND owner=?2 AND generation=?3 AND expires_at>?4",params![id.to_string(),owner,generation,record::sql_time(now),record::sql_time(expires)])?)
}

pub fn recoverable(connection: &Connection) -> StoreResult<Vec<SecurityOperationId>> {
    let mut q=connection.prepare("SELECT id FROM operations WHERE state IN('running','waiting_external','cancel_requested') ORDER BY id")?;
    q.query_map([], |r| r.get::<_, String>(0))?
        .map(|r| r?.parse().map_err(|_| Error::Invalid("operation ID")))
        .collect()
}
