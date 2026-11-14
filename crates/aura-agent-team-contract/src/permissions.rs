//! What sub-agents start with: the permission mode of each kind of sub-agent, the approvals given
//! from the start, and whether the main agent answers their approvals first.
use aura_security_contract::PermissionMode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Under 设置 → 智能能力 → 多 Agent. A sub-agent already working keeps its mode and grants; the
/// user changes them on that sub-agent.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentTeamPermissions {
    /// The mode a sub-agent with a worktree of its own starts in; its workspace is that worktree.
    pub worktree_mode: PermissionMode,
    /// The mode a sub-agent working in the starting conversation's workspace starts in.
    pub shared_mode: PermissionMode,
    /// A new sub-agent may delete files with its commands without asking, where a command names
    /// no place outside its worktree and temporary folder.
    pub allow_deletions: bool,
    /// The same for commands that write outside its workspace.
    pub allow_outside_writes: bool,
    /// The same for downloads.
    pub allow_downloads: bool,
    /// The main agent answers a sub-agent's approval first, where it may: a deletion or a write
    /// that names no place outside the sub-agent's own.
    pub main_agent_approves: bool,
}

impl AgentTeamPermissions {
    pub const DEFAULT: Self = Self {
        worktree_mode: PermissionMode::WorkspaceReadWrite,
        shared_mode: PermissionMode::ReadOnly,
        allow_deletions: false,
        allow_outside_writes: false,
        allow_downloads: false,
        main_agent_approves: false,
    };

    /// The mode a new sub-agent starts in.
    #[must_use]
    pub const fn starting_mode(&self, worktree: bool) -> PermissionMode {
        if worktree { self.worktree_mode } else { self.shared_mode }
    }
}

impl Default for AgentTeamPermissions {
    fn default() -> Self {
        Self::DEFAULT
    }
}
