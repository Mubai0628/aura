use crate::{AgentSessionPolicy, AgentSessionPolicySetRequest};
use aura_domain_common::{SessionId, UtcTimestamp};
use aura_persistence_core::{Connection, PersistenceCoreError as Error, StoreResult, Transaction};
use rusqlite::{OptionalExtension, params};

pub fn load(db: &Connection, session: SessionId) -> StoreResult<AgentSessionPolicy> {
    let row: Option<(i64, String)> = db
        .query_row(
            "SELECT revision,payload FROM agent_collaboration_session_policy WHERE session_id=?1",
            [session.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((revision, raw)) = row else {
        return Ok(AgentSessionPolicy {
            session_id: session,
            allow_proactive_temporary_agents: false,
            revision: 0,
        });
    };
    let value: AgentSessionPolicy = serde_json::from_str(&raw)?;
    if value.session_id != session || u64::try_from(revision).ok() != Some(value.revision) {
        return Err(Error::Invalid("session agent policy"));
    }
    Ok(value)
}

pub fn set(
    tx: &Transaction<'_>,
    input: &AgentSessionPolicySetRequest,
    at: UtcTimestamp,
) -> StoreResult<AgentSessionPolicy> {
    let current = load(tx, input.session_id)?;
    if current.revision != input.expected_revision {
        return Err(Error::Conflict);
    }
    if current.allow_proactive_temporary_agents == input.allow_proactive_temporary_agents {
        return Ok(current);
    }
    let next = AgentSessionPolicy {
        session_id: input.session_id,
        allow_proactive_temporary_agents: input.allow_proactive_temporary_agents,
        revision: current.revision.checked_add(1).ok_or(Error::Conflict)?,
    };
    let changed = tx.execute(
        "INSERT INTO agent_collaboration_session_policy(session_id,revision,payload,updated_at) VALUES(?1,?2,?3,?4) ON CONFLICT(session_id) DO UPDATE SET revision=excluded.revision,payload=excluded.payload,updated_at=excluded.updated_at WHERE agent_collaboration_session_policy.revision=?5",
        params![next.session_id.to_string(), i64::try_from(next.revision).map_err(|_| Error::Conflict)?, serde_json::to_string(&next)?, at.to_string(), i64::try_from(input.expected_revision).map_err(|_| Error::Conflict)?],
    )?;
    if changed != 1 {
        return Err(Error::Conflict);
    }
    Ok(next)
}
