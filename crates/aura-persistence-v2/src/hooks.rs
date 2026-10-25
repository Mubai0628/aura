//! One record per Hook call, and the decision of a pre-event kept before it takes
//! effect, so a replayed admission or a restart never runs the same Hooks again.
use aura_domain_common::{TurnTaskId, UtcTimestamp};
use aura_hooks::{
    HookEvent, HookInvocationListRequest, HookInvocationView, HookOutcome, HookTurnStatistics,
};
use aura_persistence_core::{StoreResult, record};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

/// The newest records are kept; older ones are removed as new ones arrive.
const KEEP_RECORDS: i64 = 20_000;
/// Input and output excerpts are cut at this many bytes (on a character boundary).
pub const MAX_STORED_TEXT: usize = 64 * 1024;

pub struct HookInvocationRecord<'a> {
    pub view: &'a HookInvocationView,
    /// What the decision applies to (a tool call, a model step), for replay lookups.
    pub subject: Option<&'a str>,
    pub input: &'a str,
    pub output: &'a str,
}

fn outcome_name(outcome: HookOutcome) -> &'static str {
    match outcome {
        HookOutcome::Continued => "continued",
        HookOutcome::Transformed => "transformed",
        HookOutcome::Blocked => "blocked",
        HookOutcome::Failed => "failed",
    }
}

fn bounded(text: &str) -> &str {
    if text.len() <= MAX_STORED_TEXT {
        return text;
    }
    let mut end = MAX_STORED_TEXT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

pub fn record(tx: &Transaction<'_>, entry: &HookInvocationRecord<'_>) -> StoreResult<()> {
    let view = entry.view;
    tx.execute(
        "INSERT INTO hook_invocations(id,event,hook_path,turn_task_id,subject,started_at,outcome,view,input,output) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            view.invocation_id.to_string(),
            view.event.name(),
            view.path,
            view.turn_task_id.map(|turn| turn.to_string()),
            entry.subject,
            view.started_at.to_string(),
            outcome_name(view.outcome),
            record::encode(view)?,
            bounded(entry.input),
            bounded(entry.output),
        ],
    )?;
    tx.execute(
        "DELETE FROM hook_invocations WHERE rowid IN (SELECT rowid FROM hook_invocations ORDER BY rowid DESC LIMIT -1 OFFSET ?1)",
        [KEEP_RECORDS],
    )?;
    Ok(())
}

/// Newest first, for one Hook, one turn, or all.
pub fn list(
    db: &Connection,
    request: &HookInvocationListRequest,
) -> StoreResult<Vec<HookInvocationView>> {
    let limit = i64::from(request.limit.clamp(1, 500));
    let mut query = db.prepare(
        "SELECT view FROM hook_invocations WHERE (?1 IS NULL OR hook_path=?1) AND (?2 IS NULL OR turn_task_id=?2) ORDER BY rowid DESC LIMIT ?3",
    )?;
    query
        .query_map(
            params![request.path, request.turn_task_id.map(|turn| turn.to_string()), limit],
            |row| row.get::<_, String>(0),
        )?
        .map(|row| record::decode(&row?))
        .collect()
}

pub fn turn_statistics(db: &Connection, turn: TurnTaskId) -> StoreResult<HookTurnStatistics> {
    let mut statistics = HookTurnStatistics::default();
    let mut query = db.prepare(
        "SELECT outcome, count(*) FROM hook_invocations WHERE turn_task_id=?1 GROUP BY outcome",
    )?;
    for row in query.query_map([turn.to_string()], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
    })? {
        let (outcome, count) = row?;
        statistics.calls += count;
        match outcome.as_str() {
            "failed" => statistics.failed += count,
            "blocked" => statistics.blocked += count,
            _ => statistics.succeeded += count,
        }
    }
    Ok(statistics)
}

/// The combined decision of one pre-event occurrence, written before it takes effect.
pub fn save_decision(
    tx: &Transaction<'_>,
    subject: &str,
    event: HookEvent,
    decision: &serde_json::Value,
    at: UtcTimestamp,
) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO hook_decisions(subject,event,decision,decided_at) VALUES(?1,?2,?3,?4)",
        params![subject, event.name(), record::encode(decision)?, at.to_string()],
    )?;
    Ok(())
}

pub fn decision(
    db: &Connection,
    subject: &str,
    event: HookEvent,
) -> StoreResult<Option<serde_json::Value>> {
    db.query_row(
        "SELECT decision FROM hook_decisions WHERE subject=?1 AND event=?2",
        params![subject, event.name()],
        |row| row.get::<_, String>(0),
    )
    .optional()?
    .map(|text| record::decode(&text))
    .transpose()
}

/// The combined `tool.before_call` decision for one call, kept before admission so a
/// replayed admission, the history projection and a restart all see the same call.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolCallHookDecision {
    Continue,
    /// The arguments the Hooks left; admission checks them from the start.
    Arguments {
        arguments: serde_json::Value,
    },
    Blocked {
        hook: String,
        reason: String,
    },
}

pub fn save_tool_call_decision(
    tx: &Transaction<'_>,
    call: aura_domain_common::ToolCallId,
    decision: &ToolCallHookDecision,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let value = serde_json::to_value(decision)?;
    save_decision(tx, &call.to_string(), HookEvent::ToolBeforeCall, &value, at)
}

pub fn tool_call_decision(
    db: &Connection,
    call: aura_domain_common::ToolCallId,
) -> StoreResult<Option<ToolCallHookDecision>> {
    decision(db, &call.to_string(), HookEvent::ToolBeforeCall)?
        .map(|value| serde_json::from_value(value).map_err(Into::into))
        .transpose()
}

/// Applies the kept decisions: a call whose Hooks changed its arguments gets those
/// arguments (and their hash); the blocked calls are returned with the Hook and reason.
pub fn apply_tool_call_decisions(
    db: &Connection,
    calls: &mut [aura_tool_contract::NormalizedToolCallV1],
) -> StoreResult<std::collections::BTreeMap<aura_domain_common::ToolCallId, (String, String)>> {
    use aura_tool_contract::NormalizedToolCallInputStateV1 as Input;
    let mut blocked = std::collections::BTreeMap::new();
    for call in calls.iter_mut() {
        match tool_call_decision(db, call.tool_call_id)? {
            Some(ToolCallHookDecision::Arguments { arguments }) => {
                if let Input::Ready { purpose, .. } = &call.input {
                    let arguments_hash = aura_domain_common::jcs::canonical_sha256(&arguments)
                        .map_err(|_| aura_persistence_core::PersistenceCoreError::Conflict)?;
                    call.input =
                        Input::Ready { purpose: purpose.clone(), arguments, arguments_hash };
                }
            }
            Some(ToolCallHookDecision::Blocked { hook, reason }) => {
                blocked.insert(call.tool_call_id, (hook, reason));
            }
            Some(ToolCallHookDecision::Continue) | None => {}
        }
    }
    Ok(blocked)
}

/// One call the model asked for that still needs its `tool.before_call` decision.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingToolCall {
    pub tool_call_id: aura_domain_common::ToolCallId,
    pub tool_name: String,
    pub arguments: serde_json::Value,
}

/// The step's calls awaiting `tool.before_call` Hooks, in order: none once the batch is
/// admitted, none whose decision is already kept, and none the model sent malformed
/// (admission refuses those itself).
pub fn calls_awaiting_hooks(
    db: &Connection,
    step_id: aura_domain_common::ModelStepId,
) -> StoreResult<Vec<PendingToolCall>> {
    use aura_persistence_core::PersistenceCoreError as Error;
    let step = crate::agent::turns::step(db, step_id)?;
    let aura_agent_domain::ModelStepState::Completed { tool_batch_id: Some(batch_id), .. } =
        step.state
    else {
        return Ok(Vec::new());
    };
    if crate::tool::admission::load(db, batch_id)?.is_some() {
        return Ok(Vec::new());
    }
    let output = crate::agent::history::output(db, step_id)?;
    let snapshot = crate::model::step_snapshot(db, step_id)?;
    let evidence = crate::model::request_evidence(db, snapshot.id)?.ok_or(Error::Conflict)?;
    let boundary = crate::agent::checkpoint::load(db, step_id)?;
    let registry =
        crate::tool::registry(db, boundary.registry_snapshot_ref.tool_registry_snapshot_id)?
            .ok_or(Error::Conflict)?;
    let mut calls =
        aura_model_domain::derive_p07_normalized_tool_calls_v1(&snapshot, &evidence, &output)
            .map_err(|_| Error::Conflict)?;
    // A call made through `tools.call` is offered as the tool it runs, with that tool's
    // own arguments; one that names nothing runnable never runs, so no Hook sees it.
    crate::tool::entry::expand(db, &evidence, &mut calls)?;
    let mut pending = Vec::new();
    for call in calls {
        if aura_tool_contract::is_entry_call(&registry, &call) {
            continue;
        }
        let aura_tool_contract::NormalizedToolCallInputStateV1::Ready { arguments, .. } =
            call.input
        else {
            continue;
        };
        let Some(definition) = call.definition_ref else { continue };
        let Some(tool_name) = crate::tool::entry::catalog_name(&registry, definition.tool_id)
        else {
            continue;
        };
        if tool_call_decision(db, call.tool_call_id)?.is_some() {
            continue;
        }
        pending.push(PendingToolCall { tool_call_id: call.tool_call_id, tool_name, arguments });
    }
    Ok(pending)
}

/// One finished call of a batch, as the model will see its result.
#[derive(Clone, Debug, PartialEq)]
pub struct CompletedToolCall {
    pub tool_call_id: aura_domain_common::ToolCallId,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub succeeded: bool,
    /// The exact model-visible result text.
    pub content: String,
}

/// The event a finished call is offered to: `tool.after_call` when it succeeded,
/// `tool.on_error` otherwise.
pub fn result_event(succeeded: bool) -> HookEvent {
    if succeeded { HookEvent::ToolAfterCall } else { HookEvent::ToolOnError }
}

/// The batch's finished calls that have a model-visible result and no kept post-event
/// decision yet, in order.
pub fn completed_tool_calls(
    db: &Connection,
    batch_id: aura_domain_common::ToolBatchId,
) -> StoreResult<Vec<CompletedToolCall>> {
    use aura_persistence_core::PersistenceCoreError as Error;
    let batch = crate::tool::execution::batch(db, batch_id)?;
    let Some(terminal) = batch.result.as_ref() else { return Ok(Vec::new()) };
    let registry =
        crate::tool::registry(db, batch.batch.registry_snapshot_ref.tool_registry_snapshot_id)?
            .ok_or(Error::Conflict)?;
    let owner = batch.batch.invocation_identity.agent_run_id.to_string();
    let mut completed = Vec::new();
    for (call, result) in batch.batch.calls.iter().zip(&terminal.results) {
        let Some(reference) = result.model_visible_result_ref else { continue };
        let succeeded =
            matches!(result.outcome, aura_tool_contract::ToolResultOutcomeV1::Succeeded);
        if decision(db, &call.tool_call_id.to_string(), result_event(succeeded))?.is_some() {
            continue;
        }
        // Named by the catalog, so a call made through `tools.call` is the tool it ran.
        let tool_name = call
            .definition_ref
            .as_ref()
            .and_then(|definition| crate::tool::entry::catalog_name(&registry, definition.tool_id))
            .unwrap_or_default();
        let arguments = match &call.input {
            aura_tool_contract::NormalizedToolCallInputStateV1::Ready { arguments, .. } => {
                arguments.clone()
            }
            _ => serde_json::Value::Null,
        };
        let raw = aura_persistence_core::artifact::read(db, reference, &owner)?;
        completed.push(CompletedToolCall {
            tool_call_id: call.tool_call_id,
            tool_name,
            arguments,
            succeeded,
            content: String::from_utf8(raw).map_err(|_| Error::Conflict)?,
        });
    }
    Ok(completed)
}

/// Keeps what a post-event Hook left: `Some(text)` replaces what the model sees.
pub fn save_result_decision(
    tx: &Transaction<'_>,
    call: aura_domain_common::ToolCallId,
    succeeded: bool,
    text: Option<&str>,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let value = serde_json::json!({ "text": text });
    save_decision(tx, &call.to_string(), result_event(succeeded), &value, at)
}

/// The model-visible text a `tool.after_call` or `tool.on_error` Hook put in place, if any.
pub fn result_override(
    db: &Connection,
    call: aura_domain_common::ToolCallId,
    succeeded: bool,
) -> StoreResult<Option<String>> {
    Ok(decision(db, &call.to_string(), result_event(succeeded))?
        .and_then(|value| value.get("text").and_then(|text| text.as_str().map(str::to_owned))))
}

/// The calls as admitted: what the model asked for, with each `tools.call` rewritten into the
/// tool it names and the kept `tool.before_call` decisions applied. Every check that
/// compares a batch against its model output uses this.
pub fn admitted_calls(
    db: &Connection,
    snapshot: &aura_model_domain::ModelExecutionSnapshot,
    evidence: &aura_model_domain::ProviderRequestEvidence,
    output: &aura_model_domain::NormalizedModelOutput,
) -> StoreResult<Vec<aura_tool_contract::NormalizedToolCallV1>> {
    let mut calls =
        aura_model_domain::derive_p07_normalized_tool_calls_v1(snapshot, evidence, output)
            .map_err(|_| aura_persistence_core::PersistenceCoreError::Conflict)?;
    crate::tool::entry::expand(db, evidence, &mut calls)?;
    apply_tool_call_decisions(db, &mut calls)?;
    Ok(calls)
}

/// `session.resumed` rather than `session.started`: the session ran a turn before this one.
pub fn session_had_earlier_turns(
    db: &Connection,
    session: aura_domain_common::SessionId,
    current: TurnTaskId,
) -> StoreResult<bool> {
    let count: i64 = db.query_row(
        "SELECT count(*) FROM turn_tasks WHERE session_id=?1 AND id<>?2",
        params![session.to_string(), current.to_string()],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// The visible text of a step that led to tools, for `model.after_call`, unless its decision
/// is already kept. A final answer is the user's answer and is never offered.
pub fn step_text_awaiting_hooks(
    db: &Connection,
    step_id: aura_domain_common::ModelStepId,
) -> StoreResult<Option<String>> {
    let step = crate::agent::turns::step(db, step_id)?;
    let aura_agent_domain::ModelStepState::Completed { tool_batch_id: Some(_), .. } = step.state
    else {
        return Ok(None);
    };
    if decision(db, &step_id.to_string(), HookEvent::ModelAfterCall)?.is_some() {
        return Ok(None);
    }
    let output = crate::agent::history::output(db, step_id)?;
    Ok(Some(crate::agent::history::visible_text(&output)))
}

/// The model-visible text a `model.after_call` Hook put in place for later steps, if any.
pub fn step_text_override(
    db: &Connection,
    step_id: aura_domain_common::ModelStepId,
) -> StoreResult<Option<String>> {
    Ok(decision(db, &step_id.to_string(), HookEvent::ModelAfterCall)?
        .and_then(|value| value.get("text").and_then(|text| text.as_str().map(str::to_owned))))
}

/// Replaces a kept decision (a queued message whose text the user edited).
pub fn replace_decision(
    tx: &Transaction<'_>,
    subject: &str,
    event: HookEvent,
    decision: Option<&serde_json::Value>,
    at: UtcTimestamp,
) -> StoreResult<()> {
    tx.execute(
        "DELETE FROM hook_decisions WHERE subject=?1 AND event=?2",
        params![subject, event.name()],
    )?;
    match decision {
        Some(value) => save_decision(tx, subject, event, value, at),
        None => Ok(()),
    }
}

/// The `message.before_accept` rewrites of this turn's messages, as notes the model sees
/// right after the original text. TaskLineage keeps requiring the original itself.
pub fn message_notes_for_turn(
    db: &Connection,
    turn: &aura_agent_domain::TurnTask,
) -> StoreResult<Vec<String>> {
    let mut ids = vec![turn.root_user_event_id];
    for group in crate::agent::inbox::consumed(db, turn.turn_task_id)? {
        ids.extend(group.user_event_ids.into_iter().filter(|id| *id != turn.root_user_event_id));
    }
    let mut notes = Vec::new();
    for id in ids {
        if let Some(value) = decision(db, &id.to_string(), HookEvent::MessageBeforeAccept)?
            && let Some(text) = value.get("text").and_then(|text| text.as_str())
        {
            let hooks = value.get("hooks").and_then(|h| h.as_str()).unwrap_or_default();
            notes.push(format!(
                "From Hook {hooks} (message.before_accept): the user's Hook rewrote the user message above. Follow this rewritten version:\n{text}"
            ));
        }
    }
    Ok(notes)
}

/// Keeps an answer that would end its turn until `turn.before_complete` is asked.
pub fn hold_completion(
    tx: &Transaction<'_>,
    step: aura_domain_common::ModelStepId,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let run = crate::agent::turns::step(tx, step)?.agent_run_id;
    let turn = crate::agent::turns::load_run(tx, run)?.turn_task_id;
    let value = serde_json::json!({"kind": "pending", "turn": turn.to_string()});
    save_decision(tx, &step.to_string(), HookEvent::TurnBeforeComplete, &value, at)
}

/// The run's held answer, with its text, if one waits for `turn.before_complete`.
pub fn held_completion(
    db: &Connection,
    run: aura_domain_common::AgentRunId,
) -> StoreResult<Option<(aura_domain_common::ModelStepId, String)>> {
    let mut query = db.prepare(
        "SELECT subject FROM hook_decisions WHERE event='turn.before_complete' AND json_extract(decision,'$.kind')='pending' ORDER BY decided_at DESC",
    )?;
    let subjects =
        query.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    for subject in subjects {
        let Ok(step) = subject.parse::<aura_domain_common::ModelStepId>() else { continue };
        if crate::agent::turns::step(db, step)?.agent_run_id != run {
            continue;
        }
        let (text, _, _) = crate::agent::desktop_messages::sealed_text(db, step)?;
        return Ok(Some((step, text)));
    }
    Ok(None)
}

/// Settles a held answer: allowed, it ends the turn now if it still may (true when it did);
/// blocked, the turn goes on with the Hook's reason as guidance.
pub fn resolve_completion(
    tx: &Transaction<'_>,
    step: aura_domain_common::ModelStepId,
    block: Option<(&str, &str)>,
    at: UtcTimestamp,
) -> StoreResult<bool> {
    let run = crate::agent::turns::step(tx, step)?.agent_run_id;
    let turn = crate::agent::turns::load_run(tx, run)?.turn_task_id;
    let value = match block {
        Some((hook, reason)) => serde_json::json!({
            "kind": "blocked", "turn": turn.to_string(), "hook": hook, "reason": reason
        }),
        None => serde_json::json!({"kind": "continue", "turn": turn.to_string()}),
    };
    replace_decision(tx, &step.to_string(), HookEvent::TurnBeforeComplete, Some(&value), at)?;
    if block.is_some() {
        // The Hook asked for more work; the protocol reminder starts over for it.
        crate::agent::guard::reset_protocol(tx, turn, at)?;
        return Ok(false);
    }
    let output = crate::agent::history::output(tx, step)?;
    Ok(crate::agent::completion::natural_from_response(tx, &output, at)?.is_some())
}

/// This turn's `turn.before_complete` blocks, oldest first, as (Hook, reason).
pub fn completion_blocks(db: &Connection, turn: TurnTaskId) -> StoreResult<Vec<(String, String)>> {
    let mut query = db.prepare(
        "SELECT json_extract(decision,'$.hook'), json_extract(decision,'$.reason') FROM hook_decisions WHERE event='turn.before_complete' AND json_extract(decision,'$.kind')='blocked' AND json_extract(decision,'$.turn')=?1 ORDER BY decided_at",
    )?;
    query
        .query_map([turn.to_string()], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}
