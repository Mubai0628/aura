//! Carrying a conversation's history into a branch just created for it.
//!
//! The branch opens with its newest turns; the older
//! ones follow in the background one short step at a time, so the store is never held for the
//! whole conversation, and a run in the branch waits only until the history it is sent is whole.
use super::*;
use crate::agent_store::{StoreError, StoreStep};
use aura_persistence_core::PersistenceCoreError;
use aura_persistence_v2::agent::branch::{
    self, BranchImport, BranchTiming, CopyStep, FirstBatch, SourceSnapshot,
};

/// How many of a branch's newest turns are carried before it opens.
pub(crate) const FIRST_TURNS: usize = 6;
/// How long carrying them may take before the branch opens anyway; the
/// turns not carried by then follow in the background.
const FIRST_BUDGET: std::time::Duration = std::time::Duration::from_millis(600);
/// How long a run in a branch waits for its older turns before it is refused.
const COPY_WAIT: std::time::Duration = std::time::Duration::from_secs(600);

impl AgentApplication {
    /// Mirrors the new conversation into the execution store and opens it with its newest turns,
    /// in one transaction; the older turns follow in the background.
    pub(crate) async fn carry_into_branch(
        self: &Arc<Self>,
        import: BranchImport,
    ) -> anyhow::Result<Result<AgentBranchOrigin, StoreError>> {
        let started = std::time::Instant::now();
        let projection = self.execution.persistence.workspace_session_projection()?;
        let metadata = projection
            .sessions
            .iter()
            .find(|s| s.session_id == import.branch)
            .ok_or_else(|| anyhow::anyhow!("agent.session_missing"))?
            .clone();
        let workspace = projection
            .workspaces
            .iter()
            .find(|w| w.workspace_id == metadata.workspace_id)
            .ok_or_else(|| anyhow::anyhow!("agent.workspace_missing"))?
            .clone();
        // The source is read in this store; it has to be there under its own conversation.
        self.ensure_session(import.source).await?;
        let branch = import.branch;
        let carried = self
            .store
            .write(StoreStep::operation("agent.session.branch"), move |journal| {
                journal.database.unit_of_work(|tx| {
                    AgentApplication::mirror_session(tx, &workspace, &metadata)?;
                    branch::begin(
                        tx,
                        &import,
                        FirstBatch { turns: FIRST_TURNS, budget: FIRST_BUDGET },
                    )
                })
            })
            .await;
        // Where a branch's time went, for a slow one on the owner's machine.
        if let Ok((_, timing)) = &carried {
            log_timing("session.branch_timing", started, timing);
            self.continue_branch_copy(branch);
        }
        Ok(carried.map(|(origin, _)| origin))
    }

    /// Carries a branch's older turns, one short step at a time, then puts its history in order.
    /// A copy that cannot go on is recorded on the branch, which keeps the turns it has.
    pub(crate) fn continue_branch_copy(self: &Arc<Self>, branch: SessionId) {
        let app = Arc::downgrade(self);
        tokio::spawn(async move {
            let started = std::time::Instant::now();
            let snapshot: Arc<Mutex<Option<SourceSnapshot>>> = Arc::new(Mutex::new(None));
            let timing: Arc<Mutex<BranchTiming>> = Arc::default();
            loop {
                let Some(app) = app.upgrade() else { return };
                let (kept, spent) = (Arc::clone(&snapshot), Arc::clone(&timing));
                let step = app
                    .store
                    .write(StoreStep::operation("agent.session.branch_copy"), move |journal| {
                        let mut kept = kept.lock().map_err(|_| PersistenceCoreError::Conflict)?;
                        let mut spent = spent.lock().map_err(|_| PersistenceCoreError::Conflict)?;
                        journal.database.unit_of_work(|tx| {
                            branch::step(tx, branch, UtcTimestamp::now(), &mut kept, &mut spent)
                        })
                    })
                    .await;
                match step {
                    // Other work goes first between two steps.
                    Ok(CopyStep::Carried) => tokio::task::yield_now().await,
                    Ok(CopyStep::Finished) => {
                        if let Ok(timing) = timing.lock() {
                            log_timing("session.branch_copied", started, &timing);
                        }
                        return;
                    }
                    Ok(CopyStep::Stopped) => return,
                    Err(error) => {
                        let reason = error.to_string();
                        crate::append_core_log(
                            "session.branch_copy_failed",
                            serde_json::json!({"session_id": branch, "reason": reason}),
                        );
                        let _ = app
                            .store
                            .write(StoreStep::operation("agent.session.branch_copy_failed"), {
                                move |journal| {
                                    journal.database.unit_of_work(|tx| {
                                        branch::fail(tx, branch, &reason, UtcTimestamp::now())
                                    })
                                }
                            })
                            .await;
                        return;
                    }
                }
            }
        });
    }

    /// Resumes the copies a previous process left unfinished.
    pub(crate) async fn resume_branch_copies(self: &Arc<Self>) -> anyhow::Result<()> {
        let unfinished = self
            .store
            .read(StoreStep::operation("agent.session.branch_resume"), branch::unfinished)
            .await?;
        for (session, _, failed) in unfinished {
            if !failed {
                self.continue_branch_copy(session);
            }
        }
        Ok(())
    }

    /// A run in a branch waits until the branch's history is whole: until then the model would be
    /// sent only part of it.
    pub(super) async fn wait_for_branch_copy(
        self: &Arc<Self>,
        run_id: AgentRunId,
        session: SessionId,
        cancel: &ModelCancellationToken,
    ) -> anyhow::Result<()> {
        let deadline = std::time::Instant::now() + COPY_WAIT;
        loop {
            anyhow::ensure!(!cancel.is_cancelled(), "agent.cancelled");
            let copying = self
                .store
                .read(StoreStep::run(run_id, "agent.context.branch_copy"), move |db| {
                    branch::progress(db, session)
                })
                .await?;
            let Some(copying) = copying else { return Ok(()) };
            anyhow::ensure!(
                !copying.failed,
                "agent.branch_copy_failed: 这个分支之前的消息没能复制完成，无法把完整的历史发给模型。可以从原会话重新新建分支。"
            );
            anyhow::ensure!(
                std::time::Instant::now() < deadline,
                "agent.branch_copying: 这个分支之前的消息还在复制，请稍后再发送。"
            );
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }
}

fn log_timing(event: &str, started: std::time::Instant, timing: &BranchTiming) {
    crate::append_core_log(
        event,
        serde_json::json!({
            "total_ms": started.elapsed().as_millis(),
            "turns": timing.turns,
            "calls": timing.calls,
            "phases_ms": timing.milliseconds(),
        }),
    );
}
