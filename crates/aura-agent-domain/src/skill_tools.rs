//! `skills.*` model tools: find the user's trusted Skills, read one, and load it for this
//! turn. A Skill is guidance the user trusted; loading one never allows a tool or widens what
//! the session may do.
use aura_domain_common::SkillInstanceId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where a Skill lives: `aura_store` (imported into Aura), `agent_global` (the user's
/// `.agents\skills`) or `workspace` (this workspace's `.agents\skills`).
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillOrigin {
    AuraStore,
    AgentGlobal,
    Workspace,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillsSearchInput {
    /// Words to find in a Skill's name or description; empty lists every usable Skill.
    pub query: String,
    /// At most this many results (default 20, at most 50).
    #[serde(default)]
    pub limit: Option<u32>,
}

/// One usable Skill: enabled and trusted by the user at its current content.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillSummary {
    pub skill_id: SkillInstanceId,
    pub name: String,
    pub description: String,
    pub origin: SkillOrigin,
    /// What the Skill says it needs. Only a description: no tool is loaded or allowed by it.
    pub allowed_tools: Vec<String>,
    /// Loaded in this turn.
    pub loaded: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillsSearchResult {
    pub skills: Vec<SkillSummary>,
    /// More Skills matched than were returned.
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillsDescribeInput {
    pub skill_id: SkillInstanceId,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillFileEntry {
    /// Relative to the Skill folder, `/`-separated; pass it to `skills.read_resource`.
    pub path: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillDescription {
    pub skill_id: SkillInstanceId,
    pub name: String,
    pub description: String,
    pub origin: SkillOrigin,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    pub allowed_tools: Vec<String>,
    pub files: Vec<SkillFileEntry>,
    pub loaded: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillsLoadInput {
    pub skill_ids: Vec<SkillInstanceId>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillsUnloadInput {
    pub skill_ids: Vec<SkillInstanceId>,
}

/// A Skill loaded for this turn, at the content it was loaded with.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LoadedSkillEntry {
    pub skill_id: SkillInstanceId,
    pub name: String,
    pub origin: SkillOrigin,
    /// The content fingerprint the user trusted and this load used.
    pub fingerprint: String,
}

/// The Skills loaded for this turn. Their instructions are in the task state from the next
/// step on; they unload when the turn ends.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillsLoadedResult {
    pub loaded: Vec<LoadedSkillEntry>,
    /// The most Skills one turn may have loaded.
    pub limit: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillsReadResourceInput {
    pub skill_id: SkillInstanceId,
    /// A file inside the Skill folder, as `skills.describe` lists it.
    pub path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillsReadResourceResult {
    pub skill_id: SkillInstanceId,
    pub path: String,
    pub text: String,
}
