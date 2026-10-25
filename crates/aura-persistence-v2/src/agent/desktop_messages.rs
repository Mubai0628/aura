//! Bounded desktop history pages. Text is read from immutable user facts, from
//! sealed model output, or from the normalized user-visible deltas of a step
//! that is still running — never from credentials, raw evidence, partial tool
//! arguments or private diagnostics. A body that can still grow carries the
//! identity of the attempt that produced it, because sealed output and a live
//! body are different content for the same source.
use super::{history, inbox, model_context, session, turns};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use aura_projection_contract::AgentAssistantView;
use rusqlite::{Connection, OptionalExtension, params};

const PREVIEW_BYTES: usize = 2048;
const PAGE_ITEMS: usize = 10;

pub struct Messages {
    pub users: Vec<AgentUserMessageView>,
    pub assistants: Vec<AgentAssistantView>,
    pub next: Option<AgentTurnHistoryCursor>,
}

fn page_start<T: PartialEq>(ids: &[T], before: Option<&T>, initial: bool) -> StoreResult<usize> {
    match before {
        Some(id) => ids.iter().position(|v| v == id).ok_or(Error::Conflict),
        None if initial => Ok(ids.len()),
        None => Ok(0),
    }
}

pub fn page(
    db: &Connection,
    turn: &TurnTask,
    cursor: Option<&AgentTurnHistoryCursor>,
) -> StoreResult<Messages> {
    let mut user_ids = vec![turn.root_user_event_id];
    user_ids
        .extend(inbox::consumed(db, turn.turn_task_id)?.into_iter().flat_map(|g| g.user_event_ids));
    user_ids.extend(super::query::events(db, turn.turn_task_id)?);
    // The ordering pass reads only each message's small facts and its stored length. The
    // preview below fetches the text it shows; nothing assembles a message to measure it.
    let users = session::ordered_user_summaries(db, turn.session_id, user_ids)?;
    let ids: Vec<_> = users.iter().map(|u| u.event_id).collect();
    let end =
        page_start(&ids, cursor.and_then(|c| c.before_user_event_id.as_ref()), cursor.is_none())?;
    let start = end.saturating_sub(PAGE_ITEMS);
    let before_user_event_id = (start > 0).then(|| ids[start]);
    let users = users[start..end]
        .iter()
        .map(|u| -> StoreResult<AgentUserMessageView> {
            // A message with pictures or files shows what the user typed.
            let attached = super::attachments::of_message(db, u.event_id)?;
            let mut text = user_preview(db, turn.session_id, u.event_id)?;
            let total_bytes = match &attached {
                Some((typed, _)) => {
                    text.truncate(usize::try_from(*typed).unwrap_or(usize::MAX).min(text.len()));
                    *typed
                }
                None => u.total_bytes,
            };
            Ok(AgentUserMessageView {
                event_id: u.event_id,
                text,
                total_bytes,
                sealed_revision: Some(
                    session::user_message_source(db, turn.session_id, u.event_id)?
                        .content_hash
                        .to_string(),
                ),
                origin: u.origin.clone(),
                at: u.at,
                skills: super::skills::selected(db, u.event_id)?,
                attachments: attached.map(|(_, attachments)| attachments).unwrap_or_default(),
            })
        })
        .collect::<StoreResult<_>>()?;
    let steps = model_context::steps(db, turn.agent_run_id)?;
    let ids: Vec<_> = steps.iter().map(|s| s.model_step_id).collect();
    let end =
        page_start(&ids, cursor.and_then(|c| c.before_model_step_id.as_ref()), cursor.is_none())?;
    let start = end.saturating_sub(PAGE_ITEMS);
    let before_model_step_id = (start > 0).then(|| ids[start]);
    let assistants = steps[start..end]
        .iter()
        .map(|s| assistant(db, s, turn.session_id))
        .collect::<StoreResult<_>>()?;
    let next = (before_user_event_id.is_some() || before_model_step_id.is_some()).then_some(
        AgentTurnHistoryCursor {
            turn_task_id: turn.turn_task_id,
            before_user_event_id,
            before_model_step_id,
        },
    );
    Ok(Messages { users, assistants, next })
}

/// The first `PREVIEW_BYTES` of one user message, read as a window of its stored chunks.
///
/// The message payload is the authority for its text; the chunks are the read path, so a long
/// message costs its index plus the chunk the preview touches rather than its whole payload.
/// A message stored before chunks existed has none: the payload is then asked for **its first
/// bytes only**, through the same preview bound, so that path stays bounded too.
/// A page of a message with attachments, within what the user typed (the first `typed`
/// bytes of its text, which end at a character).
fn typed_page(mut page: AgentMessageTextPage, typed: Option<u64>) -> AgentMessageTextPage {
    let Some(typed) = typed else {
        return page;
    };
    let room = typed.saturating_sub(page.from_offset);
    if (page.text.len() as u64) >= room {
        page.text.truncate(usize::try_from(room).unwrap_or(usize::MAX));
        page.next_offset = None;
    }
    page.total_bytes = typed;
    page
}

fn user_preview(db: &Connection, session_id: SessionId, event_id: EventId) -> StoreResult<String> {
    let source = session::user_message_source(db, session_id, event_id)?;
    let body = history::ChunkBody::user_message(event_id, &source.content_hash);
    match history::stored_body_window(db, &body, 0, PREVIEW_BYTES as u64)? {
        Some(window) => Ok(window.text),
        // A message with no index was stored before chunks existed. Its payload is the only
        // copy, so the prefix is asked for in characters: for text that is the unit SQLite
        // counts, and UTF-8 never needs fewer bytes than characters, so this is at least the
        // preview and never more than four times it.
        None => session::user_message_prefix(db, session_id, event_id, PREVIEW_BYTES),
    }
}

pub(crate) fn sealed_text(
    db: &Connection,
    step: ModelStepId,
) -> StoreResult<(String, String, Vec<ToolCallId>)> {
    let output = history::output(db, step)?;
    Ok((
        history::visible_text(&output),
        output
            .reasoning_blocks
            .iter()
            .map(|b| b.summary.as_deref().unwrap_or(&b.text))
            .collect::<Vec<_>>()
            .join("\n"),
        output.tool_calls.iter().map(|c| c.tool_call_id).collect(),
    ))
}

/// The ring's numbers, taken from the frozen inspection without loading the
/// context snapshot or the model outputs.
/// A step's frozen context meter, whether its compaction was running, and what its preparation
/// gave back.
struct FrozenMeter {
    usage: Option<aura_context_domain::ContextUsageSummaryView>,
    compaction_running: bool,
    reduction: Option<aura_context_domain::PromptInspectorReductionView>,
}

fn frozen_context_meter(db: &Connection, step: &str) -> FrozenMeter {
    let row: Option<(Option<String>, Option<String>, Option<String>)> = db
        .query_row(
            // One inspection per step, frozen before its output streams: the first of
            // the step's events, not the last of its thousands of streamed pieces.
            "SELECT json_extract(payload, '$.context_usage'), \
             json_extract(payload, '$.compaction.state'), json_extract(payload, '$.reduction') \
             FROM events WHERE owner=?1 AND kind='agent.prompt_inspector' AND version=1 \
             ORDER BY sequence ASC LIMIT 1",
            [step],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .ok()
        .flatten();
    let Some((usage, state, reduction)) = row else {
        return FrozenMeter { usage: None, compaction_running: false, reduction: None };
    };
    FrozenMeter {
        usage: parsed(usage),
        compaction_running: state.as_deref() == Some("running"),
        reduction: parsed(reduction),
    }
}

fn parsed<T: serde::de::DeserializeOwned>(raw: Option<String>) -> Option<T> {
    raw.filter(|raw| raw != "null").and_then(|raw| serde_json::from_str(&raw).ok())
}

/// Older tool results this step's request cleared, with nothing summarized.
fn cleared_before(
    meter: &FrozenMeter,
) -> Option<aura_projection_contract::AgentContextClearedView> {
    let reduction =
        meter.reduction.as_ref().filter(|r| !r.compacted && r.cleared_tool_results > 0)?;
    Some(aura_projection_contract::AgentContextClearedView {
        cleared_tool_results: reduction.cleared_tool_results,
        input_tokens_before: reduction.input_tokens_before,
        input_tokens_after: meter.usage.as_ref()?.used_input_tokens,
    })
}

/// The compaction this step's context was prepared after, if one was.
fn compacted_before(
    db: &Connection,
    step: &ModelStep,
    session: SessionId,
    meter: &FrozenMeter,
) -> StoreResult<Option<aura_projection_contract::AgentContextCompactedView>> {
    let row: Option<(String, i64)> = db
        .query_row(
            concat!(
                "SELECT id, COALESCE(json_extract(payload,'$.retained_recent_tokens'),0) ",
                "FROM context_compactions WHERE session_id=?3 AND state='completed' ",
                "AND json_extract(payload,'$.agent_run_id')=?1 ",
                "AND json_extract(payload,'$.source_step_id')=?2 ",
                "ORDER BY json_extract(payload,'$.sequence') DESC LIMIT 1"
            ),
            params![
                step.agent_run_id.to_string(),
                step.model_step_id.to_string(),
                session.to_string()
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((id, retained)) = row else { return Ok(None) };
    // In the command owners' range of the index, not the whole journal.
    let (from, to) = crate::context::manual_compaction::command_owners();
    let manual: bool = db.query_row(
        concat!(
            "SELECT EXISTS(SELECT 1 FROM events WHERE owner>=?2 AND owner<?3 ",
            "AND kind='context.manual_compaction' ",
            "AND json_extract(payload,'$.record.source_context_snapshot_id')=?1)"
        ),
        params![id, from, to],
        |row| row.get(0),
    )?;
    // The size before is the step's own record of its preparation; steps prepared
    // by older versions have none, and their line keeps its older form.
    let reduction = meter.reduction.as_ref().filter(|reduction| reduction.compacted);
    Ok(Some(aura_projection_contract::AgentContextCompactedView {
        manual,
        retained_recent_tokens: u64::try_from(retained).unwrap_or_default(),
        input_tokens_before: reduction.map(|reduction| reduction.input_tokens_before),
        input_tokens_after: reduction
            .and_then(|_| meter.usage.as_ref().map(|usage| usage.used_input_tokens)),
        cleared_tool_results: reduction.map_or(0, |reduction| reduction.cleared_tool_results),
    }))
}

fn assistant(
    db: &Connection,
    step: &ModelStep,
    session: SessionId,
) -> StoreResult<AgentAssistantView> {
    let snapshot_id: Option<String> = db.query_row(
        "SELECT snapshot_id FROM model_steps WHERE id=?1",
        [step.model_step_id.to_string()],
        |r| r.get(0),
    )?;
    let snapshot = snapshot_id
        .map(|id| crate::model::snapshot(db, id.parse().map_err(|_| Error::Conflict)?))
        .transpose()?
        .flatten();
    let complete = matches!(step.state, ModelStepState::Completed { .. });
    // A preview is a window of at most `PREVIEW_BYTES`: the body is never assembled to
    // answer where it starts or how long it is, whether it is sealed or still growing.
    let preview_bytes = PREVIEW_BYTES as u64;
    let (text, reasoning, stream_sequence, stream_body_id, sealed_revision, tool_call_ids) =
        if complete {
            // The sealed body is read from its stored chunks: the index is checked against the
            // response it was written for and then says which chunks cover the preview, so only
            // those are fetched. A step stored before chunks existed — or whose chunks describe
            // older content — falls back to the response itself, which is the authority.
            let seal = history::sealed_body(db, step.model_step_id)?;
            let assistant = history::visible_body_window(
                db,
                step.model_step_id,
                history::VisibleSource::Assistant,
                0,
                preview_bytes,
                &seal,
            )?;
            let thinking = history::visible_body_window(
                db,
                step.model_step_id,
                history::VisibleSource::Reasoning,
                0,
                preview_bytes,
                &seal,
            )?;
            let (text, reasoning) = match (assistant, thinking) {
                (Some(text), Some(reasoning)) => (text, reasoning),
                _ => {
                    let output = history::output(db, step.model_step_id)?;
                    (
                        history::visible_window(&output, 0, preview_bytes),
                        history::reasoning_window(&output, 0, preview_bytes),
                    )
                }
            };
            let tools = history::tool_call_ids(db, step.model_step_id)?;
            (text, reasoning, None, None, Some(seal.revision.to_string()), tools)
        } else {
            let windows = super::stream::partial_windows(db, step.model_step_id, 0, preview_bytes)?;
            (windows.text, windows.reasoning, windows.sequence, windows.body_id, None, vec![])
        };
    let meter = frozen_context_meter(db, &step.model_step_id.to_string());
    let context_compacted = compacted_before(db, step, session, &meter)?;
    let context_cleared = cleared_before(&meter);
    Ok(AgentAssistantView {
        model_step_id: step.model_step_id,
        model_profile_id: snapshot.as_ref().map(|s| s.model_profile_id),
        context_snapshot_id: snapshot.map(|s| s.context_snapshot_id),
        text_total_bytes: text.total_bytes,
        reasoning_total_bytes: reasoning.total_bytes,
        text: text.text,
        reasoning: reasoning.text,
        partial: !complete,
        stream_sequence,
        stream_body_id,
        sealed_revision,
        context_usage: meter.usage,
        context_compaction_running: meter.compaction_running,
        context_compacted,
        context_cleared,
        backup_connection: crate::model::connections::backup_of(db, step.model_step_id)?,
        tool_call_ids,
        tool_input: vec![],
        at: step.created_at,
    })
}

pub fn text(
    db: &Connection,
    request: &AgentMessageTextRequest,
) -> StoreResult<AgentMessageTextPage> {
    // Sealed text never moves, so its pages are exact slices. A running step's
    // visible body grows while it is read, so its pages carry the identity of
    // the body they came from and the boundary they really start at.
    if !(4..=65_536).contains(&request.limit) {
        return Err(Error::Invalid("text page limit"));
    }
    // A text of a turn a branch carried is read from what the branch kept.
    if let Some(page) = super::branch::carried_text(db, request)? {
        return Ok(page);
    }
    let limit = u64::from(request.limit);
    let (window, body_id, sealed_revision, sealed) = match request.source {
        AgentMessageTextSource::Completion { turn_task_id } => {
            let (session_id, state): (String, String) = db.query_row(
                "SELECT session_id, state FROM turn_tasks WHERE id=?1",
                [turn_task_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            if session_id.parse::<SessionId>().map_err(|_| Error::Conflict)? != request.session_id
                || state != "completed"
            {
                return Err(Error::Conflict);
            }
            let sealed_revision = super::desktop::completion_digest(db, turn_task_id)?;
            if let Some(window) =
                history::completion_window(db, turn_task_id, request.offset, limit)?
            {
                if window.from != request.offset.min(window.total_bytes) {
                    return Err(Error::Invalid("text byte cursor"));
                }
                return Ok(AgentMessageTextPage {
                    source: request.source,
                    text: window.text,
                    next_offset: window.next,
                    total_bytes: window.total_bytes,
                    body_id: None,
                    sealed_revision,
                    from_offset: window.from,
                });
            }
            let total: i64 = db.query_row(
                "SELECT COALESCE(length(CAST(json_extract(payload, '$.state.final_report') AS BLOB)), 0) \
                 FROM turn_tasks WHERE id=?1",
                [turn_task_id.to_string()],
                |row| row.get(0),
            )?;
            let total_bytes = u64::try_from(total).map_err(|_| Error::Conflict)?;
            let start = request.offset;
            if start > total_bytes {
                return Err(Error::Invalid("text byte cursor"));
            }
            let take = limit.min(total_bytes.saturating_sub(start));
            let bytes: Vec<u8> = db.query_row(
                "SELECT substr(CAST(json_extract(payload, '$.state.final_report') AS BLOB), ?2, ?3) \
                 FROM turn_tasks WHERE id=?1",
                params![
                    turn_task_id.to_string(),
                    i64::try_from(start.saturating_add(1)).map_err(|_| Error::Conflict)?,
                    i64::try_from(take).map_err(|_| Error::Conflict)?,
                ],
                |row| row.get(0),
            )?;
            if bytes.first().is_some_and(|byte| byte & 0b1100_0000 == 0b1000_0000) {
                return Err(Error::Invalid("text byte cursor"));
            }
            let mut end = bytes.len();
            while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
                end -= 1;
            }
            let text = std::str::from_utf8(&bytes[..end]).unwrap_or("").to_owned();
            let next = start.saturating_add(end as u64);
            return Ok(AgentMessageTextPage {
                source: request.source,
                text,
                next_offset: (next < total_bytes).then_some(next),
                total_bytes,
                body_id: None,
                sealed_revision,
                from_offset: start,
            });
        }
        AgentMessageTextSource::User { event_id } => {
            // The message payload is the authority and never moves, so a page is a strict
            // slice of it. The slice is taken from the stored chunks when the message has an
            // index — the same window rule a sealed response uses — so a page of a long
            // message does not parse the message. A message written before chunks existed
            // falls back to the payload, which is the only copy for those.
            let source = session::user_message_source(db, request.session_id, event_id)?;
            // A message with pictures or files reads as what the user typed.
            let typed = super::attachments::of_message(db, event_id)?.map(|(typed, _)| typed);
            let body = history::ChunkBody::user_message(event_id, &source.content_hash);
            let window = match history::stored_body_window(db, &body, request.offset, limit)? {
                Some(window) => window,
                None => {
                    let content = session::user_message(db, request.session_id, event_id)?.text;
                    let (text, next_offset) = text_page(&content, request.offset, request.limit)?;
                    return Ok(typed_page(
                        AgentMessageTextPage {
                            source: request.source,
                            text,
                            next_offset,
                            total_bytes: content.len() as u64,
                            body_id: None,
                            sealed_revision: Some(source.content_hash.to_string()),
                            from_offset: request.offset,
                        },
                        typed,
                    ));
                }
            };
            if window.from != request.offset.min(window.total_bytes) {
                // The body cannot have moved, so an offset inside a character is a bad
                // request rather than something to resync from.
                return Err(Error::Invalid("text byte cursor"));
            }
            return Ok(typed_page(
                AgentMessageTextPage {
                    source: request.source,
                    text: window.text,
                    next_offset: window.next,
                    total_bytes: window.total_bytes,
                    body_id: None,
                    sealed_revision: Some(source.content_hash.to_string()),
                    from_offset: window.from,
                },
                typed,
            ));
        }
        AgentMessageTextSource::Assistant { model_step_id }
        | AgentMessageTextSource::Reasoning { model_step_id } => {
            let owner_session: String = db.query_row(
                "SELECT t.session_id FROM model_steps s \
                 JOIN agent_runs r ON r.id = s.agent_run_id \
                 JOIN turn_tasks t ON t.id = r.turn_task_id \
                 WHERE s.id=?1",
                [model_step_id.to_string()],
                |row| row.get(0),
            )?;
            if owner_session.parse::<SessionId>().map_err(|_| Error::Conflict)?
                != request.session_id
            {
                return Err(Error::Conflict);
            }
            let step = turns::step(db, model_step_id)?;
            let reasoning = matches!(request.source, AgentMessageTextSource::Reasoning { .. });
            if matches!(step.state, ModelStepState::Completed { .. }) {
                // The sealed body is read from its stored chunks when the step has them: the
                // index is checked against the response it was written for, then says which
                // chunks cover the page, and only those are fetched. A step stored before
                // chunks existed has no index, so the response itself is the fallback — the
                // same window, computed from the whole body.
                let source = if reasoning {
                    history::VisibleSource::Reasoning
                } else {
                    history::VisibleSource::Assistant
                };
                let seal = history::sealed_body(db, model_step_id)?;
                match history::visible_body_window(
                    db,
                    model_step_id,
                    source,
                    request.offset,
                    limit,
                    &seal,
                )? {
                    Some(window) => (window, None, Some(seal.revision.to_string()), true),
                    None => {
                        let output = history::output(db, model_step_id)?;
                        let window = if reasoning {
                            history::reasoning_window(&output, request.offset, limit)
                        } else {
                            history::visible_window(&output, request.offset, limit)
                        };
                        (window, None, Some(seal.revision.to_string()), true)
                    }
                }
            } else {
                let windows = super::stream::partial_windows_for(
                    db,
                    model_step_id,
                    request.offset,
                    limit,
                    if reasoning {
                        super::stream::WindowSide::Reasoning
                    } else {
                        super::stream::WindowSide::Text
                    },
                )?;
                let body_id = windows.body_id;
                let window = if reasoning { windows.reasoning } else { windows.text };
                (window, body_id, None, false)
            }
        }
    };
    if sealed && window.from != request.offset.min(window.total_bytes) {
        // Sealed pages keep the strict cursor: an offset inside a character is a bad
        // request, not something to resync from, because the text cannot have moved.
        return Err(Error::Invalid("text byte cursor"));
    }
    Ok(AgentMessageTextPage {
        source: request.source,
        text: window.text,
        next_offset: window.next,
        total_bytes: window.total_bytes,
        body_id,
        sealed_revision,
        from_offset: window.from,
    })
}

// A page of a body that can still grow is read as a window of the stored deltas; it snaps
// back to the boundary before an offset that falls inside a character instead of failing,
// because the client is reading a body that moved under it. The page reports where the
// returned text really starts, so the client can resync instead of showing half a character.

/// A page of an immutable body: a strict slice whose cursor must be exact.
pub(super) fn text_page(
    content: &str,
    offset: u64,
    limit: u32,
) -> StoreResult<(String, Option<u64>)> {
    if !(4..=65_536).contains(&limit) {
        return Err(Error::Invalid("text page limit"));
    }
    let start = usize::try_from(offset).map_err(|_| Error::Conflict)?;
    if !content.is_char_boundary(start) {
        return Err(Error::Invalid("text byte cursor"));
    }
    let mut end = start.saturating_add(limit as usize).min(content.len());
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    Ok((content[start..end].into(), (end < content.len()).then_some(end as u64)))
}
