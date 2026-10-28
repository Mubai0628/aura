//! Rounds from the session's turns: a round begins with the first turn and with every turn after
//! a completed one.
use super::rows::Position;
use aura_agent_domain::{TurnTask, TurnTaskState};
use aura_domain_common::{EventId, SessionId, UtcTimestamp};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
use rusqlite::{Connection, params};

/// One round: its number, its first message and where it lies in the conversation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Round {
    pub number: u32,
    pub sent_at: UtcTimestamp,
    pub first_message: String,
    /// Where its first message is, and where the next round's is.
    pub(super) start: Position,
    pub(super) end: Option<Position>,
}

/// A turn as rounds see it: its first message and whether it completed.
#[derive(Clone, Debug)]
pub(super) struct Head {
    pub completed: bool,
    pub position: Position,
    pub sent_at: UtcTimestamp,
    pub text: String,
}

pub(super) fn rounds(db: &Connection, session: SessionId) -> StoreResult<Vec<Round>> {
    Ok(group(&heads(db, session)?))
}

/// Turns in conversation order, grouped into rounds.
pub(super) fn group(heads: &[Head]) -> Vec<Round> {
    let mut rounds: Vec<Round> = Vec::new();
    let mut begins = true;
    for head in heads {
        if begins {
            if let Some(previous) = rounds.last_mut() {
                previous.end = Some(head.position);
            }
            rounds.push(Round {
                number: u32::try_from(rounds.len() + 1).unwrap_or(u32::MAX),
                sent_at: head.sent_at,
                first_message: head.text.clone(),
                start: head.position,
                end: None,
            });
        }
        begins = head.completed;
    }
    rounds
}

/// The session's turns, carried ones first, each by its first message.
fn heads(db: &Connection, session: SessionId) -> StoreResult<Vec<Head>> {
    let mut heads: Vec<Head> = crate::agent::branch::carried_heads(db, session)?
        .into_iter()
        .map(|carried| Head {
            completed: carried.completed,
            position: (0, carried.sequence),
            sent_at: carried.sent_at,
            text: carried.text,
        })
        .collect();
    let mut query = db.prepare(
        "SELECT t.payload, e.sequence FROM turn_tasks t \
         JOIN events e ON e.event_id = json_extract(t.payload,'$.root_user_event_id') \
         WHERE t.session_id=?1",
    )?;
    let mut own = Vec::new();
    for row in query.query_map(params![session.to_string()], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })? {
        let (payload, sequence) = row?;
        let turn: TurnTask = record::decode(&payload)?;
        let sequence = u64::try_from(sequence).map_err(|_| Error::Conflict)?;
        own.push((sequence, turn));
    }
    own.sort_by_key(|(sequence, _)| *sequence);
    for (sequence, turn) in own {
        let first = first_message(db, session, turn.root_user_event_id)?;
        heads.push(Head {
            completed: matches!(turn.state, TurnTaskState::Completed { .. }),
            position: (1, sequence),
            sent_at: first.0,
            text: first.1,
        });
    }
    Ok(heads)
}

fn first_message(
    db: &Connection,
    session: SessionId,
    event: EventId,
) -> StoreResult<(UtcTimestamp, String)> {
    let message = crate::agent::session::user_message(db, session, event)?;
    Ok((message.at, message.text))
}
