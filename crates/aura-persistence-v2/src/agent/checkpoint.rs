//! Immutable model-boundary authority. It is stored before the Provider call,
//! never reconstructed from newer state after receiving model Tool arguments.
use super::{facts, lineage, session, turns};
use aura_agent_domain::{AgentRunState, ModelStepState};
use aura_domain_common::{
    jcs::{CanonicalHashV1, canonical_sha256},
    *,
};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use aura_tool_contract::{CoreInvocationIdentityV1, ToolBatchRequestV1, ToolRegistrySnapshotRef};
use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelBoundary {
    pub identity: CoreInvocationIdentityV1,
    pub context_snapshot_id: ContextProjectionSnapshotId,
    pub registry_snapshot_ref: ToolRegistrySnapshotRef,
    pub state_revision: Revision,
    pub state_hash: CanonicalHashV1,
}
pub fn load(db: &Connection, step: ModelStepId) -> StoreResult<ModelBoundary> {
    let value: ModelBoundary =
        facts::latest(db, &step.to_string(), "agent.model_boundary")?.ok_or(Error::Conflict)?;
    if value.identity.model_step_id != step {
        return Err(Error::Conflict);
    }
    Ok(value)
}
pub fn freeze(
    tx: &Transaction<'_>,
    step_id: ModelStepId,
    context_id: ContextProjectionSnapshotId,
    at: UtcTimestamp,
) -> StoreResult<ModelBoundary> {
    let step = turns::step(tx, step_id)?;
    let run = turns::load_run(tx, step.agent_run_id)?;
    let turn = turns::load(tx, run.turn_task_id)?;
    let context = crate::context::snapshot(tx, context_id)?.ok_or(Error::Conflict)?;
    if step.state != ModelStepState::Preparing
        || run.state != AgentRunState::PreparingContext
        || turn.state.terminal()
        || context.model_step_id != step_id
        || context.agent_run_id != run.agent_run_id
        || context.turn_task_id != turn.turn_task_id
        || context.session_id != turn.session_id
        || context.prompt_bundle_snapshot_id != run.prompt_bundle_snapshot_id
        || facts::latest::<ModelBoundary>(tx, &step_id.to_string(), "agent.model_boundary")?
            .is_some()
    {
        return Err(Error::Conflict);
    }
    let registry = crate::tool::registry(
        tx,
        context.tool_schema_set.registry_snapshot_ref.tool_registry_snapshot_id,
    )?
    .ok_or(Error::Conflict)?;
    if registry.snapshot_ref != context.tool_schema_set.registry_snapshot_ref
        || context.effective_tool_bindings.iter().any(|b| !registry.effective_bindings.contains(b))
    {
        return Err(Error::Conflict);
    }
    let (state_revision, state_hash) = current_state(tx, turn.turn_task_id)?;
    let value = ModelBoundary {
        identity: CoreInvocationIdentityV1 {
            command_invocation_id: CommandInvocationId::new(),
            session_id: turn.session_id,
            turn_task_id: turn.turn_task_id,
            agent_run_id: run.agent_run_id,
            model_step_id: step_id,
        },
        context_snapshot_id: context_id,
        registry_snapshot_ref: registry.snapshot_ref.clone(),
        state_revision,
        state_hash,
    };
    facts::append(tx, EventId::new(), &step_id.to_string(), "agent.model_boundary", &value, at)?;
    Ok(value)
}
pub fn current_state(db: &Connection, id: TurnTaskId) -> StoreResult<(Revision, CanonicalHashV1)> {
    let turn = turns::load(db, id)?;
    let session = session::load(db, turn.session_id)?;
    if turn.state.terminal()
        || session.active_turn != Some(id)
        || if turn.lineage_resolution_required {
            session.active_lineage.is_some()
        } else {
            session.active_lineage != Some(turn.lineage_id)
        }
    {
        return Err(Error::Conflict);
    }
    let lineages = session
        .open_lineages
        .iter()
        .map(|id| lineage::load(db, *id))
        .collect::<StoreResult<Vec<_>>>()?;
    let todo = super::work::todo(db, id)?;
    let query = super::work::query(db, id)?;
    let mut goal = if turn.mode == aura_agent_domain::AgentTaskMode::Goal {
        Some(super::work::goal(db, id)?)
    } else {
        None
    };
    // Metered usage changes when this ModelStep returns. Bind control authority
    // here; independently reload the real usage before the next Provider call.
    if let Some(goal) = &mut goal {
        goal.consumed_tokens = 0;
        goal.consumed_turns = 0;
    }
    let hash = canonical_sha256(&(
        "agent.control-state.v2",
        &turn,
        &session,
        &lineages,
        &todo,
        &query,
        &goal,
    ))
    .map_err(|_| Error::Invalid("Agent state hash"))?;
    Ok((turn.revision, hash))
}
/// Whole-batch admission is checked once against the model boundary. Later
/// calls may legitimately change task state; dispatch separately rechecks live
/// cancellation/mode and completion gates reload all current blockers.
pub fn verify_batch(db: &Connection, batch: &ToolBatchRequestV1) -> StoreResult<()> {
    verify_frozen_batch(db, batch)?;
    let frozen = load(db, batch.invocation_identity.model_step_id)?;
    if current_state(db, frozen.identity.turn_task_id)?
        != (frozen.state_revision, frozen.state_hash)
    {
        return Err(Error::Conflict);
    }
    let run = turns::load_run(db, frozen.identity.agent_run_id)?;
    if !matches!(run.state,AgentRunState::ToolsRunning{tool_batch_id} if tool_batch_id==batch.tool_batch_id)
    {
        return Err(Error::Conflict);
    }
    Ok(())
}
/// Recovery verifies the original model boundary and persisted P05 output,
/// without treating legitimate effects of earlier calls as a new admission.
pub fn verify_frozen_batch(db: &Connection, batch: &ToolBatchRequestV1) -> StoreResult<()> {
    batch.validate_hash().map_err(|_| Error::Conflict)?;
    let frozen = load(db, batch.invocation_identity.model_step_id)?;
    if frozen.identity != batch.invocation_identity
        || frozen.registry_snapshot_ref != batch.registry_snapshot_ref
        || frozen.state_revision != batch.p08_state_revision
        || frozen.state_hash != batch.p08_state_hash
    {
        return Err(Error::Conflict);
    }
    let step = turns::step(db, frozen.identity.model_step_id)?;
    if !matches!(step.state, ModelStepState::Completed { tool_batch_id: Some(id), .. } if id == batch.tool_batch_id)
    {
        return Err(Error::Conflict);
    }
    let snapshot_id: String = db.query_row(
        "SELECT snapshot_id FROM model_steps WHERE id=?1",
        [step.model_step_id.to_string()],
        |r| r.get(0),
    )?;
    let snapshot = crate::model::snapshot(db, snapshot_id.parse().map_err(|_| Error::Conflict)?)?
        .ok_or(Error::Conflict)?;
    let context =
        crate::context::snapshot(db, frozen.context_snapshot_id)?.ok_or(Error::Conflict)?;
    if snapshot.model_step_id != step.model_step_id
        || snapshot.context_snapshot_id != frozen.context_snapshot_id
        || snapshot.prompt_bundle_snapshot_id != context.prompt_bundle_snapshot_id
    {
        return Err(Error::Conflict);
    }
    let evidence = crate::model::request_evidence(db, snapshot.id)?.ok_or(Error::Conflict)?;
    let output = super::history::output(db, step.model_step_id)?;
    // The batch is the output plus the kept `tool.before_call` decisions, both durable.
    let calls = crate::hooks::admitted_calls(db, &snapshot, &evidence, &output)?;
    if calls != batch.calls {
        return Err(Error::Conflict);
    }
    Ok(())
}

pub fn verify_bound_operation(db: &Connection, operation: SecurityOperationId) -> StoreResult<()> {
    if let Some((batch, call)) = crate::tool::execution::operation_call(db, operation)? {
        verify_live_call(db, &batch, call)?;
    }
    Ok(())
}
/// Used only at a new execution boundary. Existing running operations keep
/// their original ownership and are not invalidated by a later addition.
pub fn verify_start_operation(db: &Connection, operation: SecurityOperationId) -> StoreResult<()> {
    verify_bound_operation(db, operation)
}
pub fn verify_live_call(
    db: &Connection,
    batch: &ToolBatchRequestV1,
    call_id: ToolCallId,
) -> StoreResult<()> {
    let stored = crate::tool::execution::batch(db, batch.tool_batch_id)?;
    let call = crate::tool::execution::call(db, call_id)?;
    let id = &batch.invocation_identity;
    let turn = turns::load(db, id.turn_task_id)?;
    let run = turns::load_run(db, id.agent_run_id)?;
    let current = session::load(db, id.session_id)?;
    if stored.batch != *batch
        || stored.result.is_some()
        || call.result.is_some()
        || !batch.calls.contains(&call.call)
        || turn.state.terminal()
        || current.active_turn != Some(id.turn_task_id)
        || run.turn_task_id != turn.turn_task_id
        || !matches!(run.state, AgentRunState::ToolsRunning { tool_batch_id } if tool_batch_id == batch.tool_batch_id)
    {
        return Err(Error::Conflict);
    }
    let registry =
        crate::tool::registry(db, batch.registry_snapshot_ref.tool_registry_snapshot_id)?
            .ok_or(Error::Conflict)?;
    let name = registry
        .catalog_entries
        .iter()
        .find(|v| {
            call.call
                .definition_ref
                .as_ref()
                .is_some_and(|r| v.tool_id == r.tool_id && v.tool_version == r.tool_version)
        })
        .ok_or(Error::Conflict)?
        .canonical_name
        .as_str();
    if (turn.lineage_resolution_required || !current.pending_user_event_ids.is_empty())
        && !matches!(
            name,
            "task_lineage.list_open" | "task_lineage.read_sources" | "task_lineage.reconcile"
        )
    {
        return Err(Error::Conflict);
    }
    Ok(())
}
