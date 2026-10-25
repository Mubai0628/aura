//! The newest
//! request of a conversation its provider counted, so the next request's estimate can count the
//! parts it sends again at the provider's rate. A step that failed has no count; the one before
//! it is used, so one failed step does not throw the conversation back to byte counting.
use aura_context_domain::ContextItemEstimate;
use aura_domain_common::*;
use aura_model_domain::UsageSource;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, OptionalExtension, params};

/// How far back a counted request is looked for.
const STEPS_BACK: u32 = 8;

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
    let mut query = db.prepare(
        "SELECT c.id, s.id FROM context_snapshots c \
         JOIN model_execution_snapshots e ON json_extract(e.payload,'$.context_snapshot_id')=c.id \
         JOIN model_steps s ON s.snapshot_id=e.id \
         WHERE c.session_id=?1 ORDER BY c.rowid DESC LIMIT ?2",
    )?;
    let steps = query
        .query_map(params![session.to_string(), STEPS_BACK], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(query);
    for (context, step) in steps {
        let step: ModelStepId = step.parse().map_err(|_| Error::Conflict)?;
        // A step without an answer — failed, stopped or still running — has no count.
        let Ok(output) = super::history::output(db, step) else { continue };
        if output.usage.source != UsageSource::ProviderReported {
            continue;
        }
        let sent = crate::model::step_snapshot(db, step)?;
        let parts: Option<String> = db
            .query_row(
                "SELECT json_extract(payload,'$.budget.item_estimates') FROM context_snapshots \
                 WHERE id=?1",
                [context],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let Some(parts) = parts else { continue };
        return Ok(Some(CountedRequest {
            input_tokens: output.usage.input_tokens,
            provider_id: sent.provider_id,
            model_name: sent.model_name,
            parts: serde_json::from_str(&parts)?,
        }));
    }
    Ok(None)
}
