//! What the user does to one sub-agent from the desktop besides stopping it: its worktree's folder
//! for the file manager, and taking back what its commands could do without asking.
use super::*;
use crate::{AllowanceKind, WorktreeState, worktree as rows};

impl<H: TeamHost> Team<'_, H> {
    /// The folder of the worktree sub-agent `handle` of `parent` works in, while it is in use;
    /// one merged or discarded, or none, is refused.
    pub async fn worktree_folder(&self, parent: SessionId, handle: &str) -> anyhow::Result<String> {
        let member = self.one(parent, handle).await?;
        let child = member.child_session_id;
        let worktree = self
            .host
            .read("agents.worktree.folder", move |db| rows::worktree_of(db, child))
            .await?;
        match worktree {
            Some(worktree) if worktree.state == WorktreeState::Active => Ok(worktree.details.path),
            _ => Err(Refusal::new(
                "agents.no_worktree",
                format!("{} 没有在用的工作副本。", member.handle),
            )
            .into()),
        }
    }

    /// The user takes back one thing sub-agent `handle` of `parent` could do without asking;
    /// its later commands of that kind ask again.
    pub async fn revoke_allowance(
        &self,
        parent: SessionId,
        handle: &str,
        kind: AllowanceKind,
    ) -> anyhow::Result<()> {
        let child = self.one(parent, handle).await?.child_session_id;
        self.host
            .write("agents.allowance.revoke", move |tx| {
                store::change_allowed(tx, child, |allowed| match kind {
                    AllowanceKind::Deletions => allowed.deletions = false,
                    AllowanceKind::OutsideWrites => allowed.outside_writes = false,
                    AllowanceKind::Downloads => allowed.downloads = false,
                })
                .map(|_| ())
            })
            .await
    }
}
