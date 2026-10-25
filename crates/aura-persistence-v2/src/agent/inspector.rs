use super::{facts, turns};
use aura_agent_domain::{AgentInspectorRequest, ModelStepState};
use aura_context_domain::{
    AgentPromptInspection, ContextProjectionSnapshot, PromptInspectorProjection,
    PromptInspectorTaskLineageView, SessionCacheUsageView,
};
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, OptionalExtension, Transaction};

/// A turn awaiting semantic reconciliation has only a reserved lineage ID.
/// Do not fabricate that lineage or read it as an existing persistent record.
pub fn active_lineage_view(
    db: &Connection,
    turn_id: TurnTaskId,
) -> StoreResult<Option<PromptInspectorTaskLineageView>> {
    let turn = turns::load(db, turn_id)?;
    if turn.lineage_resolution_required {
        return Ok(None);
    }
    lineage_view(db, turn.lineage_id).map(Some)
}

pub fn lineage_view(
    db: &Connection,
    id: TaskLineageId,
) -> StoreResult<PromptInspectorTaskLineageView> {
    let lineage = super::lineage::load(db, id)?;
    let preview = lineage
        .sources
        .iter()
        .map(|source| source.immutable_preview.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    Ok(PromptInspectorTaskLineageView {
        lineage_id: id,
        revision: lineage.revision,
        state: serde_json::to_value(lineage.state)?.as_str().ok_or(Error::Conflict)?.into(),
        source_count: u32::try_from(lineage.sources.len()).map_err(|_| Error::Conflict)?,
        completion_policy: serde_json::to_value(&lineage.completion_policy)?["policy"]
            .as_str()
            .ok_or(Error::Conflict)?
            .into(),
        completion_gate: serde_json::to_value(&lineage.completion_gate)?["gate"]
            .as_str()
            .ok_or(Error::Conflict)?
            .into(),
        source_recheck_required: lineage.source_recheck_required(),
        preview_characters: preview.chars().count() as u64,
        // Matches the P06 default UTF-8 byte upper-bound estimator.
        preview_estimated_tokens: preview.len() as u64,
    })
}

pub fn previous_context(
    db: &Connection,
    session: SessionId,
) -> StoreResult<Option<ContextProjectionSnapshot>> {
    let id: Option<String> = db.query_row("SELECT c.id FROM context_snapshots c JOIN model_execution_snapshots e ON json_extract(e.payload,'$.context_snapshot_id')=c.id JOIN model_steps s ON s.snapshot_id=e.id WHERE c.session_id=?1 ORDER BY c.rowid DESC LIMIT 1",
        [session.to_string()], |r| r.get(0)).optional()?;
    match id {
        Some(id) => crate::context::snapshot(db, id.parse().map_err(|_| Error::Conflict)?),
        None => Ok(None),
    }
}

pub fn freeze(
    tx: &Transaction<'_>,
    view: &PromptInspectorProjection,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let step = view.active_model_step_id.ok_or(Error::Conflict)?;
    let snapshot = crate::model::step_snapshot(tx, step)?;
    if view.context_snapshot_id != Some(snapshot.context_snapshot_id)
        || view.prompt_bundle_snapshot_id != Some(snapshot.prompt_bundle_snapshot_id)
        || view.model_profile_id != Some(snapshot.model_profile_id)
        || facts::latest::<PromptInspectorProjection>(
            tx,
            &step.to_string(),
            "agent.prompt_inspector",
        )?
        .is_some()
    {
        return Err(Error::Conflict);
    }
    facts::append(tx, EventId::new(), &step.to_string(), "agent.prompt_inspector", view, at)
}

pub fn read(
    db: &Connection,
    request: &AgentInspectorRequest,
) -> StoreResult<AgentPromptInspection> {
    let step = turns::step(db, request.model_step_id)?;
    let run = turns::load_run(db, step.agent_run_id)?;
    if turns::load(db, run.turn_task_id)?.session_id != request.session_id {
        return Err(Error::Conflict);
    }
    let mut projection: PromptInspectorProjection =
        facts::latest(db, &step.model_step_id.to_string(), "agent.prompt_inspector")?
            .ok_or(Error::Invalid("agent.inspector_not_recorded"))?;
    let execution = crate::model::step_snapshot(db, step.model_step_id)?;
    let context =
        crate::context::snapshot(db, execution.context_snapshot_id)?.ok_or(Error::Conflict)?;
    if projection.session_id != request.session_id
        || projection.context_snapshot_id != Some(context.id)
    {
        return Err(Error::Conflict);
    }
    if matches!(step.state, ModelStepState::Completed { .. }) {
        let usage = super::history::output(db, step.model_step_id)?.usage;
        projection.cache_usage.actual_input_tokens = Some(usage.input_tokens);
        projection.cache_usage.actual_cached_input_tokens = usage.cached_input_tokens;
    }
    // A read that cannot add up the conversation still shows the step.
    let session_cache = session_cache(db, request.session_id).ok();
    Ok(AgentPromptInspection { projection, execution, context, session_cache })
}

/// The conversation's finished requests, and what their Providers read from the cache. Each
/// finished step names the response event it recorded; the usage is read from there.
pub fn session_cache(db: &Connection, session: SessionId) -> StoreResult<SessionCacheUsageView> {
    let (steps, reported_steps, input_tokens, cached_input_tokens): (i64, i64, i64, i64) = db
        .query_row(
            "SELECT count(*),                     count(json_extract(e.payload,'$.usage.cached_input_tokens')),                     coalesce(sum(CASE WHEN json_extract(e.payload,'$.usage.cached_input_tokens')                         IS NOT NULL THEN json_extract(e.payload,'$.usage.input_tokens') END),0),                     coalesce(sum(json_extract(e.payload,'$.usage.cached_input_tokens')),0)              FROM turn_tasks t JOIN agent_runs r ON r.turn_task_id=t.id              JOIN model_steps m ON m.agent_run_id=r.id AND m.state='completed'              JOIN events e ON e.event_id=json_extract(m.payload,'$.state.assistant_event_id')              WHERE t.session_id=?1",
            [session.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
    let count = |value: i64| u64::try_from(value).unwrap_or_default();
    Ok(SessionCacheUsageView {
        steps: count(steps),
        reported_steps: count(reported_steps),
        input_tokens: count(input_tokens),
        cached_input_tokens: count(cached_input_tokens),
    })
}
