//! The newest
//! request of a conversation its provider counted, so the next request's estimate can count the
//! parts it sends again at the provider's rate. A step that failed has no count; the one before
//! it is used, so one failed step does not throw the conversation back to byte counting. A new
//! conversation borrows the same model's newest count from another recent conversation.
use aura_context_domain::ContextItemEstimate;
use aura_domain_common::*;
use aura_model_domain::UsageSource;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, OptionalExtension, params};

/// How far back a counted request is looked for.
const STEPS_BACK: u32 = 8;
/// How many of the store's newest requests are searched for other conversations.
const OTHER_REQUESTS_BACK: u32 = 64;
/// How many other conversations are asked for their newest count.
const OTHER_CONVERSATIONS: usize = 4;

/// What the provider counted for one request, of which model, and the bytes of each of its parts.
#[derive(Clone, Debug)]
pub struct CountedRequest {
    pub input_tokens: u64,
    pub provider_id: ProviderId,
    pub model_name: String,
    pub parts: Vec<ContextItemEstimate>,
}

/// The newest counted request among the conversation's last steps, if any.
pub fn latest(db: &Connection, session: SessionId) -> StoreResult<Option<CountedRequest>> {
    let Some(counted) = newest_counted(db, session)? else {
        return Ok(None);
    };
    let sent = crate::model::step_snapshot(db, counted.step)?;
    Ok(Some(CountedRequest {
        input_tokens: counted.input_tokens,
        provider_id: sent.provider_id,
        model_name: sent.model_name,
        parts: serde_json::from_str(&counted.parts)?,
    }))
}

/// The snapshot behind the conversation's newest counted request: the next request is measured
/// by it, so the conversation keeps it beside its newest snapshot (`context::insert_snapshot`).
pub fn latest_counted_snapshot(db: &Connection, session: SessionId) -> StoreResult<Option<String>> {
    Ok(newest_counted(db, session)?.map(|counted| counted.context))
}

struct Counted {
    context: String,
    step: ModelStepId,
    input_tokens: u64,
    parts: String,
}

/// The conversation's last `limit` steps that were sent, newest first, with the execution snapshot
/// each was sent with. Reached through the conversation's turns, runs and steps, which are indexed:
/// matching every execution snapshot's JSON against the conversation instead read all of them,
/// 30 MB for each lookup on a store of a thousand requests.
pub(crate) fn last_sent_steps(
    db: &Connection,
    session: SessionId,
    limit: u32,
) -> StoreResult<Vec<(ModelStepId, String)>> {
    let mut query = db.prepare(
        "SELECT s.id, s.snapshot_id FROM turn_tasks t \
         JOIN agent_runs r ON r.turn_task_id=t.id \
         JOIN model_steps s ON s.agent_run_id=r.id \
         WHERE t.session_id=?1 AND s.snapshot_id IS NOT NULL ORDER BY s.rowid DESC LIMIT ?2",
    )?;
    let steps = query
        .query_map(params![session.to_string(), limit], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    steps
        .into_iter()
        .map(|(step, sent)| Ok((step.parse().map_err(|_| Error::Conflict)?, sent)))
        .collect()
}

/// The context snapshot a step was sent with, while the conversation still keeps it.
pub(crate) fn sent_context(
    db: &Connection,
    session: SessionId,
    execution_snapshot: &str,
) -> StoreResult<Option<String>> {
    Ok(db
        .query_row(
            "SELECT c.id FROM model_execution_snapshots e \
             JOIN context_snapshots c ON c.id=json_extract(e.payload,'$.context_snapshot_id') \
             WHERE e.id=?1 AND c.session_id=?2",
            params![execution_snapshot, session.to_string()],
            |row| row.get(0),
        )
        .optional()?)
}

/// Among the conversation's last steps, the newest whose provider reported its count and whose
/// snapshot, still kept, names the parts it was counted by.
fn newest_counted(db: &Connection, session: SessionId) -> StoreResult<Option<Counted>> {
    for (step, sent) in last_sent_steps(db, session, STEPS_BACK)? {
        // A step without an answer — failed, stopped or still running — has no count.
        let Ok(output) = super::history::output(db, step) else { continue };
        if output.usage.source != UsageSource::ProviderReported {
            continue;
        }
        let Some(context) = sent_context(db, session, &sent)? else { continue };
        let parts: Option<String> = db
            .query_row(
                "SELECT json_extract(payload,'$.budget.item_estimates') FROM context_snapshots \
                 WHERE id=?1",
                [&context],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let Some(parts) = parts else { continue };
        return Ok(Some(Counted { context, step, input_tokens: output.usage.input_tokens, parts }));
    }
    Ok(None)
}

/// What a request of `provider`'s `model` is measured by: the conversation's own newest count
/// when it is of this model, otherwise the same model's count from another conversation.
pub fn for_model(
    db: &Connection,
    session: SessionId,
    provider: &ProviderId,
    model: &str,
) -> StoreResult<Option<CountedRequest>> {
    match latest(db, session)? {
        Some(own) if own.provider_id == *provider && own.model_name == model => Ok(Some(own)),
        _ => latest_for_model(db, provider, model, session),
    }
}

/// The newest count of `provider`'s `model` in one of the other conversations that sent the
/// store's latest requests, for a conversation with no count of its own yet: the same model
/// counts the same parts (tools, instructions) the same way in any conversation.
pub fn latest_for_model(
    db: &Connection,
    provider: &ProviderId,
    model: &str,
    except: SessionId,
) -> StoreResult<Option<CountedRequest>> {
    let mut query =
        db.prepare("SELECT session_id FROM context_snapshots ORDER BY rowid DESC LIMIT ?1")?;
    let recent = query
        .query_map([OTHER_REQUESTS_BACK], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(query);
    let mut asked = Vec::new();
    for session in recent {
        let session: SessionId = session.parse().map_err(|_| Error::Conflict)?;
        if session == except || asked.contains(&session) {
            continue;
        }
        if asked.len() == OTHER_CONVERSATIONS {
            break;
        }
        asked.push(session);
        if let Some(counted) = latest(db, session)?
            && counted.provider_id == *provider
            && counted.model_name == model
        {
            return Ok(Some(counted));
        }
    }
    Ok(None)
}
