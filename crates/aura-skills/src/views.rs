//! What Settings → Skill shows and asks. Every change names the `skills.json` revision it was
//! made against; trust is given only at the fingerprint the user was shown.
use crate::package::SkillFile;
use aura_domain_common::{SkillInstanceId, WorkspaceId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where a Skill folder lives.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    /// Imported into Aura's own Skill folder.
    AuraStore,
    /// `%USERPROFILE%\.agents\skills`, shared with other agents.
    AgentGlobal,
    /// `<workspace>\.agents\skills`.
    Workspace,
    /// In an installed plugin; trusted and switched with the plugin.
    Plugin,
}

impl SkillSource {
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::AuraStore => "aura_store",
            Self::AgentGlobal => "agent_global",
            Self::Workspace => "workspace",
            Self::Plugin => "plugin",
        }
    }
}

/// Whether the model may see and load a Skill now, and why not.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillState {
    /// Switched off. A Skill found outside Aura is off until the user switches it on.
    Disabled,
    NotTrusted,
    /// The content changed since the user trusted it; it is not offered until trusted again.
    ContentChanged,
    Ready,
    /// `SKILL.md` is missing or not usable; see `problems`.
    Invalid,
    /// The folder is a link, too large or unreadable; see `problems`.
    Unavailable,
}

/// The saved choices for one Skill in `skills.json`. Nothing else about a Skill is stored.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkillSetting {
    pub id: SkillInstanceId,
    pub source: SkillSource,
    /// The workspace a workspace Skill belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<WorkspaceId>,
    /// The folder relative to its source's root, `/`-separated.
    pub relative: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trusted_fingerprint: Option<String>,
    /// Saved for multi-agent; no sub-agent exists yet.
    #[serde(default)]
    pub available_for_subagents: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct SkillView {
    pub id: SkillInstanceId,
    pub source: SkillSource,
    /// The plugin that brings it, for a plugin's Skill.
    #[serde(default)]
    pub plugin: Option<String>,
    pub workspace_id: Option<WorkspaceId>,
    pub relative: String,
    /// The folder on this computer.
    pub path: String,
    pub state: SkillState,
    pub enabled: bool,
    pub name: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    /// What the Skill says it needs. Shown only; Aura never loads or allows a tool because a
    /// Skill names it.
    pub allowed_tools: Vec<String>,
    /// What trust would record now; absent when the folder cannot be read.
    pub fingerprint: Option<String>,
    pub trusted_fingerprint: Option<String>,
    pub warnings: Vec<String>,
    /// Files `SKILL.md` links to that are not in the folder (the Skill still works).
    pub missing_references: Vec<String>,
    pub problems: Vec<String>,
    pub file_count: u32,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct SkillRootView {
    pub source: SkillSource,
    pub path: String,
    pub exists: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct SkillRegistryView {
    pub revision: u64,
    pub skills: Vec<SkillView>,
    pub roots: Vec<SkillRootView>,
    /// Why saved choices cannot be used now (then nothing is offered to the model).
    pub issue: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct SkillPreview {
    pub skill: SkillView,
    /// `SKILL.md` as written.
    pub skill_md: String,
    pub files: Vec<SkillFile>,
}

/// List the Skills; a workspace adds its own `.agents\skills`.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkillListRequest {
    #[serde(default)]
    pub workspace_id: Option<WorkspaceId>,
}

/// One Skill, for its preview or its folder.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkillRequest {
    pub id: SkillInstanceId,
    #[serde(default)]
    pub workspace_id: Option<WorkspaceId>,
}

/// Trust exactly the shown content and switch the Skill on.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkillTrustRequest {
    pub id: SkillInstanceId,
    #[serde(default)]
    pub workspace_id: Option<WorkspaceId>,
    pub expected_fingerprint: String,
    pub expected_revision: u64,
}

/// Untrust a Skill, or remove one from Aura's own folder.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkillChangeRequest {
    pub id: SkillInstanceId,
    #[serde(default)]
    pub workspace_id: Option<WorkspaceId>,
    pub expected_revision: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkillSetEnabledRequest {
    pub id: SkillInstanceId,
    #[serde(default)]
    pub workspace_id: Option<WorkspaceId>,
    pub enabled: bool,
    pub expected_revision: u64,
}

/// Copy a Skill folder, or unpack a ZIP file, the user chose into Aura's own folder; the
/// original is not changed.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkillImportRequest {
    pub path: String,
    pub expected_revision: u64,
}

/// Main's own folder dialog for importing a Skill.
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkillSelectDirectoryRequest {}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct SkillSelectedDirectory {
    /// Absent when the user cancelled.
    pub path: Option<String>,
}

/// Main's own file dialog for importing a Skill packed as a ZIP file (`.zip`, `.skill`).
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkillSelectArchiveRequest {}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct SkillSelectedArchive {
    /// Absent when the user cancelled.
    pub path: Option<String>,
}

/// Main opened a listed Skill's folder in the file manager, or says why it did not.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkillDirectoryOpened {
    pub completed: bool,
    pub message: String,
}
