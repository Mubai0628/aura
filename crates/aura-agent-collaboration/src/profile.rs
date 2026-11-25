use crate::*;
use aura_domain_common::{AgentRunId, SessionId, UtcTimestamp, WorkspaceId};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

pub struct Identity<'a> {
    pub session: SessionId,
    pub workspace: WorkspaceId,
    pub name: &'a str,
}

pub fn load(
    db: &Connection,
    session: SessionId,
    workspace: WorkspaceId,
    name: &str,
) -> StoreResult<AgentProfile> {
    let raw: Option<String> = db
        .query_row(
            "SELECT payload FROM agent_collaboration_profiles WHERE session_id=?1",
            [session.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    let mut profile = match raw {
        Some(raw) => serde_json::from_str(&raw)?,
        None => AgentProfile {
            session_id: session,
            workspace_id: workspace,
            name: name.into(),
            description: String::new(),
            description_source: AgentDescriptionSource::None,
            revision: 0,
            updated_at: None,
            input_tokens: 0,
            output_tokens: 0,
            usage_complete: true,
            generation: None,
        },
    };
    if profile.session_id != session || profile.workspace_id != workspace {
        return Err(Error::Conflict);
    }
    profile.name = name.into();
    profile.input_tokens = 0;
    profile.output_tokens = 0;
    profile.usage_complete = true;
    let mut rows = db.prepare(
        "SELECT payload FROM agent_collaboration_generations WHERE session_id=?1 ORDER BY created_at, rowid",
    )?;
    for raw in rows.query_map([session.to_string()], |row| row.get::<_, String>(0))? {
        let generation: AgentProfileGeneration = serde_json::from_str(&raw?)?;
        profile.input_tokens = profile
            .input_tokens
            .checked_add(generation.input_tokens)
            .ok_or(Error::Invalid("profile usage overflow"))?;
        profile.output_tokens = profile
            .output_tokens
            .checked_add(generation.output_tokens)
            .ok_or(Error::Invalid("profile usage overflow"))?;
        profile.usage_complete &= generation.usage_complete;
        profile.generation = Some(generation);
    }
    Ok(profile)
}

pub fn validate_description(description: &str) -> StoreResult<()> {
    if description.chars().count() > MAX_DESCRIPTION_CHARS
        || description.chars().any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
    {
        return Err(Error::Invalid("agent profile description"));
    }
    Ok(())
}

pub fn set_user(
    tx: &Transaction<'_>,
    workspace: WorkspaceId,
    name: &str,
    request: &AgentProfileSetRequest,
    at: UtcTimestamp,
) -> StoreResult<AgentProfile> {
    validate_description(&request.description)?;
    let mut current = load(tx, request.session_id, workspace, name)?;
    if current.revision != request.expected_revision {
        return Err(Error::Conflict);
    }
    current.description = request.description.clone();
    current.description_source = AgentDescriptionSource::User;
    current.revision = current.revision.checked_add(1).ok_or(Error::Conflict)?;
    current.updated_at = Some(at);
    save(tx, &current, request.expected_revision)?;
    load(tx, request.session_id, workspace, name)
}

fn save(tx: &Transaction<'_>, profile: &AgentProfile, previous: u64) -> StoreResult<()> {
    let raw = serde_json::to_string(profile)?;
    let revision = i64::try_from(profile.revision).map_err(|_| Error::Conflict)?;
    let changed = if previous == 0 {
        tx.execute(
            "INSERT INTO agent_collaboration_profiles(session_id,revision,payload) VALUES(?1,?2,?3) ON CONFLICT(session_id) DO NOTHING",
            params![profile.session_id.to_string(), revision, raw],
        )?
    } else {
        tx.execute(
            "UPDATE agent_collaboration_profiles SET revision=?2,payload=?3 WHERE session_id=?1 AND revision=?4",
            params![profile.session_id.to_string(), revision, raw, i64::try_from(previous).map_err(|_| Error::Conflict)?],
        )?
    };
    if changed == 1 { Ok(()) } else { Err(Error::Conflict) }
}

/// A run is reserved before any model request, so repeated completion notices cannot bill twice.
pub fn reserve_generation(
    tx: &Transaction<'_>,
    session: SessionId,
    base_revision: u64,
    record: &AgentProfileGeneration,
    at: UtcTimestamp,
) -> StoreResult<bool> {
    reserve_generation_as(tx, &record.source_run_id.to_string(), session, base_revision, record, at)
}

pub fn reserve_generation_as(
    tx: &Transaction<'_>,
    key: &str,
    session: SessionId,
    base_revision: u64,
    record: &AgentProfileGeneration,
    at: UtcTimestamp,
) -> StoreResult<bool> {
    if record.state != AgentProfileGenerationState::Running {
        return Err(Error::Conflict);
    }
    Ok(tx.execute(
        "INSERT INTO agent_collaboration_generations(id,source_run_id,session_id,base_revision,state,payload,created_at,updated_at) VALUES(?6,?1,?2,?3,'running',?4,?5,?5) ON CONFLICT DO NOTHING",
        params![record.source_run_id.to_string(), session.to_string(), i64::try_from(base_revision).map_err(|_| Error::Conflict)?, serde_json::to_string(record)?, at.to_string(), key],
    )? == 1)
}

/// Late generated text cannot overwrite either a user's edit or a newer generation.
/// Its actual usage is retained even when its text is superseded or generation failed.
pub fn finish_generation(
    tx: &Transaction<'_>,
    session: SessionId,
    workspace: WorkspaceId,
    name: &str,
    description: Option<&str>,
    record: &AgentProfileGeneration,
    at: UtcTimestamp,
) -> StoreResult<bool> {
    finish_generation_as(
        tx,
        &record.source_run_id.to_string(),
        Identity { session, workspace, name },
        description,
        record,
        at,
    )
}

pub fn finish_generation_as(
    tx: &Transaction<'_>,
    key: &str,
    identity: Identity<'_>,
    description: Option<&str>,
    record: &AgentProfileGeneration,
    at: UtcTimestamp,
) -> StoreResult<bool> {
    let Identity { session, workspace, name } = identity;
    if record.state == AgentProfileGenerationState::Running {
        return Err(Error::Conflict);
    }
    if let Some(text) = description {
        validate_description(text)?;
    }
    if let Some(preview) = &record.preview {
        validate_description(&preview.description)?;
    }
    let (bound_session, base_revision, state, raw): (String, i64, String, String) = tx.query_row(
        "SELECT session_id,base_revision,state,payload FROM agent_collaboration_generations WHERE id=?1",
        [key], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
    )?;
    let reserved: AgentProfileGeneration = serde_json::from_str(&raw)?;
    if bound_session != session.to_string()
        || reserved.source_turn_id != record.source_turn_id
        || reserved.source_run_id != record.source_run_id
    {
        return Err(Error::Conflict);
    }
    if state != "running" {
        return Ok(false);
    }
    let mut profile = load(tx, session, workspace, name)?;
    let applicable = profile.revision
        == u64::try_from(base_revision).map_err(|_| Error::Conflict)?
        && profile.description_source != AgentDescriptionSource::User;
    let mut finished = record.clone();
    if !applicable
        && finished.state == AgentProfileGenerationState::Completed
        && finished.preview.is_none()
        && description.is_some()
    {
        finished.state = AgentProfileGenerationState::Superseded;
        finished.message = Some("生成期间简介已被修改；保留当前内容，生成用量仍已记录。".into());
    }
    tx.execute(
        "UPDATE agent_collaboration_generations SET state=?2,payload=?3,updated_at=?4 WHERE id=?1 AND state='running'",
        params![key, serde_json::to_value(finished.state)?.as_str(), serde_json::to_string(&finished)?, at.to_string()],
    )?;
    if applicable
        && finished.state == AgentProfileGenerationState::Completed
        && let Some(text) = description
        && text != profile.description
    {
        let previous = profile.revision;
        profile.description = text.into();
        profile.description_source = AgentDescriptionSource::Model;
        profile.revision = previous.checked_add(1).ok_or(Error::Conflict)?;
        profile.updated_at = Some(at);
        save(tx, &profile, previous)?;
        return Ok(true);
    }
    Ok(false)
}

pub fn interrupt_generations(tx: &Transaction<'_>, at: UtcTimestamp) -> StoreResult<()> {
    let mut statement =
        tx.prepare("SELECT id,payload FROM agent_collaboration_generations WHERE state='running'")?;
    let rows = statement
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    for (id, raw) in rows {
        let mut generation: AgentProfileGeneration = serde_json::from_str(&raw)?;
        generation.state = AgentProfileGenerationState::Interrupted;
        generation.usage_complete = false;
        generation.message =
            Some("上次简介生成被中断，未覆盖旧简介；这次不会重放，后续轮次可重新生成。".into());
        tx.execute("UPDATE agent_collaboration_generations SET state='interrupted',payload=?2,updated_at=?3 WHERE id=?1", params![id,serde_json::to_string(&generation)?,at.to_string()])?;
    }
    Ok(())
}

pub fn generation_exists(db: &Connection, run: AgentRunId) -> StoreResult<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_collaboration_generations WHERE id=?1)",
        [run.to_string()],
        |r| r.get(0),
    )?)
}

/// A failed worker leaves an incomplete-usage fact rather than a permanently running row.
pub fn fail_unfinished(tx: &Transaction<'_>, key: &str, at: UtcTimestamp) -> StoreResult<()> {
    let raw: Option<String> = tx
        .query_row(
            "SELECT payload FROM agent_collaboration_generations WHERE id=?1 AND state='running'",
            [key],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(raw) = raw {
        let mut generation: AgentProfileGeneration = serde_json::from_str(&raw)?;
        generation.state = AgentProfileGenerationState::Failed;
        generation.usage_complete = false;
        generation.message =
            Some("简介生成未正常结算，保留旧简介；用量可能不完整，可重新生成建议。".into());
        tx.execute(
            "UPDATE agent_collaboration_generations SET state='failed',payload=?2,updated_at=?3 WHERE id=?1 AND state='running'",
            params![key, serde_json::to_string(&generation)?, at.to_string()],
        )?;
    }
    Ok(())
}
