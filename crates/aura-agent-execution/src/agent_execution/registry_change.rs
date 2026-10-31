//! A load request changes the session's dynamic membership; the tool list it builds applies from
//! the next turn.
use super::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, Transaction};
use aura_persistence_v2::tool::{exposure, session_tools};
use std::collections::BTreeSet;

pub(super) fn change(
    tx: &Transaction<'_>,
    call: &CallContext,
    input: &AgentToolLoadInput,
    at: UtcTimestamp,
) -> StoreResult<ToolLoadResultV1> {
    let session = call.snapshot.session_id;
    let saved = session_tools::load(tx, session)?;
    let before =
        saved.as_ref().map_or_else(BTreeSet::new, |s| s.tool_ids.iter().copied().collect());
    let fixed: BTreeSet<_> = call
        .registry
        .catalog_entries
        .iter()
        .filter(|e| matches!(e.load_state, ToolLoadStateV1::Core | ToolLoadStateV1::Pinned))
        .map(|e| e.tool_id)
        .collect();
    let settings = aura_persistence_v2::tool::admission::load(tx, call.batch.tool_batch_id)?
        .ok_or(Error::Conflict)?
        .settings;
    let available = call.registry.catalog_entries.iter().map(|e| e.tool_id).collect();
    let next_ids = aura_tool_registry::update_session_membership(&before, &fixed, &available,
        input, settings.max_dynamic_tool_schemas).map_err(|error| match error {
            aura_tool_registry::SessionMembershipError::FixedTool => Error::Invalid("tools.fixed_tool: fixed tools cannot be unloaded or replaced"),
            aura_tool_registry::SessionMembershipError::UnknownTool => Error::Invalid("tools.unknown_tool_id: search available tools before loading"),
            aura_tool_registry::SessionMembershipError::CapacityExceeded => Error::Invalid("tools.capacity_exceeded: explicitly replace or unload dynamic tools within the configured limit"),
        })?;
    // The request's tool list stays the same for a whole
    // turn so the Provider can reuse its cached prefix. The new membership is what the next turn's
    // registry is built from; until then the tool is reached through tools.call.
    let current = exposure::load(tx, &call.registry.scope)?
        .map_or_else(|| call.registry.snapshot_ref.clone(), |v| v.head);
    if before == next_ids {
        return Ok(ToolLoadResultV1 { changed: false, resulting_registry_snapshot_ref: current });
    }
    session_tools::save(
        tx,
        saved.as_ref(),
        &SessionDynamicTools {
            session_id: session,
            revision: Revision::new(saved.as_ref().map_or(1, |s| s.revision.value() + 1)),
            tool_ids: next_ids.into_iter().collect(),
        },
        at,
    )?;
    Ok(ToolLoadResultV1 { changed: true, resulting_registry_snapshot_ref: current })
}
