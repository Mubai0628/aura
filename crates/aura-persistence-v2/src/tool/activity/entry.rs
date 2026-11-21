//! One tool call, its recorded outcome and optional execution details.
use super::*;

pub(super) fn project(
    db: &Connection,
    session_id: SessionId,
    id: ToolCallId,
    batch_id: ToolBatchId,
) -> StoreResult<AgentToolActivityEntry> {
    let stored = crate::tool::execution::call(db, id)?;
    let batch = crate::tool::execution::batch(db, batch_id)?;
    let Some(reference) = stored.call.definition_ref.as_ref() else {
        return unbound::project(db, &stored, &batch);
    };
    let registry =
        crate::tool::registry(db, reference.registry_snapshot_ref.tool_registry_snapshot_id)?
            .ok_or(Error::Conflict)?;
    let entry = registry
        .catalog_entries
        .iter()
        .find(|v| v.tool_id == reference.tool_id && v.tool_version == reference.tool_version)
        .ok_or(Error::Conflict)?;
    let shell_query_available =
        shell::NAMES.contains(&entry.canonical_name.as_str()) && stored.attempt_id.is_some();
    let authority = crate::tool::admission::load(db, batch_id)?;
    let executing = crate::tool::preparing::load_executing(db, batch_id)?;
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
    // A call that waits for the user's answer to its approval waits for the user, not in a
    // queue: its row says so instead of spinning as if Aura were working on it.
    if stored.result.is_none()
        && status == ToolCallStatusV1::Queued
        && let Some(operation) = authority.as_ref().and_then(|a| a.operations.get(&id))
        && crate::agent::approval::awaits_answer(
            db,
            batch.batch.invocation_identity.agent_run_id,
            operation.operation_id,
        )?
    {
        status = ToolCallStatusV1::WaitingApproval;
    }
    if shell_query_available {
        let view = crate::execution::shell::query(
            db,
            session_id,
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
    // A call reserved but never marked started cannot start once its run has ended: its
    // turn was stopped before it started.
    let never_started = status == ToolCallStatusV1::RecoveryRequired
        && attempt_of(db, id)?.is_some_and(|attempt| attempt.dispatched_at.is_none());
    if never_started {
        status = ToolCallStatusV1::Finalized;
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
    if never_started {
        outcome = Some(ToolResultOutcomeSummaryView::NotExecuted {
            reason: ToolNotExecutedReasonV1::CancelledBeforeDispatch,
        });
    }
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
    let error = stored.result.as_ref().and_then(|r| match &r.outcome {
        ToolResultOutcomeV1::Failed { error }
        | ToolResultOutcomeV1::OperationRejected { error }
        | ToolResultOutcomeV1::Indeterminate { error, .. } => Some(Some(error)),
        ToolResultOutcomeV1::NotExecuted { error, .. } => Some(error.as_ref()),
        ToolResultOutcomeV1::Succeeded | ToolResultOutcomeV1::Cancelled { .. } => None,
    });
    let error_class = error.map(|error| {
        error.map_or(ToolCallErrorClassV1::InvalidRequest, |error| classify(error.category))
    });
    let error_code = error.flatten().map(|error| error.code.clone());
    let mut display = mcp::decorate(
        db,
        package::decorate(
            db,
            memory::decorate(
                git::decorate(
                    display::project(db, &stored, &batch, entry.canonical_name.as_str())?,
                    entry.canonical_name.as_str(),
                    entry.source,
                    stored.result.as_ref(),
                ),
                authority.as_ref().and_then(|a| a.operations.get(&id)),
            ),
            authority.as_ref().and_then(|a| a.operations.get(&id)),
        )?,
        authority.as_ref().and_then(|a| a.operations.get(&id)),
    )?;
    if status == ToolCallStatusV1::RecoveryRequired {
        display.summary = display::INTERRUPTED_WITHOUT_RESULT.into();
    }
    Ok(AgentToolActivityEntry {
        model_step_id: batch.batch.invocation_identity.model_step_id,
        display,
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
            error_code,
            side_effect_state: stored.result.as_ref().map(|r| r.side_effect_state),
            started_at,
            completed_at: stored.result.as_ref().map(|r| r.completed_at),
            can_cancel: stored.result.is_none() && !never_started,
        },
        shell_query_available,
        permission_mode: authority.as_ref().map(|a| a.permission_snapshot.mode),
        permission_revision: authority
            .as_ref()
            .map(|a| a.permission_snapshot.session_permission_revision),
        remote_decision: crate::agent::approver::of_call(db, id)?,
    })
}
