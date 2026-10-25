//! The Skills loaded for one agent run. Every change appends the whole set as a fact, so
//! the set any step used can be read back. A run belongs to one turn, so loads end with it.
//! The Skills a user chose with a message are kept with that message for its history.
use super::facts;
use aura_agent_domain::LoadedSkillEntry;
use aura_domain_common::{AgentRunId, EventId, SkillInstanceId, UtcTimestamp};
use aura_persistence_core::StoreResult;
use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};

const KIND: &str = "agent.skills_loaded";
const SELECTED: &str = "agent.skills_selected";
const CHOSEN: &str = "agent.skills_chosen";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LoadedSkills {
    skills: Vec<LoadedSkillEntry>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ChosenSkills {
    skill_ids: Vec<SkillInstanceId>,
}

/// The Skills this run has loaded now, in load order.
pub fn loaded(db: &Connection, run: AgentRunId) -> StoreResult<Vec<LoadedSkillEntry>> {
    Ok(facts::latest::<LoadedSkills>(db, &run.to_string(), KIND)?
        .map(|value| value.skills)
        .unwrap_or_default())
}

/// Records the run's whole loaded set after a load or unload.
pub fn set_loaded(
    tx: &Transaction<'_>,
    run: AgentRunId,
    skills: &[LoadedSkillEntry],
    at: UtcTimestamp,
) -> StoreResult<()> {
    facts::append(
        tx,
        EventId::new(),
        &run.to_string(),
        KIND,
        &LoadedSkills { skills: skills.to_vec() },
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

/// The Skills the user chose in the composer for this run's turn (the rest were loaded by the
/// model).
pub fn chosen(db: &Connection, run: AgentRunId) -> StoreResult<Vec<SkillInstanceId>> {
    Ok(facts::latest::<ChosenSkills>(db, &run.to_string(), CHOSEN)?
        .map(|value| value.skill_ids)
        .unwrap_or_default())
}

/// Records which Skills the user chose for this run's turn, when the turn starts.
pub fn set_chosen(
    tx: &Transaction<'_>,
    run: AgentRunId,
    skill_ids: &[SkillInstanceId],
    at: UtcTimestamp,
) -> StoreResult<()> {
    facts::append(
        tx,
        EventId::new(),
        &run.to_string(),
        CHOSEN,
        &ChosenSkills { skill_ids: skill_ids.to_vec() },
        at,
    )
}
