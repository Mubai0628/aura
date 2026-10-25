use super::facts;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentSessionState {
    pub session_id: SessionId,
    pub revision: Revision,
    pub active_turn: Option<TurnTaskId>,
    pub active_lineage: Option<TaskLineageId>,
    pub open_lineages: Vec<TaskLineageId>,
    pub pending_user_event_ids: Vec<EventId>,
}
pub fn load(db: &Connection, session: SessionId) -> StoreResult<AgentSessionState> {
    let state: AgentSessionState =
        facts::latest(db, &format!("agent-session:{session}"), "agent.session_state")?.unwrap_or(
            AgentSessionState {
                session_id: session,
                revision: Revision::INITIAL,
                active_turn: None,
                active_lineage: None,
                open_lineages: vec![],
                pending_user_event_ids: vec![],
            },
        );
    if state.session_id != session || state.revision.value() == 0 {
        return Err(Error::Conflict);
    }
    Ok(state)
}
pub fn save(
    tx: &Transaction<'_>,
    previous: &AgentSessionState,
    next: &AgentSessionState,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if load(tx, previous.session_id)? != *previous
        || previous.session_id != next.session_id
        || next.revision.value()
            != previous.revision.value().checked_add(1).ok_or(Error::Conflict)?
        || next.open_lineages.iter().collect::<std::collections::BTreeSet<_>>().len()
            != next.open_lineages.len()
        || next.active_lineage.is_some_and(|id| !next.open_lineages.contains(&id))
        || next.pending_user_event_ids.iter().collect::<std::collections::BTreeSet<_>>().len()
            != next.pending_user_event_ids.len()
        || (next.active_turn.is_none() && next.active_lineage.is_some())
    {
        return Err(Error::Conflict);
    }
    for event in &next.pending_user_event_ids {
        user_message(tx, next.session_id, *event)?;
    }
    for id in &next.open_lineages {
        let lineage = super::lineage::load(tx, *id)?;
        if lineage.session_id != next.session_id
            || matches!(
                lineage.state,
                aura_agent_domain::TaskLineageState::Completed
                    | aura_agent_domain::TaskLineageState::CancelledByUser
                    | aura_agent_domain::TaskLineageState::Expired
            )
        {
            return Err(Error::Conflict);
        }
    }
    if let Some(id) = next.active_turn {
        let turn = super::turns::load(tx, id)?;
        if turn.session_id != next.session_id
            || if turn.lineage_resolution_required {
                next.active_lineage.is_some()
            } else {
                Some(turn.lineage_id) != next.active_lineage
            }
            || turn.state.terminal()
        {
            return Err(Error::Conflict);
        }
    }
    facts::append(
        tx,
        EventId::new(),
        &format!("agent-session:{}", next.session_id),
        "agent.session_state",
        next,
        at,
    )
}
pub fn add_user_message(
    tx: &Transaction<'_>,
    message: &aura_agent_domain::OriginalUserMessage,
) -> StoreResult<()> {
    message.validate().map_err(|_| Error::Invalid("original user message"))?;
    if crate::workspace::session(tx, message.session_id)?.is_none() {
        return Err(Error::Conflict);
    }
    facts::append(
        tx,
        message.event_id,
        &format!("agent-user:{}", message.session_id),
        "agent.user_message",
        message,
        message.at,
    )?;
    // Identity, digest and length live apart from the text. A page reads this row and the
    // chunks it needs, and does not decode the payload that still holds the authority.
    facts::append(
        tx,
        EventId::new(),
        &message.event_id.to_string(),
        "agent.user_message_head",
        &UserMessageHead {
            event_id: message.event_id,
            session_id: message.session_id,
            origin: message.origin.clone(),
            at: message.at,
            content_hash: message.content_hash.clone(),
            text_bytes: message.text.len() as u64,
        },
        message.at,
    )?;
    // The message payload above stays the authority for the text. The chunks are a read path
    // for it, so a preview or a page of a long user message reads the bytes it asks for
    // instead of parsing the whole payload — the same mechanism a sealed response uses.
    super::history::record_chunks(
        tx,
        &super::history::ChunkBody::user_message(message.event_id, &message.content_hash),
        &message.text,
        message.at,
    )
}

/// What a caller needs to order and measure user messages, without their text.
///
/// A history page shows a bounded preview and a total length for each message. Reading every
/// payload to compute those means parsing the whole conversation to answer for one page, so
/// the ordering pass reads only the small facts and the text is fetched per message by the
/// caller that needs it.
pub struct UserMessageSummary {
    pub event_id: EventId,
    pub session_id: SessionId,
    pub origin: aura_agent_domain::UserMessageOrigin,
    pub at: UtcTimestamp,
    /// The text's byte length, taken from the stored chunk index when the message has one.
    pub total_bytes: u64,
}

pub fn ordered_user_summaries(
    db: &Connection,
    session: SessionId,
    ids: impl IntoIterator<Item = EventId>,
) -> StoreResult<Vec<UserMessageSummary>> {
    let mut seen = std::collections::BTreeSet::new();
    let mut values = Vec::new();
    for id in ids {
        if !seen.insert(id) {
            continue;
        }
        let Some(head) = user_message_head(db, session, id)? else { continue };
        let sequence: i64 =
            db.query_row("SELECT sequence FROM events WHERE event_id=?1", [id.to_string()], |r| {
                r.get(0)
            })?;
        values.push((
            sequence,
            UserMessageSummary {
                event_id: id,
                session_id: session,
                origin: head.origin,
                at: head.at,
                total_bytes: head.text_bytes,
            },
        ));
    }
    values.sort_by_key(|(sequence, _)| *sequence);
    Ok(values.into_iter().map(|(_, summary)| summary).collect())
}

/// The identity and content digest of one user message, without its text.
///
/// A reader that wants a bounded window needs to know *which* body it is reading and what that
/// body hashes to — so an index describing older content is refused — but not the text itself.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UserMessageHead {
    event_id: EventId,
    session_id: SessionId,
    origin: aura_agent_domain::UserMessageOrigin,
    at: UtcTimestamp,
    content_hash: ContentHash,
    text_bytes: u64,
}

pub struct UserMessageSource {
    pub event_id: EventId,
    pub session_id: SessionId,
    pub content_hash: ContentHash,
}

pub fn user_message_source(
    db: &Connection,
    session: SessionId,
    id: EventId,
) -> StoreResult<UserMessageSource> {
    let head = user_message_head(db, session, id)?
        .ok_or(Error::Sqlite(rusqlite::Error::QueryReturnedNoRows))?;
    Ok(UserMessageSource { event_id: id, session_id: session, content_hash: head.content_hash })
}

fn user_message_head(
    db: &Connection,
    session: SessionId,
    id: EventId,
) -> StoreResult<Option<UserMessageHead>> {
    let raw: Option<String> = db
        .query_row(
            "SELECT payload FROM events WHERE owner=?1 AND kind='agent.user_message_head' \
             AND version=1",
            [id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(raw) = raw {
        let head: UserMessageHead = aura_persistence_core::record::decode(&raw)?;
        if head.event_id != id || head.session_id != session {
            return Err(Error::Conflict);
        }
        return Ok(Some(head));
    }
    // A message stored before the head existed still has its payload. Pull only the small
    // fields out of it; the text stays in SQLite and is not returned to this process.
    let extracted: Option<(String, String, String, String, i64, String)> = db
        .query_row(
            "SELECT json_extract(payload,'$.event_id'), json_extract(payload,'$.session_id'), \
             json_extract(payload,'$.origin'), json_extract(payload,'$.at'), \
             COALESCE(length(CAST(json_extract(payload,'$.text') AS BLOB)), 0), \
             json_extract(payload,'$.content_hash') \
             FROM events WHERE event_id=?1 AND owner=?2 AND kind='agent.user_message' AND version=1",
            rusqlite::params![id.to_string(), format!("agent-user:{session}")],
            |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
            },
        )
        .optional()?;
    let Some((event_id, session_id, origin, at, text_bytes, content_hash)) = extracted else {
        return Ok(None);
    };
    let head = UserMessageHead {
        event_id: event_id.parse().map_err(|_| Error::Conflict)?,
        session_id: session_id.parse().map_err(|_| Error::Conflict)?,
        origin: serde_json::from_str(&origin).map_err(|_| Error::Conflict)?,
        at: at.parse().map_err(|_| Error::Conflict)?,
        content_hash: serde_json::from_str(&content_hash).map_err(|_| Error::Conflict)?,
        text_bytes: u64::try_from(text_bytes).map_err(|_| Error::Conflict)?,
    };
    if head.event_id != id || head.session_id != session {
        return Err(Error::Conflict);
    }
    Ok(Some(head))
}

/// Up to `chars` characters of one user message's text, taken from the stored payload.
///
/// The payload the message was written with is still there, and for a message stored before
/// the chunk index existed it is the only copy. SQLite does the truncation, so the bytes that
/// reach this process are the bounded prefix rather than the whole message and whatever the
/// JSON decoder would have allocated for it. The count is in characters, which is how SQLite
/// counts `substr` over text, so the result always ends on a character boundary.
pub fn user_message_prefix(
    db: &Connection,
    session: SessionId,
    id: EventId,
    chars: usize,
) -> StoreResult<String> {
    let raw: Option<String> = db
        .query_row(
            "SELECT substr(json_extract(payload,'$.text'), 1, ?3) FROM events \
             WHERE event_id=?1 AND owner=?2 AND kind='agent.user_message' AND version=1",
            rusqlite::params![
                id.to_string(),
                format!("agent-user:{session}"),
                i64::try_from(chars).map_err(|_| Error::Conflict)?
            ],
            |row| row.get(0),
        )
        .optional()?;
    let Some(prefix) = raw else {
        return Err(Error::Sqlite(rusqlite::Error::QueryReturnedNoRows));
    };
    Ok(prefix)
}

pub fn ordered_messages(
    db: &Connection,
    session: SessionId,
    ids: impl IntoIterator<Item = EventId>,
) -> StoreResult<Vec<aura_agent_domain::OriginalUserMessage>> {
    let mut seen = std::collections::BTreeSet::new();
    let mut values = Vec::new();
    for id in ids {
        if !seen.insert(id) {
            continue;
        }
        let message = user_message(db, session, id)?;
        let sequence: i64 =
            db.query_row("SELECT sequence FROM events WHERE event_id=?1", [id.to_string()], |r| {
                r.get(0)
            })?;
        values.push((sequence, message));
    }
    values.sort_by_key(|(sequence, _)| *sequence);
    Ok(values.into_iter().map(|(_, message)| message).collect())
}
pub fn user_message(
    db: &Connection,
    session: SessionId,
    id: EventId,
) -> StoreResult<aura_agent_domain::OriginalUserMessage> {
    let message: aura_agent_domain::OriginalUserMessage =
        facts::exact(db, id, &format!("agent-user:{session}"), "agent.user_message")?;
    message.validate().map_err(|_| Error::Invalid("original user message"))?;
    if message.event_id != id || message.session_id != session {
        return Err(Error::Conflict);
    }
    Ok(message)
}

/// `Ok(None)` is a missing user message. Any other read failure stays an error.
pub fn user_text(db: &Connection, session: SessionId, id: EventId) -> StoreResult<Option<String>> {
    match user_message(db, session, id) {
        Ok(message) => Ok(Some(message.text)),
        Err(Error::Sqlite(rusqlite::Error::QueryReturnedNoRows)) => Ok(None),
        Err(error) => Err(error),
    }
}
