//! The owners a deletion touches, cloned from the runtime's services so a deletion can finish on
//! its own task after the route that started it has answered.
use crate::agent_store::{AgentStore, StoreAccess, StoreStep};
use crate::*;
use aura_domain_common::SessionId;
use aura_persistence::RemovedWorkspace;
use aura_persistence_v2::session_purge;
use aura_runtime_contract::CoreBusyActivityV1;
use aura_windows_user_contract::ManagedProcessStopRequestV1;
use aura_workspace_domain::SessionSummary;

#[derive(Clone)]
pub(crate) struct Purger {
    persistence: Arc<PersistenceCoordinator>,
    broker: Arc<broker::BrokerJournal>,
    store: Arc<AgentStore>,
    commands: Option<Arc<command_execution::CommandExecution>>,
}

impl Purger {
    pub(crate) fn of(services: &RuntimeServices) -> Self {
        Self {
            persistence: Arc::clone(&services.persistence),
            broker: Arc::clone(&services.broker),
            store: Arc::clone(&services.agent_store),
            commands: services.commands().ok(),
        }
    }

    async fn registry<T: Send + 'static>(
        &self,
        work: impl FnOnce(&PersistenceCoordinator) -> Result<T, aura_persistence::PersistenceError>
        + Send
        + 'static,
    ) -> anyhow::Result<T> {
        let persistence = Arc::clone(&self.persistence);
        tokio::task::spawn_blocking(move || work(&persistence)).await?.map_err(|error| {
            if error.to_string().contains("session.not_found") {
                anyhow::anyhow!("session.not_found: 这个会话已不存在。")
            } else {
                error.into()
            }
        })
    }

    async fn summary(&self, session: SessionId) -> anyhow::Result<SessionSummary> {
        self.registry(|p| p.workspace_session_projection())
            .await?
            .sessions
            .into_iter()
            .find(|s| s.session_id == session)
            .ok_or_else(|| anyhow::anyhow!("session.not_found: 这个会话已不存在。"))
    }

    /// Only an
    /// archived conversation can be deleted.
    pub(crate) async fn ensure_archived(&self, session: SessionId) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.summary(session).await?.state == "archived",
            "session.not_archived: 请先归档这个会话，再到“设置 → 归档”里删除。"
        );
        Ok(())
    }

    /// Refuses while the Session has a task that has not finished; archiving also while it has a
    /// background process running, because an archived Session is out of sight.
    pub(crate) async fn ensure_idle(
        &self,
        session: SessionId,
        archiving: bool,
    ) -> anyhow::Result<()> {
        self.summary(session).await?;
        let broker = Arc::clone(&self.broker);
        let busy = self
            .store
            .run_critical(
                StoreStep::operation("session.lifecycle.check"),
                StoreAccess::Read,
                move || {
                    let state = aura_persistence_v2::agent::session::load(
                        broker.lock()?.database.connection(),
                        session,
                    )?;
                    Ok(state.active_turn.is_some())
                },
            )
            .await?;
        anyhow::ensure!(!busy, "session.busy: 这个会话还有任务在运行，请先停止当前任务。");
        if !archiving {
            // A branch still copying this conversation's history reads it.
            let broker = Arc::clone(&self.broker);
            let copied = self
                .store
                .run_critical(
                    StoreStep::operation("session.lifecycle.branch_copy"),
                    StoreAccess::Read,
                    move || {
                        Ok(aura_persistence_v2::agent::branch::copying_from(
                            broker.lock()?.database.connection(),
                            session,
                        )?)
                    },
                )
                .await?;
            anyhow::ensure!(
                !copied,
                "session.branch_copying: 有一个分支还在从这个会话复制历史，复制完成后再删除。"
            );
        }
        if archiving && let Some(commands) = &self.commands {
            anyhow::ensure!(
                commands.process_list(session).await?.processes.is_empty(),
                "session.processes_running: 这个会话还有后台进程在运行，请先在“进程”里停止。"
            );
        }
        Ok(())
    }

    pub(crate) async fn set_archived(
        &self,
        session: SessionId,
        archived: bool,
    ) -> anyhow::Result<SessionSummary> {
        self.registry(move |p| p.set_session_archived(session, archived)).await
    }

    /// Hides the Session, then deletes everything Aura holds for it.
    pub(crate) async fn delete(&self, session: SessionId) -> anyhow::Result<()> {
        // A long deletion holds the store; the status says Core is busy deleting.
        let _busy = crate::busy::begin(CoreBusyActivityV1::DeletingSession);
        self.registry(move |p| p.mark_session_deleting(session)).await?;
        self.finish(session).await
    }

    /// Stops what still runs for the Session, then deletes its execution-store rows and its
    /// primary-registry rows. Repeating it after an interruption is safe.
    async fn finish(&self, session: SessionId) -> anyhow::Result<()> {
        self.stop(&[session]).await?;
        self.purge_execution(session).await?;
        self.registry(move |p| p.purge_session(session)).await?;
        Ok(())
    }

    async fn stop(&self, sessions: &[SessionId]) -> anyhow::Result<()> {
        let Some(commands) = &self.commands else {
            return Ok(());
        };
        for &session in sessions {
            for process in commands.process_list(session).await?.processes {
                commands
                    .process_stop(ManagedProcessStopRequestV1 {
                        session_id: session,
                        tool_call_id: process.tool_call_id,
                    })
                    .await?;
            }
        }
        // Terminals of Sessions the projection no longer lists are closed and their records go.
        let live = self.registry(|p| p.workspace_session_projection()).await?.sessions;
        commands.close_removed_terminal_sessions(&live).await;
        Ok(())
    }

    async fn purge_execution(&self, session: SessionId) -> anyhow::Result<()> {
        let broker = Arc::clone(&self.broker);
        self.store
            .run_critical(StoreStep::operation("session.purge"), StoreAccess::Write, move || {
                Ok(broker
                    .lock()?
                    .database
                    .unit_of_work(|tx| session_purge::purge_session(tx, session))?)
            })
            .await?;
        // The zip entries opened for the Session's 文件 tab.
        let broker = Arc::clone(&self.broker);
        tokio::task::spawn_blocking(move || {
            crate::agent_application::attachment_entries::forget_entries(&broker, session)
        })
        .await??;
        Ok(())
    }

    /// Finishes what an earlier deletion or Workspace removal left: Sessions still `deleting`,
    /// then removed Workspaces' execution-store rows and Aura's project-memory files for them.
    pub(crate) async fn resume(&self) -> anyhow::Result<()> {
        for session in self.registry(|p| p.deleting_sessions()).await? {
            let _busy = crate::busy::begin(CoreBusyActivityV1::FinishingDeletion);
            self.finish(session).await?;
        }
        for removed in self.registry(|p| p.removed_workspaces()).await? {
            self.remove_workspace(removed).await?;
        }
        Ok(())
    }

    async fn remove_workspace(&self, removed: RemovedWorkspace) -> anyhow::Result<()> {
        let _busy = crate::busy::begin(CoreBusyActivityV1::RemovingWorkspace);
        let workspace = removed.workspace_id;
        let broker = Arc::clone(&self.broker);
        let sessions = self
            .store
            .run_critical(
                StoreStep::operation("workspace.purge.sessions"),
                StoreAccess::Read,
                move || {
                    Ok(session_purge::workspace_sessions(
                        broker.lock()?.database.connection(),
                        workspace,
                    )?)
                },
            )
            .await?;
        self.stop(&sessions).await?;
        for session in sessions {
            self.purge_execution(session).await?;
        }
        let broker = Arc::clone(&self.broker);
        self.store
            .run_critical(StoreStep::operation("workspace.purge"), StoreAccess::Write, move || {
                Ok(broker
                    .lock()?
                    .database
                    .unit_of_work(|tx| session_purge::purge_workspace(tx, workspace))?)
            })
            .await?;
        let memory = Arc::clone(self.broker.memory());
        let roots = removed.roots.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            for root in roots {
                memory.forget_project(&root)?;
            }
            Ok(())
        })
        .await??;
        self.registry(move |p| p.finish_removed_workspace(&removed)).await?;
        Ok(())
    }

    /// Runs `resume` on its own task. What fails is logged and tried again at the next start.
    pub(crate) fn spawn_resume(self) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        runtime.spawn(async move {
            if let Err(error) = self.resume().await {
                append_core_log("session.cleanup_unfinished", json!({"reason": error.to_string()}));
            }
        });
    }
}
