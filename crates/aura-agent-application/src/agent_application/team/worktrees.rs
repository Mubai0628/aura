//! Worktrees for the desktop: what one changed, where new ones go, their removal with a deleted
//! conversation, and their links named again at start.
use super::worktree_host::{check_location, free_space, location_folder};
use super::*;
use aura_agent_team::{
    AgentTeamChangesRequest, AgentTeamChangesView, TeamHost, WorktreeHost,
    WorktreeLocationSetRequest, WorktreeLocationView, worktree as rows,
};
use std::path::Path;

impl AgentApplication {
    /// What a sub-agent's worktree changed, or the conversation's own, with its newest checks.
    pub async fn team_changes(
        self: &Arc<Self>,
        request: AgentTeamChangesRequest,
    ) -> anyhow::Result<AgentTeamChangesView> {
        let host = TeamApp(Arc::clone(self));
        Team::new(&host, self.execution.team.news())
            .changes(request.session_id, request.agent.as_deref())
            .await
    }

    /// Where new worktrees go, how many are in use and their bytes, and the drive's free space.
    pub async fn worktree_location(self: &Arc<Self>) -> anyhow::Result<WorktreeLocationView> {
        let (chosen, revision) = self
            .store
            .read(StoreStep::operation("agents.worktree.location"), rows::location)
            .await?;
        let active = self
            .store
            .read(StoreStep::operation("agents.worktree.active"), rows::active_worktrees)
            .await?;
        let host = TeamApp(Arc::clone(self));
        let sizes: Option<Vec<u64>> =
            active.iter().map(|worktree| host.worktree_size(worktree)).collect();
        let default_location = location_folder(None)?;
        let location = location_folder(chosen.as_deref())?;
        Ok(WorktreeLocationView {
            location: location.display().to_string(),
            chosen: chosen.is_some(),
            default_location: default_location.display().to_string(),
            revision,
            worktrees: u32::try_from(active.len()).unwrap_or(u32::MAX),
            size_bytes: sizes.map(|sizes| sizes.into_iter().sum()),
            free_bytes: free_space(&location),
        })
    }

    /// Chooses the folder for new worktrees, or the default again; worktrees already made stay
    /// where they are.
    pub async fn set_worktree_location(
        self: &Arc<Self>,
        request: WorktreeLocationSetRequest,
    ) -> anyhow::Result<WorktreeLocationView> {
        let chosen = request.location.map(|location| location.trim().to_owned());
        if let Some(chosen) = chosen.as_deref() {
            let path = Path::new(chosen);
            check_location(self, path)?;
            anyhow::ensure!(
                !path.exists() || path.is_dir(),
                "agents.worktree_location: 这个位置是一个文件，请选一个文件夹。"
            );
        }
        let expected = request.expected_revision;
        let host = TeamApp(Arc::clone(self));
        host.write("agents.worktree.location.set", move |tx| {
            rows::set_location(tx, chosen.as_deref(), expected)
        })
        .await
        .map_err(|error| match error.downcast_ref::<StoreError>() {
            Some(StoreError::Failed {
                error: aura_persistence_core::PersistenceCoreError::Conflict,
                ..
            }) => anyhow::anyhow!("agents.location_changed: 位置刚被改过，请重新打开设置再改。"),
            _ => error,
        })?;
        self.worktree_location().await
    }

    /// Removes the worktrees of a conversation being deleted and of its sub-agents, and the
    /// sub-agents' temporary folders, before their records go.
    pub async fn remove_session_worktrees(
        self: &Arc<Self>,
        session: SessionId,
    ) -> anyhow::Result<()> {
        let host = TeamApp(Arc::clone(self));
        Team::new(&host, self.execution.team.news()).remove_session_worktrees(session).await?;
        let members = self
            .store
            .read(StoreStep::operation("agents.members.temp"), move |db| {
                aura_agent_team::store::members_of(db, session)
            })
            .await?;
        for member in members {
            aura_agent_execution::command_execution::process_policy::remove_temp_directory(
                member.child_session_id,
            );
        }
        Ok(())
    }
}

/// Lists every worktree's workspace under the workspace it was made from again, before the
/// projection is first read.
pub(in crate::agent_application) fn restore_worktree_links(
    execution: &agent_execution::AgentExecution,
    store: &AgentStore,
) -> anyhow::Result<()> {
    let worktrees = rows::all_worktrees(store.journal().lock()?.database.connection())?;
    for worktree in worktrees {
        execution.persistence.link_worktree(worktree.workspace_id, worktree.parent_workspace_id)?;
    }
    Ok(())
}
