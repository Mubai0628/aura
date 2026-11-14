//! What the team needs from the application for worktrees: where a new one goes, the Git work, the
//! hidden workspace each one is registered as, the processes started in it, and its size.
use crate::{Worktree, WorktreeChange};
use aura_domain_common::{CommandInvocationId, WorkspaceId};
use std::future::Future;

/// Where a new worktree goes, already checked against the location rules and the free space.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreePlan {
    pub path: String,
    pub branch: String,
    /// The commit it starts from: the workspace's current one.
    pub start: String,
}

/// A worktree just added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddedWorktree {
    /// The hidden workspace it is registered as.
    pub workspace_id: WorkspaceId,
    /// The commit its own work starts from: the plan's start, or the commit on its branch that
    /// holds the workspace's uncommitted changes it took along.
    pub base: String,
    /// The files it took along, by path.
    pub carried: Vec<String>,
}

/// How bringing a worktree's changes into the workspace it came from ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorktreeMerge {
    /// Merged into the workspace's current branch, bringing in `files`.
    Merged { head: String, files: Vec<String> },
    /// Written into these files of the workspace without a commit: the workspace had
    /// uncommitted changes, or the worktree took them along.
    Applied { files: Vec<String> },
    /// The branch had nothing the workspace's current branch lacks.
    UpToDate,
    /// Both sides changed these files; nothing was merged and the workspace is as it was.
    Conflicted { files: Vec<String> },
    /// The workspace's uncommitted changes to these files clash with the worktree's; nothing was
    /// written and the workspace is as it was.
    Clashed { files: Vec<String> },
}

/// The step of [`WorktreeHost::merge_worktree`] a failure happened in, attached to its error as
/// context. Until the changes are brought into the workspace, the workspace is as it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeStep {
    /// Finding the workspace's folder.
    Locate,
    /// Reading the worktree's changes and committing them on its branch.
    Commit,
    /// Bringing the branch's changes into the workspace.
    BringIn,
}

impl std::fmt::Display for MergeStep {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Locate => "找主工作区的文件夹",
            Self::Commit => "在工作副本里提交它的改动",
            Self::BringIn => "把改动带进主工作区",
        })
    }
}

pub trait WorktreeHost: Send + Sync {
    /// A worktree of `workspace` named by `key`: its folder under the worktree location and its
    /// branch. Refused when the workspace is not a repository, the location is not allowed, or the
    /// drive would be left with less than its reserve.
    fn plan_worktree(
        &self,
        workspace: WorkspaceId,
        key: &str,
    ) -> impl Future<Output = anyhow::Result<WorktreePlan>> + Send;

    /// Adds the worktree with the workspace's uncommitted changes, ignored files left out, and
    /// registers its folder as a hidden workspace listed under `workspace`. The same plan again
    /// answers with the same workspace.
    fn add_worktree(
        &self,
        workspace: WorkspaceId,
        plan: &WorktreePlan,
        command: CommandInvocationId,
    ) -> impl Future<Output = anyhow::Result<AddedWorktree>> + Send;

    /// What the worktree changed since it started: commits, edits and new files.
    fn worktree_changes(
        &self,
        worktree: &Worktree,
    ) -> impl Future<Output = anyhow::Result<Vec<WorktreeChange>>> + Send;

    /// Commits what the worktree has not committed, then brings its own changes into the
    /// workspace it came from: its branch merged into the current branch when the workspace has
    /// no uncommitted changes in the way and the worktree took none along, otherwise written into
    /// the workspace's files without a commit. A failure carries the [`MergeStep`] it happened in
    /// as context.
    fn merge_worktree(
        &self,
        worktree: &Worktree,
        message: &str,
        command: CommandInvocationId,
    ) -> impl Future<Output = anyhow::Result<WorktreeMerge>> + Send;

    /// Ends the processes started in the worktree, then removes its folder with everything in it
    /// and its branch. Gone already is done.
    fn remove_worktree(
        &self,
        worktree: &Worktree,
        command: CommandInvocationId,
    ) -> impl Future<Output = anyhow::Result<()>> + Send;

    /// Its bytes on disk, build outputs included, when they have been counted.
    fn worktree_size(&self, worktree: &Worktree) -> Option<u64>;
}
