//! Skills facts. The Skills a user chose with a message are kept with that message for its
//! history, written out whole for it when it was sent; what a run's first step decided about the
//! pinned Skills and the list of names is kept for its other steps. Runs recorded before Skills
//! were loaded by name kept their loaded set; it is still read for those runs.
use super::facts;
use aura_agent_domain::LoadedSkillEntry;
use aura_domain_common::{AgentRunId, EventId, SessionId, SkillInstanceId, UtcTimestamp};
use aura_persistence_core::StoreResult;
use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};

const KIND: &str = "agent.skills_loaded";
const SELECTED: &str = "agent.skills_selected";
const CHOSEN_TEXT: &str = "agent.skills_chosen_text";
const TURN: &str = "agent.skills_turn";

/// A run's Skills ahead of AGENTS.md: the pinned Skills it sends whole, in order, and the list
/// of the other names as sent.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TurnSkills {
    pub pinned: Vec<SkillInstanceId>,
    pub list: Option<String>,
}

/// What this run's first step decided, if it has.
pub fn turn_skills(db: &Connection, run: AgentRunId) -> StoreResult<Option<TurnSkills>> {
    facts::latest::<TurnSkills>(db, &run.to_string(), TURN)
}

/// Records what a run's first step decided.
pub fn set_turn_skills(
    tx: &Transaction<'_>,
    run: AgentRunId,
    skills: &TurnSkills,
    at: UtcTimestamp,
) -> StoreResult<()> {
    facts::append(tx, EventId::new(), &run.to_string(), TURN, skills, at)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LoadedSkills {
    skills: Vec<LoadedSkillEntry>,
}

/// The Skills chosen with one message, written out as they are added to it. The Session is named
/// so a queued message that never became part of the conversation goes with it.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ChosenTexts {
    session_id: SessionId,
    texts: Vec<String>,
}

/// The Skills an earlier run loaded, in load order; none for a run since.
pub fn loaded(db: &Connection, run: AgentRunId) -> StoreResult<Vec<LoadedSkillEntry>> {
    Ok(facts::latest::<LoadedSkills>(db, &run.to_string(), KIND)?
        .map(|value| value.skills)
        .unwrap_or_default())
}

/// The Skills chosen with a message, written out when it was sent; none for a message without.
pub fn chosen_texts(db: &Connection, message: EventId) -> StoreResult<Vec<String>> {
    Ok(facts::latest::<ChosenTexts>(db, &message.to_string(), CHOSEN_TEXT)?
        .map(|value| value.texts)
        .unwrap_or_default())
}

/// Records the Skills chosen with a message as they are added to it, when it is sent.
pub fn set_chosen_texts(
    tx: &Transaction<'_>,
    message: EventId,
    session: SessionId,
    texts: &[String],
    at: UtcTimestamp,
) -> StoreResult<()> {
    facts::append(
        tx,
        EventId::new(),
        &message.to_string(),
        CHOSEN_TEXT,
        &ChosenTexts { session_id: session, texts: texts.to_vec() },
        at,
    )
}

/// The Skills the user chose with this message, as they were when its turn started.
pub fn selected(db: &Connection, user_event: EventId) -> StoreResult<Vec<LoadedSkillEntry>> {
    Ok(facts::latest::<LoadedSkills>(db, &user_event.to_string(), SELECTED)?
        .map(|value| value.skills)
        .unwrap_or_default())
}

/// Records the Skills chosen with one message when its turn starts.
pub fn set_selected(
    tx: &Transaction<'_>,
    user_event: EventId,
    skills: &[LoadedSkillEntry],
    at: UtcTimestamp,
) -> StoreResult<()> {
    facts::append(
        tx,
        EventId::new(),
        &user_event.to_string(),
        SELECTED,
        &LoadedSkills { skills: skills.to_vec() },
        at,
    )
}
