//! The file changes that calls written earlier in one response made and kept. The batch starts a
//! call on a file only after the calls written before it on that file finished, so by then their
//! results are committed here.
use super::execution::CallExecution;
use aura_domain_common::{ContentHash, ToolBatchId};
use aura_execution_contract::{SecuredOperation, SideEffectStateV1};
use aura_persistence_core::{StoreResult, record};
use aura_tool_contract::ToolResultOutcomeV1;
use rusqlite::Connection;

/// A change an earlier call made: what it was asked to do, and the hash it left the file with.
#[derive(Clone, Debug)]
pub struct EarlierChange {
    pub operation: SecuredOperation,
    pub left: ContentHash,
}

/// The `fs.patch` and `fs.write` calls of the batch written before `ordinal` that succeeded and
/// left a file hash, in the order written.
pub fn earlier_changes(
    connection: &Connection,
    batch: ToolBatchId,
    ordinal: u32,
) -> StoreResult<Vec<EarlierChange>> {
    let mut statement = connection.prepare(concat!(
        "SELECT c.payload, o.payload FROM tool_calls c JOIN operations o ON o.id=c.operation_id ",
        "WHERE c.batch_id=?1"
    ))?;
    let rows = statement
        .query_map([batch.to_string()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut changes = Vec::new();
    for row in rows {
        let (call, operation) = row?;
        let call: CallExecution = record::decode(&call)?;
        let kept = call.result.as_ref().is_some_and(|result| {
            matches!(result.outcome, ToolResultOutcomeV1::Succeeded)
                && result.side_effect_state != SideEffectStateV1::Unknown
        });
        if call.call.ordinal >= ordinal || !kept {
            continue;
        }
        let operation: SecuredOperation = serde_json::from_str(&operation)?;
        if !matches!(operation.content.action_kind.as_str(), "fs.patch" | "fs.write") {
            continue;
        }
        let (_, output) = super::result::execution_output(connection, call.call.tool_call_id)?;
        let Ok(left) = serde_json::from_value::<ContentHash>(output["logical_hash"].clone()) else {
            continue;
        };
        changes.push((call.call.ordinal, EarlierChange { operation, left }));
    }
    changes.sort_by_key(|(ordinal, _)| *ordinal);
    Ok(changes.into_iter().map(|(_, change)| change).collect())
}
