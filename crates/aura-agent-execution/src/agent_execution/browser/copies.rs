//! A sub-agent's worktree as the main agent's browser reaches it: a file of the copy opens by the
//! sub-agent's handle, and a page that shows one may load only that copy's files. The copy is
//! Aura's own folder for this conversation's sub-agent; the page only reads it.
use super::super::*;
use aura_agent_team::{WorktreeState, store as team_store, worktree as rows};
use std::path::PathBuf;

impl AgentExecution {
    /// The workspace of the active worktree of `session`'s sub-agent `handle`.
    pub(super) fn sub_agent_copy(
        &self,
        session: SessionId,
        handle: &str,
    ) -> anyhow::Result<WorkspaceId> {
        let journal = self.journal.lock()?;
        let db = journal.database.connection();
        let member = team_store::member_by_handle(db, session, handle)?.ok_or_else(|| {
            anyhow::anyhow!(
                "browser.open: 这个会话没有子 Agent {handle}；agent 写 agents.status 列出的编号，例如 A1"
            )
        })?;
        let worktree = rows::worktree_of(db, member.child_session_id)?.ok_or_else(|| {
            anyhow::anyhow!(
                "browser.open: {handle} 没有工作副本，它读的就是这个工作区；去掉 agent 再打开"
            )
        })?;
        anyhow::ensure!(
            worktree.state == WorktreeState::Active,
            "browser.open: {handle} 的工作副本已经合并或丢弃，打不开里面的文件"
        );
        Ok(worktree.workspace_id)
    }

    /// The folder of the active worktree of one of `session`'s sub-agents that holds the file
    /// `url` names, when it is one.
    pub(super) fn copy_holding(
        &self,
        session: SessionId,
        url: &str,
    ) -> anyhow::Result<Option<PathBuf>> {
        // Compared as the file addresses Aura makes of paths, ignoring case as Windows does.
        let url = url.to_lowercase();
        if !url.starts_with("file:") {
            return Ok(None);
        }
        let worktrees =
            rows::worktrees_of_parent(self.journal.lock()?.database.connection(), session)?;
        for worktree in worktrees.into_iter().filter(|w| w.state == WorktreeState::Active) {
            // A copy whose folder cannot be named now holds nothing the page may load.
            let Ok(resolver) = self.workspace.resolver(worktree.workspace_id) else { continue };
            let root = resolver.canonical_root().to_path_buf();
            let Ok(prefix) = url::Url::from_directory_path(&root) else { continue };
            if url.starts_with(&prefix.as_str().to_lowercase()) {
                return Ok(Some(root));
            }
        }
        Ok(None)
    }
}
