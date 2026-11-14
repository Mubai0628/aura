//! Sub-agents a conversation's agent starts for one task each: the task sheet it hands over, the
//! tools the sub-agent is given, the reports it hands back, and how many may run at once.
//!
//! A sub-agent works in a conversation of its own in the same workspace. That conversation is
//! hidden from the sidebar; the starting conversation sees only its reports. The application
//! layer supplies the conversations and turns through [`TeamHost`]; the tool calls arrive through
//! [`TeamTools`], which the execution layer reaches without knowing the application. What the
//! team exchanges (sheet, reports, tool inputs and results, the rules) is in
//! `aura-agent-team-contract`, re-exported here.
mod host;
pub mod limits;
mod port;
pub mod restart;
mod sizes;
pub mod store;
mod team;
pub mod worktree;
mod worktree_host;

pub use aura_agent_team_contract::*;
pub use host::{ModelOffer, Parent, Progress, ReviewRequest, StopNote, TeamHost};
pub use port::{Mark, Tally, TeamCall, TeamFuture, TeamPort, TeamTools, WatchFuture};
pub use sizes::FolderSizes;
pub use team::{
    Team, WAIT_DEFAULT_SECONDS, WAIT_MAX_SECONDS, Watchdog, cause_of, choose_model,
    main_agent_denial, one_line,
};
pub use worktree::{Worktree, WorktreeDetails};
pub use worktree_host::{AddedWorktree, MergeStep, WorktreeHost, WorktreeMerge, WorktreePlan};
