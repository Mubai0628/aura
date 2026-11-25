//! Latest round headers for automatic recall and the summary index. Page the text in SQLite.
use super::Round;
use aura_domain_common::{ContentHash, SessionId, UtcTimestamp};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, params};

pub(super) struct Head {
    pub round: Round,
    pub characters: u64,
    pub hash: ContentHash,
    pub more_users: bool,
}

pub(super) fn recent(
    db: &Connection,
    session: SessionId,
    count: usize,
    until: Option<UtcTimestamp>,
    chars: usize,
) -> StoreResult<Vec<Head>> {
    if crate::agent::branch::origin(db, session)?.is_some() {
        let mut heads = super::rounds(db, session)?
            .into_iter()
            .rev()
            .filter(|r| until.is_none_or(|end| r.sent_at <= end))
            .take(count)
            .map(|mut round| {
                let characters = round.first_message.chars().count() as u64;
                let hash = ContentHash::sha256(round.first_message.as_bytes());
                round.first_message = round.first_message.chars().take(chars).collect();
                Head { round, characters, hash, more_users: true }
            })
            .collect::<Vec<_>>();
        heads.reverse();
        return Ok(heads);
    }
    let owner = format!("agent-user:{session}");
    let mut query=db.prepare(
        "WITH turns AS (
            SELECT e.sequence,e.event_id,e.created_at,lag(t.state,1,'completed') OVER (ORDER BY e.sequence) prior
            FROM turn_tasks t JOIN events e ON e.event_id=json_extract(t.payload,'$.root_user_event_id')
            WHERE t.session_id=?1 AND e.kind='agent.user_message'
        ), starts AS (
            SELECT *,row_number() OVER (ORDER BY sequence) n,lead(sequence) OVER (ORDER BY sequence) finish
            FROM turns WHERE prior='completed'
        ), page AS (
            SELECT * FROM starts WHERE ?2 IS NULL OR created_at<=?2 ORDER BY sequence DESC LIMIT ?3
        ) SELECT n,page.sequence,finish,page.created_at,
            substr(json_extract(e.payload,'$.text'),1,?4),length(json_extract(e.payload,'$.text')),
            json_extract(e.payload,'$.content_hash'),
            EXISTS(SELECT 1 FROM events u WHERE u.owner=?5 AND u.kind='agent.user_message' AND u.sequence>page.sequence AND (finish IS NULL OR u.sequence<finish))
            FROM page JOIN events e ON e.event_id=page.event_id ORDER BY page.sequence"
    )?;
    let mut rows = query.query(params![
        session.to_string(),
        until.map(|t| t.to_string()),
        count as i64,
        chars as i64,
        owner
    ])?;
    let mut heads = Vec::new();
    while let Some(row) = rows.next()? {
        let start: i64 = row.get(1)?;
        let end: Option<i64> = row.get(2)?;
        heads.push(Head {
            round: Round {
                number: row.get(0)?,
                start: (1, start as u64),
                end: end.map(|v| (1, v as u64)),
                sent_at: row.get::<_, String>(3)?.parse().map_err(|_| Error::Conflict)?,
                first_message: row.get(4)?,
            },
            characters: row.get::<_, i64>(5)? as u64,
            hash: serde_json::from_str(&row.get::<_, String>(6)?)?,
            more_users: row.get(7)?,
        });
    }
    Ok(heads)
}
