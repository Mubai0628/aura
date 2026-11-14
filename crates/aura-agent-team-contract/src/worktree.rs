//! Worktrees: a copy of the repository on a branch of its own, for a sub-agent that changes
//! files. What the merge and discard tools take and answer, what the desktop shows of a worktree,
//! and where new worktrees go.
use crate::MemberState;
use aura_domain_common::SessionId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MERGE: &str = "agents.merge";
pub const DISCARD: &str = "agents.discard";

/// The folder new worktrees go under when the user has not chosen one, below the user's profile.
pub const DEFAULT_LOCATION_FOLDER: &str = "AuraWorktrees";

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentsMergeInput {
    /// The sub-agent's handle. Leave it out in a conversation that works in a worktree of its own
    /// to merge that worktree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct AgentsMergeResult {
    /// The handle, or none for this conversation's own worktree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    pub merged: bool,
    /// The commit the workspace's current branch is on after the merge; none when the changes
    /// were written into its files without a commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    /// The files the merge brought in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// Files both sides changed; nothing was merged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<String>,
    /// What happened and what to do next.
    pub note: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentsDiscardInput {
    /// The sub-agent's handle. Leave it out in a conversation that works in a worktree of its own
    /// to discard that worktree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct AgentsDiscardResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    pub discarded: bool,
    pub note: String,
}

/// Where a worktree is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeState {
    Active,
    Merged,
    Discarded,
}

impl WorktreeState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Merged => "merged",
            Self::Discarded => "discarded",
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "active" => Some(Self::Active),
            "merged" => Some(Self::Merged),
            "discarded" => Some(Self::Discarded),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeChangeKind {
    Added,
    Modified,
    Deleted,
}

/// One file a worktree changed since it started.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct WorktreeChange {
    pub path: String,
    pub kind: WorktreeChangeKind,
    /// Lines added and removed; none for a binary or very large file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed: Option<u64>,
}

/// A sub-agent's worktree as the 多 Agent page shows it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct AgentWorktreeView {
    pub branch: String,
    pub state: WorktreeState,
    /// Bytes on disk, build outputs included; none until counted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

/// The changes of a sub-agent's worktree, or of the conversation's own when `agent` is none.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentTeamChangesRequest {
    pub session_id: SessionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct AgentTeamChangesView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_state: Option<MemberState>,
    pub branch: String,
    pub state: WorktreeState,
    pub files: Vec<WorktreeChange>,
    /// The checks of its newest final report: each command with its exit code.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<String>,
    /// What its newest final report says is not done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

/// Where new worktrees go.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct WorktreeLocationView {
    /// The folder new worktrees go under.
    pub location: String,
    /// The user chose it; otherwise it is the default.
    pub chosen: bool,
    pub default_location: String,
    /// The settings revision a change names.
    pub revision: u64,
    /// Worktrees there now, and their bytes on disk; none until counted.
    pub worktrees: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    /// Free bytes on its drive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub free_bytes: Option<u64>,
}

/// Choose the folder for new worktrees, or go back to the default with `location` none.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorktreeLocationSetRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    pub expected_revision: u64,
}

/// The folder the user picked in Main's own dialog for new worktrees; none when cancelled.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct WorktreeLocationSelected {
    pub path: Option<String>,
}
