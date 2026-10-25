//! Session-bound pagination and real start/result facts for the desktop.
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use aura_tool_contract::*;
use aura_tool_projection::*;
use rusqlite::{Connection, OptionalExtension};
mod browser;
mod display;
pub(crate) use display::title as tool_title;
mod git;
mod mcp;
mod memory;
mod network_route;
mod package;
mod query;
mod shell;
mod unbound;
mod workspace_init;

/// The display class of one stored error category.
///
/// `Validation` covers both the argument envelope and the tool's own schema, because
/// what a person has to do about either is the same: nothing ran, and the request has to
/// be corrected. A `cancelled` or `transport` failure is deliberately `Other`: it is a
/// real failure with a real cause, and this vocabulary does not claim to know which
/// correction it needs.
fn classify(category: aura_domain_common::ErrorCategory) -> ToolCallErrorClassV1 {
    use aura_domain_common::ErrorCategory as Category;
    match category {
        Category::NotFound => ToolCallErrorClassV1::NotFound,
        Category::Permission => ToolCallErrorClassV1::Permission,
        Category::Conflict => ToolCallErrorClassV1::Conflict,
        Category::Validation => ToolCallErrorClassV1::InvalidRequest,
        Category::Unavailable | Category::Compatibility => ToolCallErrorClassV1::Unavailable,
        Category::Cancelled | Category::Transport | Category::Protocol | Category::Internal => {
            ToolCallErrorClassV1::Other
        }
    }
}

/// Whether the run is over: completed, cancelled or waiting for recovery.
fn run_ended(db: &Connection, run: AgentRunId) -> StoreResult<bool> {
    let state: Option<String> = db
        .query_row("SELECT state FROM agent_runs WHERE id=?1", [run.to_string()], |r| r.get(0))
        .optional()?;
    Ok(matches!(state.as_deref(), Some("completed" | "cancelled" | "recovery_required")))
}

pub fn page(
    db: &Connection,
    input: &AgentToolActivityRequest,
) -> StoreResult<AgentToolActivityPage> {
    if input.limit == 0 || input.limit > 100 {
        return Err(Error::Invalid("activity limit"));
    }
    let mut carried = Vec::new();
    let rows = if let Some(ids) = &input.call_ids {
        if ids.is_empty()
            || ids.len() > input.limit as usize
            || input.before_call_id.is_some()
            || ids.iter().collect::<std::collections::BTreeSet<_>>().len() != ids.len()
        {
            return Err(Error::Invalid("visible activity identities"));
        }
        // A conversation that does not exist is refused; inside one, a call is read only there.
        db.query_row("SELECT 1 FROM sessions WHERE id=?1", [input.session_id.to_string()], |_| {
            Ok(())
        })?;
        let mut rows = Vec::with_capacity(ids.len());
        // A call of a turn a branch carried is shown from what the branch kept of it.
        carried = crate::agent::branch::carried_activity(db, input.session_id, ids)?;
        for id in ids.iter().filter(|id| !carried.iter().any(|(call, _)| call == *id)) {
            // A call not stored (yet) - one the model wrote whose batch is not admitted, or
            // never was because its step was interrupted - is left out rather than failing the
            // read of every call asked with it.
            rows.extend(db.query_row("SELECT c.id,c.batch_id FROM tool_calls c JOIN tool_batches b ON c.batch_id=b.id WHERE c.id=?1 AND b.session_id=?2",
                [id.to_string(), input.session_id.to_string()], |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?))).optional()?);
        }
        rows
    } else {
        let before: i64 = if let Some(call) = input.before_call_id {
            db.query_row("SELECT c.rowid FROM tool_calls c JOIN tool_batches b ON c.batch_id=b.id WHERE c.id=?1 AND b.session_id=?2", [call.to_string(), input.session_id.to_string()], |r| r.get(0))?
        } else {
            i64::MAX
        };
        let mut statement = db.prepare("SELECT c.id,c.batch_id FROM tool_calls c JOIN tool_batches b ON c.batch_id=b.id WHERE b.session_id=?1 AND c.rowid<?2 ORDER BY c.rowid DESC LIMIT ?3")?;
        statement
            .query_map(
                rusqlite::params![input.session_id.to_string(), before, input.limit + 1],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )?
            .collect::<Result<Vec<_>, _>>()?
    };
    let more = rows.len() > input.limit as usize;
    let mut calls = vec![];
    for (id, batch_id) in rows.into_iter().take(input.limit as usize) {
        let id: ToolCallId = id.parse().map_err(|_| Error::Conflict)?;
        let batch_id: ToolBatchId = batch_id.parse().map_err(|_| Error::Conflict)?;
        let stored = super::execution::call(db, id)?;
        let batch = super::execution::batch(db, batch_id)?;
        let Some(reference) = stored.call.definition_ref.as_ref() else {
            calls.push(unbound::project(db, &stored, &batch)?);
            continue;
        };
        let registry =
            super::registry(db, reference.registry_snapshot_ref.tool_registry_snapshot_id)?
                .ok_or(Error::Conflict)?;
        let entry = registry
            .catalog_entries
            .iter()
            .find(|v| v.tool_id == reference.tool_id && v.tool_version == reference.tool_version)
            .ok_or(Error::Conflict)?;
        let shell = matches!(entry.canonical_name.as_str(), "shell.powershell" | "shell.cmd");
        let authority = super::admission::load(db, batch_id)?;
        let executing = super::preparing::load_executing(db, batch_id)?;
        let mut started_at = stored.result.as_ref().and_then(|r| r.started_at);
        let mut status = if stored.result.is_some() {
            ToolCallStatusV1::Finalized
        } else {
            ToolCallStatusV1::Queued
        };
        // The execution lifecycle decides Running for every Tool, not only the two
        // Shell names. A claimed Attempt means Core has already moved the call out
        // of the queue and is performing it, so the projection reports the real fact
        // instead of leaving a working call looking queued.
        //
        // Preparation is *not* that fact. Backend selection, readiness checks, an approval
        // wait and a wait for a free execution slot all happen before an Attempt is
        // reserved; a call in that window is still waiting its turn, and reporting Running
        // from the preparing mark alone made a queued call look like a running one for as
        // long as the wait lasted. The preparing mark stays an internal timestamp: it says
        // Core has begun working on the call, not that execution started.
        if stored.result.is_none()
            && let Some(started) = executing.get(&id)
        {
            started_at = started_at.or(Some(*started));
            status = ToolCallStatusV1::Running;
        }
        if stored.result.is_none()
            && status != ToolCallStatusV1::Running
            && let Some(operation) = authority.as_ref().and_then(|a| a.operations.get(&id))
        {
            // One admission reserves at most one Attempt per logical operation, so
            // the lowest ordinal is the only one that can exist here.
            let attempt: Option<String> = db
                .query_row(
                    "SELECT payload FROM execution_attempts WHERE operation_id=?1 \
                     ORDER BY ordinal LIMIT 1",
                    [operation.operation_id.to_string()],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(attempt) = attempt {
                let attempt: aura_execution_contract::ExecutionAttempt =
                    aura_persistence_core::record::decode(&attempt)?;
                // Dispatch time is the most precise start fact available; a call
                // that is claimed but not yet dispatched still started processing.
                started_at = started_at.or(attempt.dispatched_at).or(Some(attempt.prepared_at));
                status = ToolCallStatusV1::Running;
            }
        }
        if shell && matches!(stored.call.input, NormalizedToolCallInputStateV1::Ready { .. }) {
            let view = crate::execution::shell::query(
                db,
                input.session_id,
                &aura_windows_user_contract::ShellQueryInput {
                    tool_call_id: id,
                    stdout_offset: 0,
                    stderr_offset: 0,
                    max_bytes: 1,
                },
            )?;
            // The Shell's own process facts refine the start time; they never
            // downgrade a call the generic lifecycle already reported as running.
            started_at = started_at.or(view.started_at);
            if view.state == aura_windows_user_contract::ShellCallState::Running {
                status = ToolCallStatusV1::Running;
            }
        }
        if entry.canonical_name.as_str().starts_with("web.")
            && let Some(operation) = authority.as_ref().and_then(|a| a.operations.get(&id))
        {
            let actual: Option<String> = db.query_row("SELECT created_at FROM operation_journal_entries WHERE operation_id=?1 AND kind='web.transport_started' ORDER BY ordinal LIMIT 1",
                [operation.operation_id.to_string()], |r| r.get(0)).optional()?;
            if let Some(at) = actual {
                started_at = Some(at.parse().map_err(|_| Error::Conflict)?);
                if stored.result.is_none() {
                    status = ToolCallStatusV1::Running;
                }
            }
        }
        // A call its run left without a result when
        // the run ended (an unresolved call of a stopped or interrupted run) no longer runs. A
        // background command has its result and may well outlive its run.
        if stored.result.is_none()
            && status == ToolCallStatusV1::Running
            && run_ended(db, batch.batch.invocation_identity.agent_run_id)?
        {
            status = ToolCallStatusV1::RecoveryRequired;
        }
        let mut outcome = stored.result.as_ref().map(|r| match r.outcome {
            ToolResultOutcomeV1::Succeeded => ToolResultOutcomeSummaryView::Succeeded,
            ToolResultOutcomeV1::OperationRejected { .. } => {
                ToolResultOutcomeSummaryView::OperationRejected
            }
            ToolResultOutcomeV1::Failed { .. } => ToolResultOutcomeSummaryView::Failed,
            ToolResultOutcomeV1::Cancelled { reason } => {
                ToolResultOutcomeSummaryView::Cancelled { reason }
            }
            ToolResultOutcomeV1::NotExecuted { reason, .. } => {
                ToolResultOutcomeSummaryView::NotExecuted { reason }
            }
            ToolResultOutcomeV1::Indeterminate { reason, .. } => {
                ToolResultOutcomeSummaryView::Indeterminate { reason }
            }
        });
        // A question still waiting for the user is not finished in the
        // conversation either. Its record says so and is read again until the question ends; it
        // stayed "等待你的回答" after the answer when the call's own result counted as its end.
        if entry.canonical_name.as_str() == query::NAME
            && stored.result.is_some()
            && query::waiting(db, batch.batch.invocation_identity.turn_task_id, id)?
        {
            status = ToolCallStatusV1::Running;
            outcome = None;
        }
        // The row's reason comes from the stored error's category, so a screen can say
        // "the target does not exist" only when the Store recorded `not_found`. It is
        // deliberately not derived from the message text: that is how four missing
        // paths came to be displayed as a permission refusal.
        let error_class = stored.result.as_ref().and_then(|r| match &r.outcome {
            ToolResultOutcomeV1::Failed { error }
            | ToolResultOutcomeV1::OperationRejected { error }
            | ToolResultOutcomeV1::Indeterminate { error, .. } => Some(classify(error.category)),
            ToolResultOutcomeV1::NotExecuted { error, .. } => Some(
                error
                    .as_ref()
                    .map_or(ToolCallErrorClassV1::InvalidRequest, |error| classify(error.category)),
            ),
            ToolResultOutcomeV1::Succeeded | ToolResultOutcomeV1::Cancelled { .. } => None,
        });
        calls.push(AgentToolActivityEntry {
            model_step_id: batch.batch.invocation_identity.model_step_id,
            display: mcp::decorate(
                db,
                package::decorate(
                    db,
                    memory::decorate(
                        git::decorate(
                            display::project(db, &stored, &batch, entry.canonical_name.as_str())?,
                            entry.canonical_name.as_str(),
                            entry.source,
                        ),
                        authority.as_ref().and_then(|a| a.operations.get(&id)),
                    ),
                    authority.as_ref().and_then(|a| a.operations.get(&id)),
                )?,
                authority.as_ref().and_then(|a| a.operations.get(&id)),
            )?,
            summary: ToolCallActivitySummaryView {
                tool_call_id: id,
                tool_batch_id: batch_id,
                ordinal: stored.call.ordinal,
                tool_id: Some(reference.tool_id),
                tool_version: Some(reference.tool_version.clone()),
                display_name: entry.canonical_name.as_str().into(),
                status,
                outcome,
                error_class,
                side_effect_state: stored.result.as_ref().map(|r| r.side_effect_state),
                started_at,
                completed_at: stored.result.as_ref().map(|r| r.completed_at),
                can_cancel: stored.result.is_none(),
            },
            shell_query_available: shell
                && matches!(stored.call.input, NormalizedToolCallInputStateV1::Ready { .. }),
            permission_mode: authority.as_ref().map(|a| a.permission_snapshot.mode),
            permission_revision: authority
                .as_ref()
                .map(|a| a.permission_snapshot.session_permission_revision),
        });
    }
    let next_before_call_id =
        if more { calls.last().map(|v| v.summary.tool_call_id) } else { None };
    calls.extend(carried.into_iter().map(|(_, entry)| entry));
    Ok(AgentToolActivityPage { session_id: input.session_id, calls, next_before_call_id })
}
