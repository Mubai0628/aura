//! Read exact delivery requirements from original, active sources, including after compaction.
use aura_agent_domain::{
    AgentTaskMode, ExactArtifactValue, OriginalUserMessage, SourceMembership, TurnTask,
    UserMessageOrigin, apply_exact_artifact_values, explicit_title, latest_title_only_target,
};
use aura_domain_common::{EventId, SessionId, TurnTaskId};
use aura_persistence_core::{PersistenceCoreError, StoreResult};
use rusqlite::Connection;
use std::collections::BTreeSet;

pub fn requirements(db: &Connection, turn: &TurnTask) -> StoreResult<Vec<ExactArtifactValue>> {
    if turn.mode != AgentTaskMode::Normal {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    let mut active_title = None;
    // Journal order also covers a typed answer followed by a steer. Read one body at a time.
    for id in source_ids(db, turn)? {
        let message = super::session::user_message(db, turn.session_id, id)?;
        visit_texts(&message, |text| {
            apply_source(db, turn.session_id, id, text, &mut active_title, &mut result)
        })?;
    }
    Ok(result)
}

fn source_ids(db: &Connection, turn: &TurnTask) -> StoreResult<Vec<EventId>> {
    let lineage = super::lineage::load(db, turn.lineage_id)?;
    let mut ids: BTreeSet<_> = super::query::events(db, turn.turn_task_id)?.into_iter().collect();
    for source in &lineage.sources {
        if source.membership == SourceMembership::Active {
            ids.insert(source.user_event_id);
        } else {
            ids.remove(&source.user_event_id);
        }
    }
    let sources = super::session::ordered_user_summaries(db, turn.session_id, ids.iter().copied())?;
    if sources.len() != ids.len() {
        return Err(PersistenceCoreError::Conflict);
    }
    Ok(sources.into_iter().map(|source| source.event_id).collect())
}

fn visit_texts(
    message: &OriginalUserMessage,
    mut visit: impl FnMut(&str) -> StoreResult<()>,
) -> StoreResult<()> {
    match message.origin {
        UserMessageOrigin::UserInput | UserMessageOrigin::Steer => visit(&message.text),
        UserMessageOrigin::QueryAnswer { .. } => {
            let reply: serde_json::Value = serde_json::from_str(&message.text)?;
            for answer in reply["query_reply"]["answers"].as_array().into_iter().flatten() {
                let value = &answer["reply"];
                if matches!(value["kind"].as_str(), Some("custom" | "text"))
                    && let Some(text) = value["text"].as_str()
                {
                    visit(text)?;
                }
            }
            Ok(())
        }
    }
}

fn apply_source(
    db: &Connection,
    session: SessionId,
    event: EventId,
    text: &str,
    active_title: &mut Option<String>,
    values: &mut Vec<ExactArtifactValue>,
) -> StoreResult<()> {
    if let Some(title) = explicit_title(text) {
        *active_title = Some(title.to_owned());
    }
    if let Some(path) = latest_title_only_target(text) {
        let title = match active_title {
            Some(title) => Some(title.clone()),
            None => prior_title(db, session, event)?,
        };
        if let Some(title) = title {
            ExactArtifactValue::set_title_only(values, path, title);
        }
    }
    apply_exact_artifact_values(values, text);
    Ok(())
}

fn prior_title(
    db: &Connection,
    session: SessionId,
    before: EventId,
) -> StoreResult<Option<String>> {
    let mut statement = db.prepare(
        "SELECT t.id FROM turn_tasks t
         JOIN events e ON e.event_id=json_extract(t.payload,'$.root_user_event_id')
         WHERE t.session_id=?1 AND t.state='completed'
         AND e.sequence < (SELECT sequence FROM events WHERE event_id=?2)
         ORDER BY e.sequence DESC LIMIT 32",
    )?;
    let mut rows = statement.query([session.to_string(), before.to_string()])?;
    while let Some(row) = rows.next()? {
        let id = row
            .get::<_, String>(0)?
            .parse::<TurnTaskId>()
            .map_err(|_| PersistenceCoreError::Conflict)?;
        let past = super::turns::load(db, id)?;
        for source in source_ids(db, &past)?.into_iter().rev() {
            let message = super::session::user_message(db, session, source)?;
            let mut title = None;
            visit_texts(&message, |text| {
                if let Some(value) = explicit_title(text) {
                    title = Some(value.to_owned());
                }
                Ok(())
            })?;
            if title.is_some() {
                return Ok(title);
            }
        }
    }
    Ok(None)
}
