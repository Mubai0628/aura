//! `skills.*` model tools: find the user's usable Skills and load one by its name. A Skill is
//! guidance the user keeps; loading one never allows a tool or widens what the session may do.
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

/// One usable Skill.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillFound {
    pub name: String,
    /// Where it is from, to pass to skills.load with its name; only when another usable Skill
    /// has the same name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillsFoundResult {
    pub skills: Vec<SkillFound>,
    /// More Skills matched than were returned.
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillsLoadInput {
    /// The Skill's name, as the list of Skills or skills.search shows it.
    pub name: String,
    /// Where it is from: the place in brackets in the list, or skills.search's from. Needed
    /// only when two usable Skills have the same name.
    #[serde(default)]
    pub from: Option<String>,
}

/// A loaded Skill: its instructions, and its folder and files to read when they point to them.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillContent {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    pub description: String,
    /// The Skill's folder on this computer.
    pub folder: String,
    /// The folder's other files, relative to it.
    pub files: Vec<String>,
    /// Files in the folder that files does not name.
    #[serde(default)]
    pub more_files: u32,
    /// What the Skill says it needs. Only a description: no tool is loaded or allowed by it.
    pub allowed_tools: Vec<String>,
    /// The instructions of its SKILL.md.
    pub instructions: String,
}

/// A Skill the user chose in the composer, at the content it was chosen with.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LoadedSkillEntry {
    pub skill_id: SkillInstanceId,
    pub name: String,
    pub origin: SkillOrigin,
    /// The content fingerprint of the Skill when it was chosen.
    pub fingerprint: String,
}

/// The message as the model reads it with the Skills the user chose for it: its text (what the
/// user typed, with its attachments), then each chosen Skill whole. With none it is the text.
#[must_use]
pub fn message_with_skills(text: &str, chosen: &[String]) -> String {
    let mut whole = text.to_owned();
    for skill in chosen {
        whole.push_str("\n\n");
        whole.push_str(skill);
    }
    whole
}
