//! Completion control groups are one SQLite unit: real domain effect, execution
//! receipt, paired Tool results, P06 history and terminal projection. Nothing
//! below runs a Provider, command, or claims a synthetic execution succeeded.
use super::{checkpoint, completion, history, lineage, session, tool_state, turns, work};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_execution_contract::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use aura_tool_contract::*;
use rusqlite::{Connection, Transaction};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub struct PreparedControl {
    batch: ToolBatchRequestV1,
    before: Value,
    at: UtcTimestamp,
    outputs: BTreeMap<ToolCallId, Value>,
    review: Option<(TaskLineage, TaskLineage, CompletionReview)>,
    completion: Finish,
}
enum Finish {
    None,
    Normal {
        call: ToolCallId,
        input: NormalCompleteInput,
        record: NormalCompletionRecord,
        same_batch: bool,
    },
    Plan {
        call: ToolCallId,
        input: PlanCompleteInput,
        record: PlanRecord,
    },
    Goal {
        call: ToolCallId,
        input: GoalCompleteInput,
        state: GoalState,
    },
}
impl PreparedControl {
    pub fn executed_call_ids(&self) -> impl Iterator<Item = ToolCallId> + '_ {
        self.outputs.keys().copied()
    }
    fn stamp_commit(&mut self, at: UtcTimestamp) -> StoreResult<()> {
        if at < self.at {
            return Err(Error::Conflict);
        }
        if let Some((_, next, review)) = &mut self.review {
            let old = serde_json::to_value(&*review)?;
            next.updated_at = at;
            review.created_at = at;
            let mut found = 0;
            for output in self.outputs.values_mut() {
                if *output == old {
                    *output = serde_json::to_value(&*review)?;
                    found += 1;
                }
            }
            if found != 1 {
                return Err(Error::Conflict);
            }
        }
        match &mut self.completion {
            Finish::Normal { call, record, .. } => {
                record.at = at;
                self.outputs.insert(*call, serde_json::to_value(record)?);
            }
            Finish::Plan { call, record, .. } => {
                record.at = at;
                self.outputs.insert(*call, serde_json::to_value(record)?);
            }
            Finish::None | Finish::Goal { .. } => {}
        }
        self.at = at;
        Ok(())
    }
}
pub fn prepare(
    db: &Connection,
    batch: &ToolBatchRequestV1,
    at: UtcTimestamp,
) -> StoreResult<PreparedControl> {
    let id = &batch.invocation_identity;
    let turn = turns::load(db, id.turn_task_id)?;
    let run = turns::load_run(db, id.agent_run_id)?;
    if run.parent_agent_run_id.is_some() || turn.lineage_resolution_required {
        return Err(Error::Conflict);
    }
    let registry =
        crate::tool::registry(db, batch.registry_snapshot_ref.tool_registry_snapshot_id)?
            .ok_or(Error::Conflict)?;
    let mut inputs = BTreeMap::new();
    for call in &batch.calls {
        checkpoint::verify_live_call(db, batch, call.tool_call_id)?;
        if crate::tool::execution::call(db, call.tool_call_id)?.attempt_id.is_some() {
            return Err(Error::Conflict);
        }
        let entry = registry
            .catalog_entries
            .iter()
            .find(|v| {
                call.definition_ref
                    .as_ref()
                    .is_some_and(|r| v.tool_id == r.tool_id && v.tool_version == r.tool_version)
            })
            .ok_or(Error::Conflict)?;
        let NormalizedToolCallInputStateV1::Ready { arguments, .. } = &call.input else {
            return Err(Error::Conflict);
        };
        if inputs.insert(entry.canonical_name.as_str(), (call.tool_call_id, arguments)).is_some() {
            return Err(Error::Conflict);
        }
    }
    let names: std::collections::BTreeSet<_> = inputs.keys().copied().collect();
    if !(names.len() == 1
        && names.iter().all(|n| {
            matches!(
                *n,
                "task_lineage.review_completion"
                    | "normal.complete"
                    | "plan.complete"
                    | "goal.complete"
            )
        }))
        && names != ["task_lineage.review_completion", "normal.complete"].into()
    {
        return Err(Error::Conflict);
    }
    let original = lineage::load(db, turn.lineage_id)?;
    let mut selected = original.clone();
    let context = CompletionContext { turn: &turn, run: &run, model_step_id: id.model_step_id, at };
    let mut result = PreparedControl {
        batch: batch.clone(),
        before: tool_state::inspect(db, batch)?,
        at,
        outputs: BTreeMap::new(),
        review: None,
        completion: Finish::None,
    };
    if let Some((call, args)) = inputs.get("task_lineage.review_completion") {
        let input: TaskLineageReviewCompletionInput = arguments(args)?;
        let receipt = input
            .source_read_receipt_id
            .map(|r| lineage::receipt(db, selected.lineage_id, r))
            .transpose()?;
        super::evidence::verify(
            db,
            input
                .evidence_refs
                .iter()
                .chain(input.reviewed_user_intents.iter().flat_map(|v| &v.evidence_refs)),
            &id.agent_run_id.to_string(),
        )?;
        let review = selected
            .review_completion(input, receipt.as_ref(), CompletionReviewId::new(), &context)
            .map_err(|_| Error::Invalid("completion review"))?;
        result.outputs.insert(*call, serde_json::to_value(&review)?);
        result.review = Some((original, selected.clone(), review));
    }
    if let Some((call, args)) = inputs.get("normal.complete") {
        let input: NormalCompleteInput = arguments(args)?;
        let same_batch = result.review.is_some();
        let mut checked = input.clone();
        if let Some((old, _, _)) = &result.review {
            if checked.expected_lineage_revision != old.revision {
                return Err(Error::Conflict);
            }
            checked.expected_lineage_revision = selected.revision;
        }
        let outcome = selected
            .check_normal_completion(&checked, &context)
            .map_err(|_| Error::Invalid("Normal completion identity"))?;
        let record = NormalCompletionRecord {
            record_id: NormalCompletionRecordId::new(),
            turn_task_id: turn.turn_task_id,
            agent_run_id: run.agent_run_id,
            lineage_id: selected.lineage_id,
            review_id: result.review.as_ref().map(|(_, _, review)| review.review_id),
            outcome,
            final_report: input.final_report.clone(),
            disclosed_limitations: input.disclosed_limitations.clone(),
            at,
        };
        result.outputs.insert(*call, serde_json::to_value(&record)?);
        result.completion = Finish::Normal { call: *call, input, record, same_batch };
    }
    if let Some((call, args)) = inputs.get("plan.complete") {
        let input: PlanCompleteInput = arguments(args)?;
        input.validate().map_err(|_| Error::Invalid("Plan completion"))?;
        if turn.mode != AgentTaskMode::Plan || turn.revision != input.expected_turn_revision {
            return Err(Error::Conflict);
        }
        let user = session::user_message(db, turn.session_id, turn.root_user_event_id)?;
        let record = PlanRecord {
            turn_task_id: turn.turn_task_id,
            revision: Revision::INITIAL,
            original_user_goal: user.text,
            plan: input.clone(),
            at,
        };
        result.outputs.insert(*call, serde_json::to_value(&record)?);
        result.completion = Finish::Plan { call: *call, input, record };
    }
    if let Some((call, args)) = inputs.get("goal.complete") {
        let input: GoalCompleteInput = arguments(args)?;
        if turn.mode != AgentTaskMode::Goal {
            return Err(Error::Conflict);
        }
        let mut state = work::goal(db, turn.turn_task_id)?;
        state.check_completion(&input).map_err(|_| Error::Invalid("Goal completion gate"))?;
        super::evidence::verify(db, &input.evidence_refs, &id.agent_run_id.to_string())?;
        state.revision =
            Revision::new(state.revision.value().checked_add(1).ok_or(Error::Conflict)?);
        state.completed = true;
        result.outputs.insert(*call, serde_json::to_value(&state)?);
        result.completion = Finish::Goal { call: *call, input, state };
    }
    Ok(result)
}
/// A completion tool's arguments in its own type. Admission checked them against the tool's
/// schema; one that still does not decode (an id in a form Aura never issues) is the model's to
/// correct, not a damaged store.
fn arguments<T: serde::de::DeserializeOwned>(args: &Value) -> StoreResult<T> {
    serde_json::from_value(args.clone()).map_err(|_| {
        Error::Invalid(
            "tool.arguments_invalid: an id in these arguments is not one Aura gave you; copy ids exactly from the task state",
        )
    })
}
pub fn commit(
    tx: &Transaction<'_>,
    mut prepared: PreparedControl,
    dispatches: &BTreeMap<ToolCallId, AuthorizedDispatch>,
    at: UtcTimestamp,
) -> StoreResult<Vec<ToolResultV1>> {
    // Preflight may wait on policy/locks. Real dispatch and terminal facts use
    // commit time, after authorization and reservations, never preflight time.
    prepared.stamp_commit(at)?;
    let batch = &prepared.batch;
    if prepared.before != tool_state::inspect(tx, batch)?
        || prepared.outputs.keys().collect::<Vec<_>>() != dispatches.keys().collect::<Vec<_>>()
    {
        return Err(Error::Conflict);
    }
    for (call_id, dispatch) in dispatches {
        checkpoint::verify_live_call(tx, batch, *call_id)?;
        let request = dispatch.request();
        if request.plan.prepared.preparation["domain"] != prepared.before {
            return Err(Error::Conflict);
        }
        crate::execution::dispatch_authority(tx, request, prepared.at)?;
        crate::execution::mark_dispatched(
            tx,
            request.operation.operation_id,
            request.binding.attempt.ordinal.get(),
            prepared.at,
        )?;
    }
    if let Some((old, next, review)) = &prepared.review {
        lineage::save(tx, old, next)?;
        lineage::save_review(tx, review)?;
    }
    for (call, output) in &prepared.outputs {
        let request = dispatches[call].request();
        let evidence = BrokerEvidence {
            action_kind: request.operation.content.action_kind.clone(),
            request_fingerprint: request.operation.fingerprint.0.clone(),
            before: prepared.before.clone(),
            after: output.clone(),
            side_effect: SideEffectStateV1::FullyApplied,
            domain_journal: json!({"schema":"agent-control-effect-v1","output":output,"atomic_batch_id":batch.tool_batch_id}),
        };
        crate::execution::broker_result::record(tx, request, &evidence, true, prepared.at)?;
        crate::tool::result::materialize(tx, batch.tool_batch_id, *call)?;
    }
    history::complete_batch(tx, batch.tool_batch_id, prepared.at)?;
    // The provisional payload is visible only if the owning completion gate
    // reproduces it exactly and all actual terminal state commits with it.
    let step = batch.invocation_identity.model_step_id;
    match prepared.completion {
        Finish::None => {}
        Finish::Normal { call, input, record, same_batch } => {
            let actual = completion::normal_with_id(
                tx,
                step,
                batch.tool_batch_id,
                &input,
                same_batch,
                record.record_id,
                prepared.at,
            )?;
            if actual != record || serde_json::to_value(&actual)? != prepared.outputs[&call] {
                return Err(Error::Conflict);
            }
        }
        Finish::Plan { call, input, record } => {
            let actual = completion::plan(tx, step, batch.tool_batch_id, &input, prepared.at)?;
            if actual != record || serde_json::to_value(&actual)? != prepared.outputs[&call] {
                return Err(Error::Conflict);
            }
        }
        Finish::Goal { call, input, state } => {
            let actual = completion::goal(tx, step, batch.tool_batch_id, &input, prepared.at)?;
            if actual != state || serde_json::to_value(&actual)? != prepared.outputs[&call] {
                return Err(Error::Conflict);
            }
        }
    }
    batch
        .calls
        .iter()
        .map(|c| crate::tool::execution::call(tx, c.tool_call_id)?.result.ok_or(Error::Conflict))
        .collect()
}
