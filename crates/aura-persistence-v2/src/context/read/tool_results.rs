//! Session-scoped tool result pages backed by the original sealed artifact and its text index.
use super::*;
use crate::agent::{branch, history};
use crate::tool::execution::{self, BatchExecution, CallExecution};
use aura_tool_contract::{NormalizedToolCallInputStateV1, ToolResultOutcomeV1, ToolResultV1};

const PAGE_SIZE: u64 = 65_536;
const INDEX_KIND: &str = "context.tool_result_pages";

#[derive(Deserialize, Serialize)]
struct PageIndex {
    artifact_hash: ContentHash,
    text_hash: ContentHash,
}

/// Store the same model-readable body that the short receipt refers to, including applied Hooks.
/// The call owns both the sealed result and this derived read path, so session purge removes both.
pub fn index_tool_result(
    tx: &Transaction<'_>,
    id: ToolCallId,
    text: &str,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let call = execution::call(tx, id)?;
    let result = call.result.ok_or(Error::Conflict)?;
    let artifact_hash = artifact::identity(tx, artifact_id(&result)?, &id.to_string())?;
    let text_hash = ContentHash::blake3(text.as_bytes());
    if index(tx, id, &artifact_hash)?.is_some_and(|saved| saved.revision == text_hash) {
        return Ok(());
    }
    save_index(tx, id, text, artifact_hash, text_hash, at)
}

pub fn tool_result(
    tx: &Transaction<'_>,
    session: SessionId,
    run: AgentRunId,
    input: &AgentContextReadToolResultInput,
    at: UtcTimestamp,
) -> StoreResult<ContextReadToolResultResult> {
    scope(tx, session, run)?;
    let id = input.tool_call_id;
    let source = match branch::carried_tool_result(tx, session, id)? {
        Some(carried) => ResultSource::Carried(carried),
        None => {
            ResultSource::own(tx, session, id, at).map_err(|error| unreadable(tx, id, error))?
        }
    };
    let hash = source.hash();
    let legacy = input.offset_bytes.is_none() && input.max_bytes.is_none();
    let max_bytes = input.max_bytes.unwrap_or(PAGE_SIZE);
    if !(4..=PAGE_SIZE).contains(&max_bytes) {
        return Err(Error::Invalid("context.read_tool_result.max_bytes: use 4 to 65,536 bytes"));
    }
    let binding = if legacy {
        canonical_sha256(&(id, input.include_input, &hash))
    } else {
        canonical_sha256(&(id, input.include_input, &hash, input.offset_bytes, max_bytes))
    }
    .map_err(|_| Error::Conflict)?;
    let mut page = cursor(tx, run, input.cursor.as_deref(), "tool_result", binding)?;
    let offset =
        if input.cursor.is_some() { page.position as u64 } else { input.offset_bytes.unwrap_or(0) };
    let limit = if legacy { 4 * PAGE_SIZE } else { max_bytes };
    let window = source.window(tx, offset, limit)?;
    if offset > window.total_bytes || (input.cursor.is_some() && offset != window.from) {
        return Err(Error::Invalid("context.read_tool_result.offset_bytes: outside the result"));
    }
    let mut text = window.text;
    if legacy && let Some((end, _)) = text.char_indices().nth(PAGE_SIZE as usize) {
        text.truncate(end);
    }
    let end = window.from + text.len() as u64;
    let (tool_name, arguments, is_error) = source.details(input.include_input && offset == 0)?;
    if arguments.as_ref().is_some_and(|value| value.chars().count() > HARD_CHARS) {
        return Err(Error::Invalid(
            "context.read_tool_result.include_input: input exceeds 262,144 characters; set include_input to false",
        ));
    }
    page.position = usize::try_from(end).map_err(|_| Error::Conflict)?;
    Ok(ContextReadToolResultResult {
        value: ContextReadToolResultView {
            tool_call_id: id,
            tool_name,
            input: arguments,
            result: text,
            is_error,
            content_hash: hash,
        },
        next_cursor: (end < window.total_bytes)
            .then(|| save_cursor(tx, run, &page, at))
            .transpose()?,
    })
}

enum ResultSource {
    Carried(branch::CarriedToolResult),
    Indexed { body: history::ChunkBody, details: branch::CarriedToolResult },
}

impl ResultSource {
    fn own(
        tx: &Transaction<'_>,
        session: SessionId,
        id: ToolCallId,
        at: UtcTimestamp,
    ) -> StoreResult<Self> {
        let (call, name) = scoped_call(tx, session, id)?;
        let result = call.result.as_ref().ok_or(Error::Conflict)?;
        let artifact_hash = artifact::identity(tx, artifact_id(result)?, &id.to_string())?;
        if let Some(body) = index(tx, id, &artifact_hash)? {
            return Ok(Self::Indexed { body, details: details(call, name, String::new())? });
        }
        let text = body_text(tx, result, &name)?;
        if text.len() as u64 > PAGE_SIZE {
            let text_hash = ContentHash::blake3(text.as_bytes());
            save_index(tx, id, &text, artifact_hash, text_hash, at)?;
        }
        Ok(Self::Carried(details(call, name, text)?))
    }

    fn hash(&self) -> ContentHash {
        match self {
            Self::Carried(whole) => ContentHash::blake3(whole.result.as_bytes()),
            Self::Indexed { body, .. } => body.revision.clone(),
        }
    }

    fn window(
        &self,
        db: &Connection,
        offset: u64,
        limit: u64,
    ) -> StoreResult<history::VisibleWindow> {
        match self {
            Self::Carried(whole) => {
                Ok(history::window_of([whole.result.as_str()].into_iter(), offset, limit))
            }
            Self::Indexed { body, .. } => {
                history::stored_body_window(db, body, offset, limit)?.ok_or(Error::Conflict)
            }
        }
    }

    fn details(self, include_input: bool) -> StoreResult<(String, Option<String>, bool)> {
        let value = match self {
            Self::Carried(value) | Self::Indexed { details: value, .. } => value,
        };
        let args = if include_input { Some(value.input.ok_or(Error::Conflict)?) } else { None };
        Ok((value.tool_name, args, value.is_error))
    }
}

fn index(
    db: &Connection,
    id: ToolCallId,
    artifact_hash: &ContentHash,
) -> StoreResult<Option<history::ChunkBody>> {
    let owner = id.to_string();
    let raw: Option<String> = db.query_row(
        "SELECT payload FROM events WHERE owner=?1 AND kind=?2 AND version=1 ORDER BY sequence DESC LIMIT 1",
        params![owner, INDEX_KIND], |r| r.get(0),
    ).optional()?;
    let saved: Option<PageIndex> = raw.map(|raw| record::decode(&raw)).transpose()?;
    Ok(saved
        .filter(|row| row.artifact_hash == *artifact_hash)
        .map(|row| chunk_body(id, row.text_hash)))
}

fn chunk_body(id: ToolCallId, hash: ContentHash) -> history::ChunkBody {
    history::ChunkBody {
        owner: id.to_string(),
        source: history::VisibleSource::ToolResult,
        revision: hash,
    }
}

fn save_index(
    tx: &Transaction<'_>,
    id: ToolCallId,
    text: &str,
    artifact_hash: ContentHash,
    text_hash: ContentHash,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let body = chunk_body(id, text_hash.clone());
    history::record_chunks(tx, &body, text, at)?;
    event::append(
        tx,
        &event::EventDraft {
            id: EventId::new(),
            owner: &body.owner,
            kind: INDEX_KIND,
            version: 1,
            payload: &serde_json::to_value(PageIndex { artifact_hash, text_hash })?,
            at,
        },
        "context",
    )?;
    Ok(())
}

fn scoped_call(
    db: &Connection,
    session: SessionId,
    id: ToolCallId,
) -> StoreResult<(CallExecution, String)> {
    let batch_id: String = db.query_row(
        "SELECT c.batch_id FROM tool_calls c JOIN tool_batches b ON b.id=c.batch_id WHERE c.id=?1 AND b.session_id=?2",
        params![id.to_string(), session.to_string()], |r| r.get(0),
    )?;
    let batch = execution::batch(db, batch_id.parse().map_err(|_| Error::Conflict)?)?;
    let producer = crate::agent::turns::load_run(db, batch.batch.invocation_identity.agent_run_id)?;
    if producer.parent_agent_run_id.is_some() {
        return Err(Error::Conflict);
    }
    let call = execution::call(db, id)?;
    let name = tool_name(db, &batch, &call)?;
    Ok((call, name))
}

fn tool_name(db: &Connection, batch: &BatchExecution, call: &CallExecution) -> StoreResult<String> {
    let Some(reference) = &call.call.definition_ref else {
        return crate::tool::call_name::unbound(db, batch, &call.call);
    };
    let registry =
        crate::tool::registry(db, batch.batch.registry_snapshot_ref.tool_registry_snapshot_id)?
            .ok_or(Error::Conflict)?;
    Ok(registry
        .catalog_entries
        .iter()
        .find(|e| e.tool_id == reference.tool_id && e.tool_version == reference.tool_version)
        .ok_or(Error::Conflict)?
        .canonical_name
        .to_string())
}

fn artifact_id(result: &ToolResultV1) -> StoreResult<ArtifactId> {
    result.model_visible_result_ref.ok_or(Error::Conflict)
}

fn body_text(db: &Connection, result: &ToolResultV1, name: &str) -> StoreResult<String> {
    let raw = artifact::read(db, artifact_id(result)?, &result.tool_call_id.to_string())?;
    let envelope =
        serde_json::from_slice::<aura_tool_contract::ModelVisibleToolResultEnvelopeV1>(&raw)?;
    Ok(crate::agent::model_context::result_text::model_text(name, &envelope))
}

fn details(
    call: CallExecution,
    name: String,
    result: String,
) -> StoreResult<branch::CarriedToolResult> {
    let terminal = call.result.ok_or(Error::Conflict)?;
    let input = match &call.call.input {
        NormalizedToolCallInputStateV1::Ready { arguments, .. } => {
            serde_json::to_string(arguments)?
        }
        input => serde_json::to_string(input)?,
    };
    Ok(branch::CarriedToolResult {
        tool_call_id: terminal.tool_call_id,
        tool_name: name,
        input: Some(input),
        result,
        is_error: !matches!(terminal.outcome, ToolResultOutcomeV1::Succeeded),
    })
}

/// A branch copies the whole result of every call it carries.
pub(crate) fn whole_tool_result(
    db: &Connection,
    session: SessionId,
    id: ToolCallId,
) -> StoreResult<branch::CarriedToolResult> {
    let (call, name) = scoped_call(db, session, id)?;
    let terminal = call.result.as_ref().ok_or(Error::Conflict)?;
    let artifact_hash = artifact::identity(db, artifact_id(terminal)?, &id.to_string())?;
    let text = match index(db, id, &artifact_hash)? {
        Some(body) => {
            history::stored_body_window(db, &body, 0, u64::MAX)?.ok_or(Error::Conflict)?.text
        }
        None => body_text(db, terminal, &name)?,
    };
    details(call, name, text)
}

fn unreadable(db: &Connection, id: ToolCallId, error: Error) -> Error {
    match error {
        Error::Sqlite(rusqlite::Error::QueryReturnedNoRows) => Error::Invalid(
            "context.read_tool_result.tool_call_id: no tool call of this conversation has this id; copy the id exactly from the call whose result you want",
        ),
        Error::Conflict => match execution::call(db, id) {
            Ok(call) if call.result.is_none() => {
                Error::Invalid("context.read_tool_result.unfinished")
            }
            _ => Error::Conflict,
        },
        error => error,
    }
}
