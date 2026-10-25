//! Restart-stable Core admission inputs. The exact operation ID/content survives
//! approvals and reconnects; the Store never recomputes a more permissive call.
use aura_domain_common::*;
use aura_execution_contract::SecuredOperation;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, event, record};
use aura_security_contract::PermissionSnapshot;
use aura_tool_contract::*;
use rusqlite::{Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AdmissionInputs {
    pub permission_snapshot: PermissionSnapshot,
    pub settings: ToolSettingsSnapshotV1,
    pub operations: BTreeMap<ToolCallId, SecuredOperation>,
}
pub fn load(db: &Connection, id: ToolBatchId) -> StoreResult<Option<AdmissionInputs>> {
    let raw: Option<String> = db.query_row("SELECT payload FROM events WHERE owner=?1 AND kind='tool.admission_inputs' AND version=1 ORDER BY sequence DESC LIMIT 1",
        [id.to_string()], |r| r.get(0)).optional()?;
    raw.map(|s| record::decode(&s)).transpose()
}
pub fn record(
    tx: &Transaction<'_>,
    batch: &ToolBatchRequestV1,
    snapshot: &PermissionSnapshot,
    settings: &ToolSettingsSnapshotV1,
    operations: BTreeMap<ToolCallId, SecuredOperation>,
) -> StoreResult<()> {
    let stored = super::execution::batch(tx, batch.tool_batch_id)?;
    if stored.batch != *batch
        || stored.plan.tool_settings_hash != settings.content_hash
        || stored.plan.tool_settings_revision != settings.revision
        || snapshot.session_id != batch.invocation_identity.session_id
        || crate::security::snapshot(tx, snapshot.id)?.as_ref() != Some(snapshot)
        || operations.len() != stored.plan.calls.len()
        || stored.plan.calls.iter().any(|c| !operations.contains_key(&c.tool_call_id))
        || operations
            .values()
            .map(|o| o.operation_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != operations.len()
    {
        return Err(Error::Conflict);
    }
    for operation in operations.values() {
        operation.validate().map_err(|_| Error::Conflict)?;
        let identity = &operation.content.identity;
        if identity.session_id != Some(snapshot.session_id)
            || identity.workspace_id != Some(snapshot.workspace_id)
            || identity.permission_snapshot_id != Some(snapshot.id)
            || identity.turn_task_id != Some(batch.invocation_identity.turn_task_id)
            || identity.agent_run_id != Some(batch.invocation_identity.agent_run_id)
        {
            return Err(Error::Conflict);
        }
    }
    let value = AdmissionInputs {
        permission_snapshot: snapshot.clone(),
        settings: settings.clone(),
        operations,
    };
    if let Some(previous) = load(tx, batch.tool_batch_id)? {
        return if value == previous { Ok(()) } else { Err(Error::Conflict) };
    }
    event::append(
        tx,
        &event::EventDraft {
            id: EventId::new(),
            owner: &batch.tool_batch_id.to_string(),
            kind: "tool.admission_inputs",
            version: 1,
            payload: &serde_json::to_value(value)?,
            at: stored.admitted_at,
        },
        "tool",
    )?;
    Ok(())
}
