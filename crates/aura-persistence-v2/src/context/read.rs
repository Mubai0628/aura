//! Model-readable V2 history, with opaque request-bound pagination. Raw
//! Provider responses, reasoning, credentials and internal diagnostics are
//! never selected. A Tool response group is returned whole or not at all: an event too long for
//! the page comes with its length and no text, and a part of it only when asked for by offset.
use aura_context_domain::*;
use aura_domain_common::{
    jcs::{CanonicalHashV1, canonical_sha256},
    *,
};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact, event, record};
use aura_tool_contract::{NormalizedToolCallInputStateV1, ToolResultOutcomeV1};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::json;

const HARD_CHARS: usize = 262_144;
const RESULT_PAGE: usize = 65_536;
#[derive(Deserialize, Serialize)]
struct Cursor {
    operation: String,
    request: CanonicalHashV1,
    cutoff: i64,
    position: usize,
}
fn scope(db: &Connection, session: SessionId, run: AgentRunId) -> StoreResult<()> {
    let run = crate::agent::turns::load_run(db, run)?;
    if run.parent_agent_run_id.is_some()
        || crate::agent::turns::load(db, run.turn_task_id)?.session_id != session
    {
        return Err(Error::Conflict);
    }
    Ok(())
}
pub fn compaction(
    db: &Connection,
    session: SessionId,
    run: AgentRunId,
    input: &AgentContextReadCompactionInput,
) -> StoreResult<ContextReadCompactionResult> {
    scope(db, session, run)?;
    // A branch reads its own copy of a compaction it carried, by the identity the model
    // was told in the summary.
    let id =
        crate::agent::branch::carried_compaction(db, session, input.source_context_snapshot_id)?
            .unwrap_or(input.source_context_snapshot_id);
    let value = super::compaction::load(db, id)?;
    if value.session_id != session || value.sequence != input.sequence || value.state != "completed"
    {
        return Err(Error::Conflict);
    }
    let result = value.result.ok_or(Error::Conflict)?;
    let bytes = artifact::read(
        db,
        value.result_artifact_id.ok_or(Error::Conflict)?,
        &value.id.to_string(),
    )?;
    if serde_json::from_slice::<ContextReadCompactionResult>(&bytes)? != result {
        return Err(Error::Conflict);
    }
    Ok(result)
}
fn cursor(
    db: &Connection,
    run: AgentRunId,
    token: Option<&str>,
    operation: &str,
    request: CanonicalHashV1,
) -> StoreResult<Cursor> {
    if let Some(token) = token {
        if token.len() > 4096 {
            return Err(Error::Invalid("context cursor too long"));
        }
        let raw:String=db.query_row("SELECT payload FROM events WHERE owner=?1 AND kind='context.read_cursor' AND event_id=?2 AND version=1",params![run.to_string(),token],|r|r.get(0))?;
        let value: Cursor = record::decode(&raw)?;
        if value.operation != operation || value.request != request || value.cutoff < 0 {
            return Err(Error::Conflict);
        }
        Ok(value)
    } else {
        Ok(Cursor {
            operation: operation.into(),
            request,
            cutoff: db
                .query_row("SELECT COALESCE(MAX(sequence),0) FROM events", [], |r| r.get(0))?,
            position: 0,
        })
    }
}
fn save_cursor(
    tx: &Transaction<'_>,
    run: AgentRunId,
    value: &Cursor,
    at: UtcTimestamp,
) -> StoreResult<String> {
    let id = EventId::new();
    event::append(
        tx,
        &event::EventDraft {
            id,
            owner: &run.to_string(),
            kind: "context.read_cursor",
            version: 1,
            payload: &serde_json::to_value(value)?,
            at,
        },
        "context",
    )?;
    Ok(id.to_string())
}
pub fn events(
    tx: &Transaction<'_>,
    session: SessionId,
    run: AgentRunId,
    input: &AgentContextReadEventsInput,
    at: UtcTimestamp,
) -> StoreResult<ContextReadEventsResult> {
    scope(tx, session, run)?;
    if input.selectors.is_empty()
        || input.selectors.len() > 32
        || input.character_limit == 0
        || input.character_limit > HARD_CHARS as u64
    {
        return Err(Error::Invalid("context read limits"));
    }
    if let Some(offset) = input.content_offset {
        return part(tx, session, input, offset);
    }
    let request = canonical_sha256(&(&input.selectors, input.character_limit))
        .map_err(|_| Error::Conflict)?;
    let mut page = cursor(tx, run, input.cursor.as_deref(), "events", request)?;
    let rows = rows(tx, session, page.cutoff)?;
    // A turn a branch carried is not in this store under this session; its root user
    // message is known from the branch.
    let carried = crate::agent::branch::carried_roots(tx, session)?;
    let mut selected = std::collections::BTreeSet::new();
    for selector in &input.selectors {
        let mut found = false;
        for (index, row) in rows.iter().enumerate() {
            let matches = match selector {
                ContextEventSelector::Event { event_id } => row.view.event_id == *event_id,
                ContextEventSelector::SequenceRange { start, end } => {
                    if start > end {
                        return Err(Error::Conflict);
                    }
                    row.view.session_sequence >= *start && row.view.session_sequence <= *end
                }
                ContextEventSelector::TurnTask { turn_task_id } => {
                    row.turn == Some(*turn_task_id)
                        || if let Some(root) = carried.get(turn_task_id) {
                            *root == row.view.event_id
                        } else {
                            let turn = crate::agent::turns::load(tx, *turn_task_id)?;
                            if turn.session_id != session {
                                return Err(Error::Conflict);
                            }
                            turn.root_user_event_id == row.view.event_id
                        }
                }
                ContextEventSelector::HistoryAtomicGroup { group_ref } => {
                    row.view.history_atomic_group_ref.as_ref() == Some(group_ref)
                }
                // V2 groups are content-addressed. A legacy opaque group string
                // cannot impersonate a verified V2 group.
                ContextEventSelector::AtomicGroup { .. } => false,
            };
            if matches {
                found = true;
                selected.insert(index);
            }
        }
        if !found && !matches!(selector, ContextEventSelector::SequenceRange { .. }) {
            return Err(Error::Invalid("context source not found"));
        }
    }
    let selected = selected.into_iter().collect::<Vec<_>>();
    if page.position > selected.len() {
        return Err(Error::Conflict);
    }
    let mut events = vec![];
    let mut count = 0usize;
    let limit = input.character_limit as usize;
    for index in &selected[page.position..] {
        let row = &rows[*index];
        let chars = row.view.content.chars().count();
        // A page never holds more text than character_limit: a longer event is named with its
        // length, and read whole with a larger limit or in parts by offset.
        let shown = if chars > limit { 0 } else { chars };
        if events.len() >= 128 || (!events.is_empty() && count + shown > limit) {
            break;
        }
        let mut view = row.view.clone();
        if chars > limit {
            view.content = String::new();
            view.content_chars = Some(chars as u64);
        }
        count += shown;
        events.push(view);
        page.position += 1;
    }
    let next_cursor =
        if page.position < selected.len() { Some(save_cursor(tx, run, &page, at)?) } else { None };
    Ok(ContextReadEventsResult { events, next_cursor, character_count: count as u64 })
}
/// One event's text from `offset`, at most character_limit characters, named as a part of it. The
/// event keeps its identity and the hash of its whole text.
fn part(
    tx: &Transaction<'_>,
    session: SessionId,
    input: &AgentContextReadEventsInput,
    offset: u64,
) -> StoreResult<ContextReadEventsResult> {
    let ([ContextEventSelector::Event { event_id }], None) =
        (input.selectors.as_slice(), &input.cursor)
    else {
        return Err(Error::Invalid(
            "context.read_events.content_offset: give one event selector and no cursor",
        ));
    };
    let cutoff = tx.query_row("SELECT COALESCE(MAX(sequence),0) FROM events", [], |r| r.get(0))?;
    let mut view = rows(tx, session, cutoff)?
        .into_iter()
        .find(|row| row.view.event_id == *event_id)
        .ok_or(Error::Invalid("context source not found"))?
        .view;
    let chars = view.content.chars().count();
    let start = usize::try_from(offset).ok().filter(|start| *start <= chars).ok_or(
        Error::Invalid("context.read_events.content_offset: past the end of the event's text"),
    )?;
    let text: String =
        view.content.chars().skip(start).take(input.character_limit as usize).collect();
    let end = start + text.chars().count();
    view.content = text;
    view.content_chars = Some(chars as u64);
    view.content_offset = Some(start as u64);
    view.content_next_offset = (end < chars).then_some(end as u64);
    Ok(ContextReadEventsResult {
        events: vec![view],
        next_cursor: None,
        character_count: (end - start) as u64,
    })
}

pub(crate) struct Row {
    pub(crate) turn: Option<TurnTaskId>,
    pub(crate) view: ContextReadEventView,
}
/// A branch's carried rows come first; they were written before anything of its own.
fn rows(db: &Connection, session: SessionId, cutoff: i64) -> StoreResult<Vec<Row>> {
    let mut rows: Vec<Row> = crate::agent::branch::carried_rows(db, session)?
        .into_iter()
        .map(|row| Row { turn: row.turn, view: row.view })
        .collect();
    rows.extend(own_rows(db, session, cutoff)?);
    Ok(rows)
}
pub(crate) fn own_rows(db: &Connection, session: SessionId, cutoff: i64) -> StoreResult<Vec<Row>> {
    let groups = super::history(db, session)?;
    // The user's messages are the events of one owner and the answers those of the
    // session's top-level runs: both read through the owner index (`CROSS JOIN` keeps the runs
    // outside the events). Asked in one statement with an `OR`, they were looked for in the whole
    // journal.
    let mut query = db.prepare(
        "SELECT * FROM ( \
         SELECT e.sequence,e.event_id,e.kind,e.payload,NULL FROM events e \
         WHERE e.owner=?3 AND e.kind='agent.user_message' AND e.version=1 AND e.sequence<=?2 \
         UNION ALL \
         SELECT e.sequence,e.event_id,e.kind,e.payload,ar.turn_task_id FROM turn_tasks t \
         CROSS JOIN agent_runs ar CROSS JOIN events e \
         WHERE t.session_id=?1 AND ar.turn_task_id=t.id AND ar.parent_id IS NULL \
         AND e.owner=ar.id AND e.kind='agent.assistant_response' AND e.version=1 \
         AND e.sequence<=?2) ORDER BY 1",
    )?;
    let raw = query.query_map(
        params![session.to_string(), cutoff, format!("agent-user:{session}")],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        },
    )?;
    let mut result = vec![];
    for item in raw {
        let (sequence, event_id, kind, payload, turn) = item?;
        let sequence = u64::try_from(sequence).map_err(|_| Error::Conflict)?;
        let event_id = event_id.parse().map_err(|_| Error::Conflict)?;
        let mut group_ref = None;
        let (content, actor) = if kind == "agent.user_message" {
            let message = crate::agent::session::user_message(db, session, event_id)?;
            (message.text, "user")
        } else {
            let output: aura_model_domain::NormalizedModelOutput = record::decode(&payload)?;
            if crate::agent::history::output(db, output.model_step_id)? != output {
                return Err(Error::Conflict);
            }
            let mut tools = vec![];
            if !output.tool_calls.is_empty() {
                let Some(group) = groups.iter().find(|g| g.assistant_response_event_id == event_id)
                else {
                    continue;
                };
                let published:Option<i64>=db.query_row("SELECT sequence FROM events WHERE owner=?1 AND kind='tool.batch_completed' AND version=1",[group.tool_batch_id.to_string()],|r|r.get(0)).optional()?;
                if published.is_none_or(|v| v > cutoff) {
                    continue;
                }
                if canonical_sha256(&output).map_err(|_| Error::Conflict)?
                    != group.assistant_response_content_hash
                {
                    return Err(Error::Conflict);
                }
                for entry in &group.calls {
                    let call = crate::tool::execution::call(db, entry.tool_call_id)?;
                    let terminal = call.result.ok_or(Error::Conflict)?;
                    if canonical_sha256(&call.call).map_err(|_| Error::Conflict)?
                        != entry.tool_call_fact_hash
                        || canonical_sha256(&terminal).map_err(|_| Error::Conflict)?
                            != entry.tool_result_fact_hash
                    {
                        return Err(Error::Conflict);
                    }
                    let body = artifact::read(
                        db,
                        terminal.model_visible_result_ref.ok_or(Error::Conflict)?,
                        &entry.tool_call_id.to_string(),
                    )?;
                    tools.push(
                        serde_json::from_slice::<
                            aura_tool_contract::ModelVisibleToolResultEnvelopeV1,
                        >(&body)?
                        .model_view(),
                    );
                }
                group_ref = Some(group.group_ref().map_err(|_| Error::Conflict)?);
            }
            (
                serde_json::to_string(
                    &json!({"assistant_blocks":output.assistant_blocks,"tool_calls":output.tool_calls,"tool_results":tools,"refusal":output.refusal}),
                )?,
                "assistant",
            )
        };
        result.push(Row {
            turn: turn.map(|v| v.parse().map_err(|_| Error::Conflict)).transpose()?,
            view: ContextReadEventView {
                event_id,
                session_sequence: SessionSequence::new(sequence),
                event_type: kind,
                actor: actor.into(),
                history_atomic_group_ref: group_ref,
                atomic_group_id: None,
                legacy_atomic_group_verification: None,
                content_hash: ContentHash::blake3(content.as_bytes()),
                content,
                content_chars: None,
                content_offset: None,
                content_next_offset: None,
            },
        });
    }
    Ok(result)
}
pub fn tool_result(
    tx: &Transaction<'_>,
    session: SessionId,
    run: AgentRunId,
    input: &AgentContextReadToolResultInput,
    at: UtcTimestamp,
) -> StoreResult<ContextReadToolResultResult> {
    scope(tx, session, run)?;
    // A call of a turn a branch carried is read from what the branch kept of it.
    let whole = match crate::agent::branch::carried_tool_result(tx, session, input.tool_call_id)? {
        Some(carried) => carried,
        None => whole_tool_result(tx, session, input.tool_call_id)?,
    };
    let content = whole.result;
    let hash = ContentHash::blake3(content.as_bytes());
    let binding = canonical_sha256(&(input.tool_call_id, input.include_input, &hash))
        .map_err(|_| Error::Conflict)?;
    let mut page = cursor(tx, run, input.cursor.as_deref(), "tool_result", binding)?;
    if page.position > content.len() || !content.is_char_boundary(page.position) {
        return Err(Error::Conflict);
    }
    let end = content[page.position..]
        .char_indices()
        .nth(RESULT_PAGE)
        .map_or(content.len(), |(i, _)| page.position + i);
    let text = content[page.position..end].to_owned();
    let arguments = if input.include_input && page.position == 0 {
        let value = whole.input.ok_or(Error::Conflict)?;
        if value.chars().count() > HARD_CHARS {
            return Err(Error::Invalid("context Tool input too large"));
        }
        Some(value)
    } else {
        None
    };
    page.position = end;
    Ok(ContextReadToolResultResult {
        value: ContextReadToolResultView {
            tool_call_id: input.tool_call_id,
            tool_name: whole.tool_name,
            input: arguments,
            result: text,
            is_error: whole.is_error,
            content_hash: hash,
        },
        next_cursor: if end < content.len() {
            Some(save_cursor(tx, run, &page, at)?)
        } else {
            None
        },
    })
}

/// One tool call's whole stored result, its input and its name, as `context.read_tool_result`
/// pages them. A branch keeps this for each call of the turns it carries.
pub(crate) fn whole_tool_result(
    db: &Connection,
    session: SessionId,
    tool_call_id: ToolCallId,
) -> StoreResult<crate::agent::branch::CarriedToolResult> {
    let batch_id:String=db.query_row("SELECT c.batch_id FROM tool_calls c JOIN tool_batches b ON b.id=c.batch_id WHERE c.id=?1 AND b.session_id=?2",params![tool_call_id.to_string(),session.to_string()],|r|r.get(0))?;
    let batch = crate::tool::execution::batch(db, batch_id.parse().map_err(|_| Error::Conflict)?)?;
    let producer = crate::agent::turns::load_run(db, batch.batch.invocation_identity.agent_run_id)?;
    if producer.parent_agent_run_id.is_some() {
        return Err(Error::Conflict);
    }
    let call = crate::tool::execution::call(db, tool_call_id)?;
    let result = call.result.ok_or(Error::Conflict)?;
    let bytes = artifact::read(
        db,
        result.model_visible_result_ref.ok_or(Error::Conflict)?,
        &tool_call_id.to_string(),
    )?;
    // Read again, a result shows the model what it showed the first time.
    let content =
        serde_json::from_slice::<aura_tool_contract::ModelVisibleToolResultEnvelopeV1>(&bytes)?
            .model_text();
    let input = match &call.call.input {
        NormalizedToolCallInputStateV1::Ready { arguments, .. } => {
            serde_json::to_string(arguments)?
        }
        _ => serde_json::to_string(&call.call.input)?,
    };
    let name = if let Some(reference) = &call.call.definition_ref {
        let registry =
            crate::tool::registry(db, batch.batch.registry_snapshot_ref.tool_registry_snapshot_id)?
                .ok_or(Error::Conflict)?;
        registry
            .catalog_entries
            .iter()
            .find(|e| e.tool_id == reference.tool_id && e.tool_version == reference.tool_version)
            .ok_or(Error::Conflict)?
            .canonical_name
            .to_string()
    } else {
        crate::tool::call_name::unbound(db, &batch, &call.call)?
    };
    Ok(crate::agent::branch::CarriedToolResult {
        tool_call_id,
        tool_name: name,
        input: Some(input),
        result: content,
        is_error: !matches!(result.outcome, ToolResultOutcomeV1::Succeeded),
    })
}
