//! The links between a sub-agent's worktree workspace and the workspace it was made from, named
//! while Core runs (see [`crate::workspace_projection`]).
use super::PersistenceCoordinator;
use crate::PersistenceError;
use aura_domain_common::WorkspaceId;

impl PersistenceCoordinator {
    /// Lists workspace `workspace`, a worktree, under `parent` from now on. Made before the
    /// worktree's workspace is created, so no publication lists it on its own.
    pub fn link_worktree(
        &self,
        workspace: WorkspaceId,
        parent: WorkspaceId,
    ) -> Result<(), PersistenceError> {
        self.inner.worktree_links.link(workspace, parent)
    }

    /// The workspace `workspace` was made from, when it is a worktree's; read from memory, so it
    /// takes no store lock.
    #[must_use]
    pub fn worktree_parent(&self, workspace: WorkspaceId) -> Option<WorkspaceId> {
        self.inner.worktree_links.parent(workspace)
    }
}
