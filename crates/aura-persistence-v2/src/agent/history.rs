//! History is rebuilt from the durable provider output and exact Tool facts.
//! A partial batch cannot be published, and a completed group cannot be changed.
use super::{facts, turns};
use aura_agent_domain::{AgentRunState, ModelStepState};
use aura_context_domain::{
    HistoryAtomicGroupKindV1, HistoryAtomicGroupPreimageV1, HistoryToolCallResultEntryV1,
};
use aura_domain_common::{jcs::canonical_sha256, *};
use aura_model_domain::NormalizedModelOutput;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact, record};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

/// The model-visible text of one response: assistant blocks plus a refusal
/// message, joined exactly as history and the desktop projection represent it.
#[must_use]
pub fn visible_text(output: &NormalizedModelOutput) -> String {
    output
        .assistant_blocks
        .iter()
        .map(|block| block.text.as_str())
        .chain(output.refusal.iter().map(|refusal| refusal.message.as_str()))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn record_output(
    tx: &Transaction<'_>,
    output: &NormalizedModelOutput,
    batch_id: Option<ToolBatchId>,
    at: UtcTimestamp,
) -> StoreResult<EventId> {
    let previous = turns::step(tx, output.model_step_id)?;
    let run = turns::load_run(tx, previous.agent_run_id)?;
    if run.state != (AgentRunState::ModelRunning { model_step_id: output.model_step_id })
        || !matches!(previous.state, ModelStepState::Running { .. })
        || output.tool_calls.is_empty() != batch_id.is_none()
    {
        return Err(Error::Conflict);
    }
    let event_id = EventId::new();
    let artifact_id = ArtifactId::new();
    let stored = serde_json::to_vec(output)?;
    // The digest of the stored response object is the identity of the sealed body. It is taken
    // from the bytes about to be written rather than recomputed from the response later, so a
    // reader can tell whether the chunks in front of it describe *this* response without
    // materializing the response to find out.
    let revision = ContentHash::blake3(&stored);
    artifact::put(tx, artifact_id, &run.agent_run_id.to_string(), "application/json", &stored, at)?;
    facts::append(
        tx,
        event_id,
        &run.agent_run_id.to_string(),
        "agent.assistant_response",
        output,
        at,
    )?;
    let mut next = previous.clone();
    next.state = ModelStepState::Completed {
        assistant_event_id: event_id,
        output_artifact_id: artifact_id,
        tool_batch_id: batch_id,
    };
    next.finished_at = Some(at);
    turns::save_step(tx, &previous, &next)?;
    // A sealed step will not receive another delta, so a legacy stream is repaired
    // here, on the write that owns the step, rather than on every later read.
    super::stream::repair_byte_index(tx, output.model_step_id, at)?;
    super::guard::mark_feedback_delivered(tx, run.turn_task_id, output.model_step_id, at)?;
    // The body is also stored as bounded chunks with a small index, so a later page reads
    // the bytes it asks for instead of parsing the whole response. The response event above
    // stays the authority: these rows are a read path for it, and a reader that finds no
    // chunks falls back to it.
    for (source, text) in [
        (VisibleSource::Assistant, visible_text(output)),
        (VisibleSource::Reasoning, visible_reasoning(output)),
    ] {
        record_chunks(
            tx,
            &ChunkBody {
                owner: output.model_step_id.to_string(),
                source,
                revision: revision.clone(),
            },
            &text,
            at,
        )?;
    }
    // The calls of one response are listed without their arguments, so a history list reads
    // this index instead of parsing the response. It is owned by the response event, which is
    // what makes it unambiguous: a step has one sealed response.
    facts::append(
        tx,
        EventId::new(),
        &event_id.to_string(),
        "agent.tool_call_index",
        &ToolCallIndex {
            step_id: output.model_step_id,
            revision: revision.clone(),
            call_ids: output.tool_calls.iter().map(|call| call.tool_call_id).collect(),
        },
        at,
    )?;
    Ok(event_id)
}

/// Which body a stored chunk belongs to.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VisibleSource {
    Assistant,
    Reasoning,
    Completion,
}

/// How much text one stored chunk carries.
///
/// The size is the granularity of a page read: a page asks for a range, the index says which
/// chunks cover it, and only those chunks are fetched. Smaller chunks make a small page
/// cheaper and the index longer; this is the balance, and the index for a megabyte of text is
/// a few hundred bytes.
const CHUNK_BYTES: usize = 64 * 1024;

/// One stored body: whose it is, which part of it, and what it was when it was stored.
///
/// The owner is a string rather than a model step because a user message is a body too. Its
/// text is the authority and its chunks are a read path, exactly as a sealed response's are,
/// so both use one mechanism instead of two that drift apart.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChunkBody {
    pub owner: String,
    pub source: VisibleSource,
    /// A digest of the whole text at the moment the chunks were written. A reader compares it
    /// against the authority it already holds, so an index that belongs to older content is
    /// refused instead of being used to answer for newer content.
    pub revision: ContentHash,
}

impl ChunkBody {
    /// The stored body of one user message, identified by the digest of its text.
    #[must_use]
    pub fn user_message(event_id: EventId, hash: &ContentHash) -> Self {
        Self {
            owner: event_id.to_string(),
            source: VisibleSource::Assistant,
            revision: hash.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ChunkIndex {
    owner: String,
    source: VisibleSource,
    /// The byte length of every chunk, in order: the cumulative sum is the byte offset each
    /// chunk starts at, so a reader can find the covering range without reading any text.
    lengths: Vec<u64>,
    /// The digest of the body these lengths describe.
    revision: ContentHash,
    /// Digest of each chunk's text, in order. Empty on an index stored before chunks were
    /// bound to a generation; a reader then checks length only, and only when that ordinal
    /// has a single unbound chunk.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    digests: Vec<ContentHash>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ChunkText {
    owner: String,
    source: VisibleSource,
    ordinal: u32,
    text: String,
    /// The body revision this chunk was cut from. Absent on chunks stored before generations
    /// were bound; those are usable only while they are the sole chunk at their ordinal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    revision: Option<ContentHash>,
    /// Digest of `text`. Checked against the index when the index recorded one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    digest: Option<ContentHash>,
}

/// Stores one body as bounded chunks plus the index a reader searches.
///
/// The body itself is not stored here: its authority is the caller's own record (the sealed
/// response for a model step, the message payload for user text). These rows are a read path
/// for it, and a reader that finds no index falls back to the authority rather than treating
/// missing chunks as missing content.
pub fn record_chunks(
    tx: &Transaction<'_>,
    body: &ChunkBody,
    text: &str,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let mut lengths = Vec::new();
    let mut digests = Vec::new();
    let mut start = 0_usize;
    let mut ordinal = 0_u32;
    while start < text.len() {
        let mut end = (start + CHUNK_BYTES).min(text.len());
        while end < text.len() && !text.is_char_boundary(end) {
            end += 1;
        }
        let chunk = &text[start..end];
        let digest = ContentHash::blake3(chunk.as_bytes());
        facts::append(
            tx,
            EventId::new(),
            &body.owner,
            "agent.visible_chunk",
            &ChunkText {
                owner: body.owner.clone(),
                source: body.source,
                ordinal,
                text: chunk.to_owned(),
                revision: Some(body.revision.clone()),
                digest: Some(digest.clone()),
            },
            at,
        )?;
        tx.execute(
            "INSERT INTO agent_visible_body(owner, source, revision, ordinal, byte_len, text) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
             ON CONFLICT(owner, source, revision, ordinal) DO UPDATE SET \
             byte_len=excluded.byte_len, text=excluded.text",
            params![
                body.owner,
                source_name(body.source),
                body.revision.hex,
                i64::from(ordinal),
                i64::try_from(chunk.len()).map_err(|_| Error::Conflict)?,
                chunk,
            ],
        )?;
        lengths.push(chunk.len() as u64);
        digests.push(digest);
        start = end;
        ordinal = ordinal.saturating_add(1);
    }
    if lengths.is_empty() {
        // An empty body still gets an index, so a reader can tell "stored, and empty" from
        // "not stored at all" and does not fall back to parsing the whole response.
        lengths.push(0);
    }
    facts::append(
        tx,
        EventId::new(),
        &body.owner,
        "agent.visible_chunk_index",
        &ChunkIndex {
            owner: body.owner.clone(),
            source: body.source,
            lengths,
            revision: body.revision.clone(),
            digests,
        },
        at,
    )
}

/// The stored chunk index of one body, when the body has one that still describes it.
///
/// The source is part of the query, not a filter applied afterwards: a step stores one index
/// per body, and taking the newest row and then rejecting it would report "no index" for the
/// body that was stored first.
///
/// `revision` is the caller's authority for the body. An index whose digest differs belongs to
/// older content — the body was rewritten and the old rows are still there — so it is reported
/// as absent and the caller reads its own authority instead. Answering from it would return
/// bytes that no longer exist.
fn relation_lengths(db: &Connection, body: &ChunkBody) -> StoreResult<Option<Vec<u64>>> {
    let mut statement = db.prepare(
        "SELECT ordinal, byte_len FROM agent_visible_body \
         WHERE owner=?1 AND source=?2 AND revision=?3 ORDER BY ordinal",
    )?;
    let rows = statement
        .query_map(params![body.owner, source_name(body.source), body.revision.hex], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?;
    let mut lengths = Vec::new();
    for (ordinal, row) in rows.enumerate() {
        let (stored, byte_len) = row?;
        if stored != i64::try_from(ordinal).map_err(|_| Error::Conflict)? || byte_len < 0 {
            return Err(Error::Conflict);
        }
        lengths.push(u64::try_from(byte_len).map_err(|_| Error::Conflict)?);
    }
    Ok((!lengths.is_empty()).then_some(lengths))
}

fn relation_text(db: &Connection, body: &ChunkBody, ordinal: i64) -> StoreResult<Option<String>> {
    db.query_row(
        "SELECT text FROM agent_visible_body \
         WHERE owner=?1 AND source=?2 AND revision=?3 AND ordinal=?4",
        params![body.owner, source_name(body.source), body.revision.hex, ordinal],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

fn chunk_index(db: &Connection, body: &ChunkBody) -> StoreResult<Option<ChunkIndex>> {
    if let Some(lengths) = relation_lengths(db, body)? {
        return Ok(Some(ChunkIndex {
            owner: body.owner.clone(),
            source: body.source,
            lengths,
            revision: body.revision.clone(),
            digests: Vec::new(),
        }));
    }
    let raw: Option<String> = db
        .query_row(
            "SELECT payload FROM events WHERE owner=?1 AND kind='agent.visible_chunk_index' \
             AND version=1 AND json_extract(payload,'$.source')=?2 \
             AND json_extract(payload,'$.revision.hex')=?3 ORDER BY sequence DESC LIMIT 1",
            params![body.owner, source_name(body.source), body.revision.hex],
            |row| row.get(0),
        )
        .optional()?;
    let Some(raw) = raw else { return Ok(None) };
    let index: ChunkIndex = record::decode(&raw)?;
    if index.owner != body.owner
        || index.source != body.source
        || index.revision != body.revision
        || (!index.digests.is_empty() && index.digests.len() != index.lengths.len())
    {
        return Err(Error::Conflict);
    }
    Ok(Some(index))
}

/// The total byte length of a stored body without reading any of its text.
///
/// A preview needs "how long is this" and "what does it start with", and both answers come
/// from the index plus the chunks the window touches. Assembling the body to measure it is
/// what this replaces.
/// A page of a completed report from the chunks written with the turn.
///
/// The index row is the small length list, not the report. When this turn was
/// stored before chunks existed, the caller still has the report payload.
pub fn completion_window(
    db: &Connection,
    turn_task_id: TurnTaskId,
    offset: u64,
    limit: u64,
) -> StoreResult<Option<VisibleWindow>> {
    let located: Option<String> = db
        .query_row(
            "SELECT revision FROM agent_visible_body WHERE owner=?1 AND source='completion' \
             ORDER BY rowid DESC LIMIT 1",
            [turn_task_id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(hex) = located {
        let revision =
            ContentHash::new(ContentHashAlgorithm::Blake3, hex).map_err(|_| Error::Conflict)?;
        return stored_body_window(
            db,
            &ChunkBody {
                owner: turn_task_id.to_string(),
                source: VisibleSource::Completion,
                revision,
            },
            offset,
            limit,
        );
    }
    let raw: Option<String> = db
        .query_row(
            "SELECT payload FROM events WHERE owner=?1 AND kind='agent.visible_chunk_index' \
             AND version=1 AND json_extract(payload,'$.source')='completion' \
             ORDER BY sequence DESC LIMIT 1",
            [turn_task_id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    let Some(raw) = raw else { return Ok(None) };
    let index: ChunkIndex = record::decode(&raw)?;
    if index.owner != turn_task_id.to_string() || index.source != VisibleSource::Completion {
        return Err(Error::Conflict);
    }
    stored_body_window(
        db,
        &ChunkBody {
            owner: turn_task_id.to_string(),
            source: VisibleSource::Completion,
            revision: index.revision,
        },
        offset,
        limit,
    )
}

pub fn stored_length(db: &Connection, body: &ChunkBody) -> StoreResult<Option<u64>> {
    Ok(chunk_index(db, body)?
        .map(|index| index.lengths.iter().copied().fold(0_u64, u64::saturating_add)))
}

/// How a body's source is spelled in the stored rows.
fn source_name(source: VisibleSource) -> &'static str {
    match source {
        VisibleSource::Assistant => "assistant",
        VisibleSource::Reasoning => "reasoning",
        VisibleSource::Completion => "completion",
    }
}

/// One bounded window read from the stored chunks of a sealed body.
///
/// The index gives every chunk's length, so the reader turns a byte window into a chunk
/// range, fetches only those chunks, and copies only the window out of them. A body of any
/// size therefore costs the same as its index plus the chunks the page touches — the
/// response itself is never parsed for a page. `chunks_read` reports how many chunk rows
/// were really fetched, so "the read stayed bounded" is a fact a caller can check.
/// The identity of a sealed response, without its text.
///
/// A reader that wants a bounded window needs to know *which* body it is reading and what that
/// body is, so an index describing older content is refused — but not the text itself.
/// Materializing the response to answer "is this index current?" would defeat the window, so
/// the digest of the stored response object is the identity instead.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SealedBody {
    pub step_id: ModelStepId,
    pub revision: ContentHash,
}

/// The digest of one sealed response, read from its stored artifact.
pub fn sealed_body(db: &Connection, step_id: ModelStepId) -> StoreResult<SealedBody> {
    let step = turns::step(db, step_id)?;
    let ModelStepState::Completed { output_artifact_id, .. } = step.state else {
        return Err(Error::Conflict);
    };
    let revision = artifact::identity(db, output_artifact_id, &step.agent_run_id.to_string())?;
    Ok(SealedBody { step_id, revision })
}

/// One bounded window of a sealed response body, when its chunks still describe it.
pub fn visible_body_window(
    db: &Connection,
    step_id: ModelStepId,
    source: VisibleSource,
    offset: u64,
    limit: u64,
    seal: &SealedBody,
) -> StoreResult<Option<VisibleWindow>> {
    if seal.step_id != step_id {
        return Err(Error::Conflict);
    }
    stored_body_window(
        db,
        &ChunkBody { owner: step_id.to_string(), source, revision: seal.revision.clone() },
        offset,
        limit,
    )
}

/// The tool call ids of one sealed response.
///
/// A history list shows the calls a response made; it does not show their arguments. The ids
/// are taken from the response's own stored tool-call index when it has one, so listing turns
/// does not parse the response, and from the response itself otherwise.
pub fn tool_call_ids(db: &Connection, step_id: ModelStepId) -> StoreResult<Vec<ToolCallId>> {
    let step = turns::step(db, step_id)?;
    let ModelStepState::Completed { assistant_event_id, output_artifact_id, .. } = step.state
    else {
        return Ok(Vec::new());
    };
    let owner = step.agent_run_id.to_string();
    let revision = artifact::identity(db, output_artifact_id, &owner)?;
    if let Some(ids) = tool_call_index(db, assistant_event_id, step_id, &revision)? {
        return Ok(ids);
    }
    let output: NormalizedModelOutput =
        facts::exact(db, assistant_event_id, &owner, "agent.assistant_response")?;
    if output.model_step_id != step_id {
        return Err(Error::Conflict);
    }
    Ok(output.tool_calls.iter().map(|call| call.tool_call_id).collect())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ToolCallIndex {
    step_id: ModelStepId,
    revision: ContentHash,
    call_ids: Vec<ToolCallId>,
}

fn tool_call_index(
    db: &Connection,
    assistant_event_id: EventId,
    step_id: ModelStepId,
    revision: &ContentHash,
) -> StoreResult<Option<Vec<ToolCallId>>> {
    let raw: Option<String> = db
        .query_row(
            "SELECT payload FROM events WHERE owner=?1 AND kind='agent.tool_call_index' \
             AND version=1 AND json_extract(payload,'$.step_id')=?2 \
             AND json_extract(payload,'$.revision.hex')=?3",
            params![assistant_event_id.to_string(), step_id.to_string(), revision.hex],
            |row| row.get(0),
        )
        .optional()?;
    let Some(raw) = raw else { return Ok(None) };
    let index: ToolCallIndex = record::decode(&raw)?;
    if index.step_id != step_id || index.revision != *revision {
        return Err(Error::Conflict);
    }
    Ok(Some(index.call_ids))
}

/// One bounded window of any stored body, sealed response or user message alike.
///
/// Both bodies are read the same way — the index says which chunks cover the window and only
/// those are fetched — because they are the same problem: text whose authority lives elsewhere
/// and whose read path must not assemble it.
pub fn stored_body_window(
    db: &Connection,
    body: &ChunkBody,
    offset: u64,
    limit: u64,
) -> StoreResult<Option<VisibleWindow>> {
    let Some(index) = chunk_index(db, body)? else {
        return Ok(None);
    };
    let total: u64 = index.lengths.iter().copied().fold(0_u64, u64::saturating_add);
    let start = offset.min(total);
    let end = start.saturating_add(limit).min(total);
    // The widened range the copy may touch, matching the in-memory walk's slack.
    let copy_from = start.saturating_sub(3);
    let copy_to = end.saturating_add(3).min(total);
    let mut window = WindowWriter::new(offset, limit);
    let mut chunks_read = 0_u32;
    let mut decoded_bytes = 0_u64;
    let mut piece_start = 0_u64;
    for (ordinal, length) in index.lengths.iter().enumerate() {
        let piece_end = piece_start.saturating_add(*length);
        if piece_end <= copy_from || piece_start >= copy_to {
            // A chunk outside the window is not fetched, but the cursor still has to know
            // how far it is from the start of the body: the window's offsets are absolute,
            // and the reported total is the whole body.
            window.advance(*length);
            piece_start = piece_end;
            continue;
        }
        let ordinal_value = i64::try_from(ordinal).map_err(|_| Error::Conflict)?;
        let text = match relation_text(db, body, ordinal_value)? {
            Some(text) => text,
            None => {
                let raw: Option<String> = db
                    .query_row(
                        "SELECT payload FROM events WHERE owner=?1 AND kind='agent.visible_chunk' \
                         AND version=1 AND json_extract(payload,'$.ordinal')=?2 \
                         AND json_extract(payload,'$.source')=?3 \
                         AND (json_extract(payload,'$.revision.hex')=?4 \
                              OR json_extract(payload,'$.revision') IS NULL) \
                         ORDER BY CASE WHEN json_extract(payload,'$.revision.hex')=?4 THEN 0 ELSE 1 END, \
                         sequence DESC LIMIT 1",
                        params![
                            body.owner,
                            ordinal_value,
                            source_name(body.source),
                            body.revision.hex,
                        ],
                        |row| row.get(0),
                    )
                    .optional()?;
                let Some(raw) = raw else { return Err(Error::Conflict) };
                let chunk: ChunkText = record::decode(&raw)?;
                if chunk.ordinal as usize != ordinal
                    || chunk.owner != body.owner
                    || chunk.source != body.source
                {
                    return Err(Error::Conflict);
                }
                match &chunk.revision {
                    Some(revision) if revision != &body.revision => return Err(Error::Conflict),
                    Some(_) => {}
                    None => {
                        let siblings: i64 = db.query_row(
                            "SELECT COUNT(*) FROM events WHERE owner=?1 AND kind='agent.visible_chunk' \
                             AND version=1 AND json_extract(payload,'$.ordinal')=?2 \
                             AND json_extract(payload,'$.source')=?3",
                            params![
                                body.owner,
                                ordinal_value,
                                source_name(body.source),
                            ],
                            |row| row.get(0),
                        )?;
                        if siblings != 1 {
                            return Err(Error::Conflict);
                        }
                    }
                }
                chunk.text
            }
        };
        if text.len() as u64 != *length {
            return Err(Error::Conflict);
        }
        let chunk = ChunkText {
            owner: body.owner.clone(),
            source: body.source,
            ordinal: u32::try_from(ordinal).map_err(|_| Error::Conflict)?,
            text,
            revision: Some(body.revision.clone()),
            digest: None,
        };
        if chunk.ordinal as usize != ordinal
            || chunk.owner != body.owner
            || chunk.source != body.source
            || chunk.text.len() as u64 != *length
        {
            return Err(Error::Conflict);
        }
        match &chunk.revision {
            Some(revision) if revision != &body.revision => return Err(Error::Conflict),
            Some(_) => {}
            None => {
                let siblings: i64 = db.query_row(
                    "SELECT COUNT(*) FROM events WHERE owner=?1 AND kind='agent.visible_chunk' \
                     AND version=1 AND json_extract(payload,'$.ordinal')=?2 \
                     AND json_extract(payload,'$.source')=?3",
                    params![
                        body.owner,
                        i64::try_from(ordinal).map_err(|_| Error::Conflict)?,
                        source_name(body.source),
                    ],
                    |row| row.get(0),
                )?;
                // An unbound chunk cannot be told apart from another generation's chunk at
                // the same ordinal. Refuse it instead of answering with the older text.
                if siblings != 1 {
                    return Ok(None);
                }
            }
        }
        if let Some(expected) = index.digests.get(ordinal) {
            let actual = chunk.digest.unwrap_or_else(|| ContentHash::blake3(chunk.text.as_bytes()));
            if actual != *expected {
                return Err(Error::Conflict);
            }
        }
        chunks_read = chunks_read.saturating_add(1);
        decoded_bytes = decoded_bytes.saturating_add(chunk.text.len() as u64);
        // The chunks are contiguous slices of the body that was joined when it was stored,
        // so they are concatenated exactly as they were cut: inserting a separator here
        // would shift every offset after the first chunk and return the wrong bytes.
        window.push(&chunk.text);
        piece_start = piece_end;
    }
    let mut window = window.finish();
    window.chunks_read = chunks_read;
    window.decoded_bytes = decoded_bytes;
    Ok(Some(window))
}
pub fn output(db: &Connection, step_id: ModelStepId) -> StoreResult<NormalizedModelOutput> {
    let step = turns::step(db, step_id)?;
    let ModelStepState::Completed { assistant_event_id, output_artifact_id, .. } = step.state
    else {
        return Err(Error::Conflict);
    };
    let from_event: NormalizedModelOutput = facts::exact(
        db,
        assistant_event_id,
        &step.agent_run_id.to_string(),
        "agent.assistant_response",
    )?;
    let from_object: NormalizedModelOutput = serde_json::from_slice(&artifact::read(
        db,
        output_artifact_id,
        &step.agent_run_id.to_string(),
    )?)?;
    if from_event != from_object || from_event.model_step_id != step_id {
        return Err(Error::Conflict);
    }
    Ok(from_event)
}
pub fn complete_batch(
    tx: &Transaction<'_>,
    batch_id: ToolBatchId,
    at: UtcTimestamp,
) -> StoreResult<HistoryAtomicGroupPreimageV1> {
    let stored = crate::tool::execution::batch(tx, batch_id)?;
    let already_published = stored.result.is_some()
        && crate::context::history(tx, stored.batch.invocation_identity.session_id)?
            .iter()
            .any(|h| h.tool_batch_id == batch_id);
    let batch = stored.batch;
    let identity = &batch.invocation_identity;
    let step = turns::step(tx, identity.model_step_id)?;
    let ModelStepState::Completed { assistant_event_id, tool_batch_id: Some(expected), .. } =
        step.state
    else {
        return Err(Error::Conflict);
    };
    let run = turns::load_run(tx, identity.agent_run_id)?;
    let turn = turns::load(tx, identity.turn_task_id)?;
    if expected != batch_id
        || step.agent_run_id != run.agent_run_id
        || run.turn_task_id != turn.turn_task_id
        || turn.session_id != identity.session_id
        || (!already_published
            && !matches!(run.state,AgentRunState::ToolsRunning{tool_batch_id}|AgentRunState::AwaitingApproval{tool_batch_id} if tool_batch_id==batch_id))
    {
        return Err(Error::Conflict);
    }
    let output = output(tx, step.model_step_id)?;
    // Protocol wire names contain the exact ToolId; canonical model-facing
    // names alone cannot prove which schema the Provider actually received.
    let snapshot = crate::model::step_snapshot(tx, step.model_step_id)?;
    let evidence = crate::model::request_evidence(tx, snapshot.id)?.ok_or(Error::Conflict)?;
    let normalized = crate::hooks::admitted_calls(tx, &snapshot, &evidence, &output)?;
    if normalized != batch.calls {
        return Err(Error::Conflict);
    }
    let result = crate::tool::execution::finish(tx, batch_id, at)?;
    let group = HistoryAtomicGroupPreimageV1 {
        kind: HistoryAtomicGroupKindV1,
        preimage_version: ToolWireVersionV1,
        session_id: identity.session_id,
        turn_task_id: identity.turn_task_id,
        agent_run_id: identity.agent_run_id,
        model_step_id: identity.model_step_id,
        assistant_response_event_id: assistant_event_id,
        assistant_response_content_hash: canonical_sha256(&output).map_err(|_| Error::Conflict)?,
        tool_batch_id: batch_id,
        calls: batch
            .calls
            .iter()
            .zip(&result.results)
            .map(|(call, result)| {
                Ok(HistoryToolCallResultEntryV1 {
                    ordinal: call.ordinal,
                    tool_call_id: call.tool_call_id,
                    tool_call_fact_hash: canonical_sha256(call).map_err(|_| Error::Conflict)?,
                    tool_result_fact_hash: canonical_sha256(result).map_err(|_| Error::Conflict)?,
                })
            })
            .collect::<StoreResult<Vec<_>>>()?,
        batch_summary_hash: canonical_sha256(&result).map_err(|_| Error::Conflict)?,
    };
    crate::context::append_history(tx, &group)?;
    Ok(group)
}

/// One window of the visible text, built without materializing the whole body.
///
/// The visible text is the assistant blocks joined by a newline. A page, or a preview plus
/// a length, walks the same pieces and copies only the bytes it asks for, so a large answer
/// is never assembled to answer a small question. The offsets are exactly the ones the
/// whole string would have and the returned window is snapped to character boundaries by
/// the same rule the in-memory page reader uses, which is why the page contract does not
/// change. `copied_bytes` reports how much was really copied, so a caller — and a test —
/// can see the read stayed bounded instead of trusting it.
pub struct VisibleWindow {
    pub text: String,
    pub total_bytes: u64,
    /// Absolute offset the returned text starts at.
    pub from: u64,
    /// Where the next page starts, or `None` when this window reaches the end.
    pub next: Option<u64>,
    pub copied_bytes: u64,
    /// How many stored chunk rows this read really fetched.
    ///
    /// A window read from the chunk index reports it, so "the page did not read the whole
    /// body" is a number the caller — and a test — can check rather than trust.
    pub chunks_read: u32,
    /// How many bytes arrived from the store and were parsed to answer this window.
    ///
    /// This is the read's real cost in the layer that can see it: `copied_bytes` says how much
    /// was kept, `chunks_read` says how many rows were touched, and this says how much text
    /// had to be decoded to produce them. A bounded read keeps all three near the window
    /// rather than near the body.
    pub decoded_bytes: u64,
}

impl VisibleWindow {
    /// A whole-body window, for a caller that really needs all of it.
    #[must_use]
    pub fn whole(text: String) -> Self {
        let total = text.len() as u64;
        Self {
            text,
            total_bytes: total,
            from: 0,
            next: None,
            copied_bytes: total,
            chunks_read: 0,
            decoded_bytes: 0,
        }
    }
}

pub fn visible_window(output: &NormalizedModelOutput, offset: u64, limit: u64) -> VisibleWindow {
    window_of(
        output
            .assistant_blocks
            .iter()
            .map(|block| block.text.as_str())
            .chain(output.refusal.iter().map(|refusal| refusal.message.as_str())),
        offset,
        limit,
    )
}

/// The reasoning text of one response, joined exactly as the projection represents it.
#[must_use]
pub fn visible_reasoning(output: &NormalizedModelOutput) -> String {
    output
        .reasoning_blocks
        .iter()
        .map(|block| block.summary.as_deref().unwrap_or(&block.text))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A bounded window of the reasoning text, joined the way `visible_reasoning` joins it.
pub fn reasoning_window(output: &NormalizedModelOutput, offset: u64, limit: u64) -> VisibleWindow {
    window_of(
        output.reasoning_blocks.iter().map(|block| block.summary.as_deref().unwrap_or(&block.text)),
        offset,
        limit,
    )
}

/// The window walk itself, over any sequence of pieces joined by newlines.
///
/// Pieces are pushed one at a time and only the bytes inside the widened window are kept, so
/// a caller whose pieces arrive from the store — a delta per row — never holds the body. The
/// total is known once the last piece is pushed. Widening by up to three bytes on each side
/// is what lets the boundary snap happen locally: a UTF-8 character is never longer than
/// four bytes, so a boundary can be honoured without seeing the rest of the body.
pub(super) fn window_of<'a>(
    pieces: impl Iterator<Item = &'a str>,
    offset: u64,
    limit: u64,
) -> VisibleWindow {
    let mut window = WindowWriter::new(offset, limit);
    for piece in pieces {
        window.push_separator();
        window.push(piece);
    }
    window.finish()
}

/// Accumulates a single window while its pieces stream past.
///
/// The buffer holds the requested bytes plus at most nine bytes of slack: three before and
/// three after the window, and three more on each side when a window edge falls inside a
/// character of the piece being copied and the copy has to be widened to that character.
pub(super) struct WindowWriter {
    offset: u64,
    limit: u64,
    /// Position in the whole body after every piece pushed so far.
    cursor: u64,
    /// Absolute offset of `buffer`'s first byte; at most three bytes before `offset`.
    buffer_from: u64,
    buffer: String,
    pushed: bool,
}

impl WindowWriter {
    pub(super) fn new(offset: u64, limit: u64) -> Self {
        Self {
            offset,
            limit,
            cursor: 0,
            buffer_from: offset.saturating_sub(3),
            buffer: String::new(),
            pushed: false,
        }
    }

    /// Advances the cursor over bytes the caller is not copying.
    ///
    /// A reader that skips a stored chunk still has to place the next one at its real
    /// offset in the body, and the reported total is the whole body, so the walk counts
    /// what it skipped without copying it.
    pub(super) fn advance(&mut self, bytes: u64) {
        self.cursor = self.cursor.saturating_add(bytes);
    }

    /// Joins the next piece to the previous one the way the whole body is joined.
    pub(super) fn push_separator(&mut self) {
        if self.pushed {
            self.push("\n");
        }
    }

    pub(super) fn push(&mut self, piece: &str) {
        self.pushed = true;
        let start = self.cursor;
        let stop = start.saturating_add(piece.len() as u64);
        self.cursor = stop;
        let to = self.offset.saturating_add(self.limit).saturating_add(3);
        if stop <= self.buffer_from || start >= to {
            return;
        }
        let mut begin = start.max(self.buffer_from);
        let mut finish = to.min(stop);
        // The window edges are byte offsets in the whole body, so they can fall inside a
        // character of this piece. Widening to the piece's own boundaries is what lets the
        // snap in `finish` see the whole character instead of a broken one.
        while begin > start && !piece.is_char_boundary((begin - start) as usize) {
            begin -= 1;
        }
        while finish < stop && !piece.is_char_boundary((finish - start) as usize) {
            finish += 1;
        }
        if finish <= begin {
            return;
        }
        if self.buffer.is_empty() {
            // The first copied bytes fix where the buffer starts. They can sit a few bytes
            // before the requested window, which is exactly the slack the snap needs.
            self.buffer_from = begin;
        }
        let begin = begin.max(self.buffer_from);
        if finish <= begin {
            return;
        }
        self.buffer.push_str(&piece[(begin - start) as usize..(finish - start) as usize]);
    }

    pub(super) fn finish(self) -> VisibleWindow {
        let total_bytes = self.cursor;
        let start = self.offset.min(total_bytes);
        let copied_end = self.buffer_from.saturating_add(self.buffer.len() as u64);
        if self.buffer.is_empty() || start >= copied_end {
            // The window lies past the bytes that exist: an empty page at the end of the
            // body, not a failure.
            return VisibleWindow {
                text: String::new(),
                total_bytes,
                from: start,
                next: (start < total_bytes).then_some(start),
                copied_bytes: 0,
                chunks_read: 0,
                decoded_bytes: 0,
            };
        }
        // The page starts where the caller asked, snapped back to the boundary at or before
        // it — the same rule the live page reader uses — and then spans the limit from
        // there, so a page never starts inside a character. Both ends only ever move back,
        // and the requested end is the one the snapped start implies.
        let mut from = start.max(self.buffer_from);
        while from > self.buffer_from
            && !self.buffer.is_char_boundary((from - self.buffer_from) as usize)
        {
            from -= 1;
        }
        let mut to = from.saturating_add(self.limit).min(total_bytes).min(copied_end);
        while to > from && !self.buffer.is_char_boundary((to - self.buffer_from) as usize) {
            to -= 1;
        }
        let text = self.buffer
            [(from - self.buffer_from) as usize..(to - self.buffer_from) as usize]
            .to_owned();
        VisibleWindow {
            text,
            total_bytes,
            from,
            next: (to < total_bytes).then_some(to),
            copied_bytes: self.buffer.len() as u64,
            chunks_read: 0,
            decoded_bytes: 0,
        }
    }
}
