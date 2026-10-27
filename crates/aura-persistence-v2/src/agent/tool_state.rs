//! Task-local Built-in effects. Input is reloaded from the exact admitted call;
//! the Core handler cannot substitute new arguments or a foreign task identity.
use super::{checkpoint, facts, lineage, session, turns, work};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_execution_contract::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use aura_tool_contract::*;
use rusqlite::{Connection, Transaction};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

fn input<T: DeserializeOwned>(value: &Value) -> StoreResult<T> {
    serde_json::from_value(value.clone()).map_err(Into::into)
}
fn value<T: Serialize>(value: T) -> StoreResult<Value> {
    serde_json::to_value(value).map_err(Into::into)
}
fn next(revision: Revision) -> StoreResult<Revision> {
    Ok(Revision::new(revision.value().checked_add(1).ok_or(Error::Conflict)?))
}

pub fn inspect(db: &Connection, batch: &ToolBatchRequestV1) -> StoreResult<Value> {
    let id = &batch.invocation_identity;
    let turn = turns::load(db, id.turn_task_id)?;
    let current = session::load(db, id.session_id)?;
    let lineages = current
        .open_lineages
        .iter()
        .map(|id| lineage::load(db, *id))
        .collect::<StoreResult<Vec<_>>>()?;
    let goal = if turn.mode == AgentTaskMode::Goal {
        Some(work::goal(db, id.turn_task_id)?)
    } else {
        None
    };
    let registry =
        crate::tool::registry(db, batch.registry_snapshot_ref.tool_registry_snapshot_id)?
            .ok_or(Error::Conflict)?;
    Ok(
        json!({"turn":turn,"session":current,"lineages":lineages,"todo":work::todo(db,id.turn_task_id)?,"goal":goal,"query":work::query(db,id.turn_task_id)?,"tool_exposure":crate::tool::exposure::load(db,&registry.scope)?,"skills":super::skills::loaded(db,id.agent_run_id)?}),
    )
}
pub fn execute(
    tx: &Transaction<'_>,
    request: &DispatchRequest,
    batch: &ToolBatchRequestV1,
    call_id: ToolCallId,
    at: UtcTimestamp,
) -> StoreResult<()> {
    execute_with(tx, request, batch, call_id, at, |tx, name, arguments| {
        dispatch(tx, batch, call_id, name, arguments, at)
    })
}
pub fn execute_with(
    tx: &Transaction<'_>,
    request: &DispatchRequest,
    batch: &ToolBatchRequestV1,
    call_id: ToolCallId,
    at: UtcTimestamp,
    handler: impl FnOnce(&Transaction<'_>, &str, &Value) -> StoreResult<Value>,
) -> StoreResult<()> {
    checkpoint::verify_live_call(tx, batch, call_id)?;
    let call = crate::tool::execution::call(tx, call_id)?;
    let NormalizedToolCallInputStateV1::Ready { arguments, .. } = &call.call.input else {
        return Err(Error::Conflict);
    };
    let before = request.plan.prepared.preparation["domain"].clone();
    if before != inspect(tx, batch)?
        || request.operation.content.action != *arguments
        || crate::execution::cancellation_requested(tx, &request.binding.attempt)?
    {
        return Err(Error::Conflict);
    }
    // Some domain actions validate several sources before applying changes.
    // Any failure discards all intermediate effects but publishes a real error.
    tx.execute_batch("SAVEPOINT agent_task_effect")?;
    let output = handler(tx, &request.operation.content.action_kind, arguments);
    match &output {
        Ok(_) => tx.execute_batch("RELEASE agent_task_effect")?,
        Err(_) => tx.execute_batch("ROLLBACK TO agent_task_effect; RELEASE agent_task_effect")?,
    }
    let after = inspect(tx, batch)?;
    let turn = turns::load(tx, batch.invocation_identity.turn_task_id)?;
    let operation_rejection = match output.as_ref().err() {
        Some(Error::Invalid(message)) => classify_invalid(message),
        _ => None,
    };
    if let Some((_, Some(failure))) = &operation_rejection {
        super::guard::record_failure(tx, turn.turn_task_id, failure, at)?;
    }
    if output.is_ok() && request.operation.content.action_kind == "task_lineage.reconcile" {
        clear_satisfied_lineage(tx, turn.turn_task_id, arguments, at)?;
    }
    let operation_rejection = operation_rejection.map(|(error, _)| error);
    let evidence = BrokerEvidence {
        action_kind: request.operation.content.action_kind.clone(),
        request_fingerprint: request.operation.fingerprint.0.clone(),
        side_effect: if before == after {
            SideEffectStateV1::NoEffect
        } else {
            SideEffectStateV1::FullyApplied
        },
        before,
        after,
        domain_journal: json!({"schema":"agent-task-effect-v1","output":output.as_ref().ok(),"error":output.as_ref().err().map(ToString::to_string),"operation_rejection":operation_rejection}),
    };
    crate::execution::broker_result::record(tx, request, &evidence, output.is_ok(), at)?;
    Ok(())
}
fn dispatch(
    tx: &Transaction<'_>,
    batch: &ToolBatchRequestV1,
    call_id: ToolCallId,
    name: &str,
    arguments: &Value,
    at: UtcTimestamp,
) -> StoreResult<Value> {
    if disposition_blocks(tx, batch, name)? {
        return Err(Error::Invalid("agent.mode_disposition_required"));
    }
    let id = &batch.invocation_identity;
    let turn = turns::load(tx, id.turn_task_id)?;
    match name {
        "shell.query" => value(crate::execution::shell_result::model_page(
            tx,
            id.session_id,
            &input(arguments)?,
        )?),
        "context.read_events" => value(crate::context::read::events(
            tx,
            id.session_id,
            id.agent_run_id,
            &input(arguments)?,
            at,
        )?),
        "context.read_tool_result" => value(crate::context::read::tool_result(
            tx,
            id.session_id,
            id.agent_run_id,
            &input(arguments)?,
            at,
        )?),
        "context.read_compaction" => value(crate::context::read::compaction(
            tx,
            id.session_id,
            id.agent_run_id,
            &input(arguments)?,
        )?),
        "todo.get" => {
            let _: EmptyToolInput = input(arguments)?;
            value(work::todo(tx, id.turn_task_id)?)
        }
        "todo.replace" => {
            let i: TodoReplaceInput = input(arguments)?;
            let old = work::todo(tx, id.turn_task_id)?;
            if old.revision != i.expected_revision {
                return Err(Error::Conflict);
            }
            let state = TodoState {
                turn_task_id: old.turn_task_id,
                revision: next(old.revision)?,
                items: i
                    .items
                    .into_iter()
                    .map(|i| TodoItem { id: TodoItemId::new(), title: i.title, status: i.status })
                    .collect(),
            };
            work::save_todo(tx, &old, &state, at)?;
            value(state)
        }
        "todo.update" => {
            let i: TodoUpdateInput = input(arguments)?;
            let old = work::todo(tx, id.turn_task_id)?;
            if old.revision != i.expected_revision {
                return Err(Error::Conflict);
            }
            let mut state = old.clone();
            let item = state.items.iter_mut().find(|v| v.id == i.item_id).ok_or(Error::Conflict)?;
            item.status = i.status;
            if let Some(title) = i.title {
                item.title = title;
            }
            state.revision = next(old.revision)?;
            work::save_todo(tx, &old, &state, at)?;
            value(state)
        }
        "goal.get" => {
            let _: EmptyToolInput = input(arguments)?;
            value(work::goal(tx, id.turn_task_id)?)
        }
        "goal.check" => {
            let i: GoalCheckInput = input(arguments)?;
            let old = work::goal(tx, id.turn_task_id)?;
            if old.revision != i.expected_goal_revision || i.assessment.trim().is_empty() {
                return Err(Error::Conflict);
            }
            super::evidence::verify(tx, &i.evidence_refs, &id.agent_run_id.to_string())?;
            let mut state = old.clone();
            state.revision = next(old.revision)?;
            state.latest_check_model_step = Some(id.model_step_id);
            work::save_goal(tx, &old, &state, at)?;
            facts::append(
                tx,
                EventId::new(),
                &id.turn_task_id.to_string(),
                "agent.goal_check",
                &i,
                at,
            )?;
            value(state)
        }
        "query.user_question" => {
            let i: QueryUserQuestionInput = input(arguments)?;
            let old = work::query(tx, id.turn_task_id)?;
            if old.as_ref().is_some_and(|q| q.state == UserQueryState::Pending) {
                return Err(Error::Conflict);
            }
            let query = UserQuery {
                query_id: UserQueryId::new(),
                turn_task_id: id.turn_task_id,
                agent_run_id: id.agent_run_id,
                tool_call_id: call_id,
                revision: Revision::INITIAL,
                input: i,
                state: UserQueryState::Pending,
                created_at: at,
            };
            work::save_query(tx, old.as_ref(), &query, at)?;
            value(query)
        }
        "task_lineage.list_open" => {
            let _: EmptyToolInput = input(arguments)?;
            let state = session::load(tx, id.session_id)?;
            let mut lineages = vec![];
            for id in &state.open_lineages {
                let l = lineage::load(tx, *id)?;
                let root = l
                    .sources
                    .iter()
                    .find(|s| s.user_event_id == l.root_user_event_id)
                    .ok_or(Error::Conflict)?;
                lineages.push(OpenTaskLineageSummary {
                    lineage_id: *id,
                    state: l.state,
                    root_user_event_id: l.root_user_event_id,
                    root_preview: root.immutable_preview.clone(),
                    source_count: l.sources.len().try_into().map_err(|_| Error::Conflict)?,
                    revision: l.revision,
                    last_active_at: l.updated_at,
                });
            }
            value(TaskLineageListOpenResult { active_lineage_id: state.active_lineage, lineages })
        }
        "task_lineage.read_sources" => {
            let i: TaskLineageReadSourcesInput = input(arguments)?;
            let l = lineage::load(tx, i.lineage_id)?;
            if l.session_id != id.session_id
                || l.revision != i.expected_lineage_revision
                || !session::load(tx, id.session_id)?.open_lineages.contains(&l.lineage_id)
            {
                return Err(Error::Conflict);
            }
            let messages = i
                .source_event_ids
                .iter()
                .map(|e| session::user_message(tx, id.session_id, *e))
                .collect::<StoreResult<Vec<_>>>()?;
            let receipt = l
                .source_read_receipt(
                    &messages,
                    &turn,
                    id.model_step_id,
                    SourceReadReceiptId::new(),
                    at,
                )
                .map_err(|_| Error::Conflict)?;
            lineage::save_receipt(tx, &receipt)?;
            value(TaskLineageReadSourcesResult {
                lineage_id: l.lineage_id,
                lineage_revision: l.revision,
                messages,
                source_read_receipt_id: receipt.receipt_id,
                completion_source_recheck_required: l.source_recheck_required(),
            })
        }
        "task_lineage.reconcile" => {
            let bound = bound_reconcile(tx, &turn, arguments)?;
            value(super::reconcile::apply(tx, turn.turn_task_id, &bound, at)?)
        }
        "mode.disposition" => {
            let input = input::<ModeDispositionInput>(arguments)?;
            value(super::mode::decide(tx, id.turn_task_id, &input, at)?)
        }
        _ => Err(Error::Invalid("not a task-state handler")),
    }
}

fn disposition_blocks(
    tx: &Transaction<'_>,
    batch: &ToolBatchRequestV1,
    name: &str,
) -> StoreResult<bool> {
    let registry =
        crate::tool::registry(tx, batch.registry_snapshot_ref.tool_registry_snapshot_id)?
            .ok_or(Error::Conflict)?;
    let mut names = Vec::new();
    for call in &batch.calls {
        if call.definition_ref.is_none() {
            continue;
        }
        let entry = registry
            .catalog_entries
            .iter()
            .find(|entry| {
                call.definition_ref.as_ref().is_some_and(|r| {
                    entry.tool_id == r.tool_id && entry.tool_version == r.tool_version
                })
            })
            .ok_or(Error::Conflict)?;
        names.push(entry.canonical_name.as_str().to_string());
    }
    if names.iter().any(|item| item == "mode.disposition") && batch.calls.len() != 1 {
        return Ok(true);
    }
    let required = super::mode::load(tx, batch.invocation_identity.session_id)?
        .is_some_and(|episode| episode.disposition_required && episode.is_active_special());
    Ok(required && !matches!(name, "mode.disposition" | "query.user_question"))
}

fn clear_satisfied_lineage(
    tx: &Transaction<'_>,
    turn: TurnTaskId,
    arguments: &Value,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let Ok(tool) = serde_json::from_value::<TaskLineageReconcileToolInput>(arguments.clone())
    else {
        return Ok(());
    };
    let needles: &[&str] = match tool.decision {
        TaskLineageReconcileToolDecision::ExpirePrior { .. } => {
            &["expire_prior forbids relations", "expire_prior forbids completion_policy_update"]
        }
        TaskLineageReconcileToolDecision::CancelExisting => &[
            "cancel_existing requires a later excerpt",
            "cancel_existing requires a cancels_task excerpt",
        ],
        TaskLineageReconcileToolDecision::ReferencePrior { .. } => {
            &["reference_prior forbids completion_policy_update"]
        }
        _ => &[],
    };
    for needle in needles {
        super::guard::clear_matching(tx, turn, needle, at)?;
    }
    Ok(())
}

fn bound_reconcile(
    tx: &Transaction<'_>,
    turn: &TurnTask,
    arguments: &Value,
) -> StoreResult<TaskLineageReconcileInput> {
    let tool: TaskLineageReconcileToolInput = input(arguments)?;
    let current = session::load(tx, turn.session_id)?;
    let current_user_event_id =
        current.pending_user_event_ids.last().copied().unwrap_or(turn.root_user_event_id);
    let expected_lineage_revision = match &tool.decision {
        TaskLineageReconcileToolDecision::ExpirePrior { lineage_id } => {
            Some(lineage::load(tx, *lineage_id)?.revision)
        }
        _ => current
            .active_lineage
            .map(|id| lineage::load(tx, id).map(|l| l.revision))
            .transpose()?,
    };
    Ok(TaskLineageReconcileInput {
        current_user_event_id,
        active_lineage_id: current.active_lineage,
        expected_lineage_revision,
        decision: tool.decision.into(),
        relations: tool.relations,
        completion_policy_update: tool.completion_policy_update,
        reason: tool.reason,
    })
}

fn classify_invalid(
    message: &'static str,
) -> Option<(ModelSafeToolErrorV1, Option<DeterministicFailure>)> {
    let failure = lineage_failure(message);
    let (code, category) = match message {
        "task_lineage.arguments_invalid"
        | "task_lineage.constraint.expire_forbids_policy"
        | "task_lineage.constraint.expire_forbids_relations"
        | "task_lineage.constraint.reference_forbids_policy"
        | "task_lineage.constraint.cancel_requires_excerpt" => {
            (ToolSchemaErrorCode::ArgumentsInvalid.error_code(), ErrorCategory::Validation)
        }
        "task_lineage.identity_mismatch" | "task_lineage.target_not_in_session" => (
            ErrorCode::new("tool.batch.identity_mismatch").expect("registered syntax"),
            ErrorCategory::Validation,
        ),
        "task_lineage.revision_stale" => (
            ErrorCode::new("tool.batch.control_state_mismatch").expect("registered syntax"),
            ErrorCategory::Conflict,
        ),
        "task_lineage.disposition_not_allowed"
        | "task_lineage.current_required"
        | "agent.mode_disposition_required"
        | "agent.mode_disposition_reason_required" => (
            ErrorCode::new("task_lineage.disposition_not_allowed").expect("registered syntax"),
            ErrorCategory::Validation,
        ),
        other
            if other.starts_with("tools.")
                || other.starts_with("task_lineage.")
                || other.starts_with("skills.")
                || other.starts_with("shell.")
                || other.starts_with("context.") =>
        {
            (ToolSchemaErrorCode::ArgumentsInvalid.error_code(), ErrorCategory::Validation)
        }
        _ => return None,
    };
    let text = failure.as_ref().map(DeterministicFailure::model_message).unwrap_or_else(|| {
        format!("{message}. retryable:false means the identical arguments will fail; corrected arguments may be submitted.")
    });
    Some((ModelSafeToolErrorV1 { code, category, message: text, retryable: false }, failure))
}

fn lineage_failure(message: &str) -> Option<DeterministicFailure> {
    Some(match message {
        "task_lineage.constraint.expire_forbids_policy" => DeterministicFailure::schema_constraint(
            "task_lineage.reconcile.completion_policy_update",
            "expire_prior forbids completion_policy_update",
            "completion_policy_update set",
            "Omit completion_policy_update when marking a prior lineage expired.",
        ),
        "task_lineage.constraint.expire_forbids_relations" => {
            DeterministicFailure::schema_constraint(
                "task_lineage.reconcile.relations",
                "expire_prior forbids relations",
                "relations not empty",
                "Omit relations when marking a prior lineage expired.",
            )
        }
        "task_lineage.constraint.reference_forbids_policy" => {
            DeterministicFailure::schema_constraint(
                "task_lineage.reconcile.completion_policy_update",
                "reference_prior forbids completion_policy_update",
                "completion_policy_update set",
                "Omit completion_policy_update when only referencing a prior lineage.",
            )
        }
        "task_lineage.constraint.cancel_requires_excerpt" => {
            DeterministicFailure::schema_constraint(
                "task_lineage.reconcile.relations",
                "cancel_existing requires a later excerpt with kind cancels_task",
                "missing cancels_task excerpt",
                "Supply a later_excerpt on a cancels_task relation grounded in the current user message.",
            )
        }
        "task_lineage.arguments_invalid" => DeterministicFailure::schema_constraint(
            "task_lineage.reconcile",
            "arguments must satisfy the published combination rules",
            "invalid combination",
            "Resubmit only the decision, optional grounded relations, and reason. Core binds identity and revision.",
        ),
        "task_lineage.disposition_not_allowed" => DeterministicFailure::disposition(
            "task_lineage.reconcile.decision",
            "interrupted and topic-switched lineages cannot become Active; expire or reference them from the current task",
            "disallowed reactivation or target",
            "Keep the current Active lineage. Use expire_prior or reference_prior for background records.",
        ),
        "task_lineage.identity_mismatch" | "task_lineage.target_not_in_session" => {
            DeterministicFailure::disposition(
                "task_lineage.reconcile",
                "target must belong to this session and the current user event",
                message,
                "Reload task_lineage.list_open and use an id from this session.",
            )
        }
        "task_lineage.revision_stale" => DeterministicFailure::disposition(
            "task_lineage.reconcile",
            "the bound lineage revision must match the live record",
            "stale revision",
            "Reload current task state and resubmit. Do not copy a previous revision.",
        ),
        _ => return None,
    })
}
