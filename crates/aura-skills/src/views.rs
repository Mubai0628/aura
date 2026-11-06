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
    /// Comes with Aura: written from Aura's own copy into a folder of Aura's at each start.
    #[serde(rename = "aura_builtin")]
    AuraBuiltIn,
    /// `%USERPROFILE%\.agents\skills`, shared with other agents.
    AgentGlobal,
    /// `<workspace>\.agents\skills`.
    Workspace,
    /// In an installed plugin; trusted and switched with the plugin.
    Plugin,
    /// `%USERPROFILE%\.claude\skills`, shared with Claude Code.
    ClaudeGlobal,
    /// `<workspace>\.claude\skills`.
    ClaudeWorkspace,
}

impl SkillSource {
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::AuraStore => "aura_store",
            Self::AuraBuiltIn => "aura_builtin",
            Self::AgentGlobal => "agent_global",
            Self::Workspace => "workspace",
            Self::Plugin => "plugin",
            Self::ClaudeGlobal => "claude_global",
            Self::ClaudeWorkspace => "claude_workspace",
        }
    }

    /// A folder of the selected workspace, which may hold someone else's repository: its Skills
    /// wait until the user trusts that workspace's Skills once.
    #[must_use]
    pub const fn in_workspace(self) -> bool {
        matches!(self, Self::Workspace | Self::ClaudeWorkspace)
    }

    /// One of the user's own folders, or Aura's: its Skills are usable as found.
    #[must_use]
    pub const fn users_own(self) -> bool {
        matches!(self, Self::AuraStore | Self::AuraBuiltIn | Self::AgentGlobal | Self::ClaudeGlobal)
    }
}

/// Whether the model may see and load a Skill now, and why not.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillState {
    /// Switched off by the user.
    Disabled,
    /// A workspace's Skill before the user trusts that workspace's Skills.
    NotTrusted,
    /// Trusted alone at a fingerprint, in a workspace not trusted as a whole, and changed since;
    /// it is not offered until trusted again.
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
    /// Pinned by the user: its instructions go whole into every request where it is usable.
    #[serde(default)]
    pub pinned: bool,
    /// Pinned and usable, but past the limit beside the pinned Skills before it in name order, so
    /// it is not sent now.
    #[serde(default)]
    pub pinned_left_out: bool,
    /// About how many tokens it takes pinned; 0 unless it is pinned and usable.
    #[serde(default)]
    pub pin_tokens: u64,
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
    /// Whether the user trusted the listed workspace's Skills; false when no workspace is listed.
    #[serde(default)]
    pub workspace_trusted: bool,
    /// About how many tokens the pinned Skills sent now take together, and the most they may.
    #[serde(default)]
    pub pinned_tokens: u64,
    #[serde(default)]
    pub pinned_limit: u64,
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

/// Trust, or stop trusting, every Skill in one workspace's `.agents\skills` and `.claude\skills`.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkillWorkspaceTrustRequest {
    pub workspace_id: WorkspaceId,
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

/// Pin a usable Skill into every request, within the shared limit, or unpin it.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkillSetPinnedRequest {
    pub id: SkillInstanceId,
    #[serde(default)]
    pub workspace_id: Option<WorkspaceId>,
    pub pinned: bool,
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
