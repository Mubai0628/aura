//! Timers hold no execution locks or strong application reference. The Store
//! resolves races against approval, mode change and cancellation in one CAS.
//!
//! The same timer ends a question nobody touched as "no response", so the
//! question never holds the rest of the task.
//!
//! OWNERSHIP: one timer per run, kept in [`ExpiryTimers`]. A timer that fails to start or fails
//! to record is a **run-local** diagnostic, never a process-wide fault: a durable wait stays
//! visible either way, and one run's timer cannot degrade another session's status.
use super::*;
use aura_core_broker::agent_store::StoreStep;
use aura_persistence_v2::agent::{approval, human_wait, query, recovery};
use std::collections::BTreeMap;

/// The timers one Core owns, at most one per run.
#[derive(Default)]
pub(crate) struct ExpiryTimers<Id = AgentRunId> {
    inner: Mutex<BTreeMap<Id, tokio::task::JoinHandle<()>>>,
}

impl<Id: Ord> ExpiryTimers<Id> {
    #[cfg(any(test, feature = "test-hooks"))]
    pub(super) async fn stop_and_join_for_test(&self, deadline: Instant) {
        let tasks = std::mem::take(&mut *self.inner.lock().expect("expiry timer owner"));
        for (_, mut task) in tasks {
            task.abort();
            let result = tokio::time::timeout_at(deadline.into(), &mut task)
                .await
                .expect("old expiry timer must end");
            assert!(result.is_ok() || result.is_err_and(|error| error.is_cancelled()));
        }
    }

    /// Replaces any previous timer for this run, so a re-armed wait cannot be expired twice.
    pub(super) fn install(&self, id: Id, task: tokio::task::JoinHandle<()>) {
        if let Ok(mut timers) = self.inner.lock() {
            timers.retain(|_, timer| !timer.is_finished());
            if let Some(previous) = timers.insert(id, task) {
                previous.abort();
            }
        }
    }

    pub(super) fn cancel(&self, run: Id) {
        if let Ok(mut timers) = self.inner.lock()
            && let Some(task) = timers.remove(&run)
        {
            task.abort();
        }
    }

    #[allow(dead_code)] // inspected by the regression tests
    pub(super) fn live(&self) -> usize {
        self.inner.lock().map(|timers| timers.len()).unwrap_or(0)
    }
}

fn sleep_until_check(deadline: UtcTimestamp) -> tokio::time::Sleep {
    let millis = (deadline.value() - UtcTimestamp::now().value())
        .whole_milliseconds()
        .clamp(50, 600_000) as u64;
    tokio::time::sleep(std::time::Duration::from_millis(millis))
}

impl AgentApplication {
    pub async fn update_human_wait(
        self: &Arc<Self>,
        input: AgentWaitActivityRequest,
    ) -> anyhow::Result<AgentWaitActivityView> {
        if let AgentWaitTarget::LegacyApproval { approval_id } = input.target {
            return self.update_legacy_wait(input, approval_id).await;
        }
        if let AgentWaitTarget::Approval { approval_id } = input.target {
            let (for_user, _) = self.split_reviews(input.session_id, vec![approval_id]).await?;
            anyhow::ensure!(
                !for_user.is_empty(),
                "approval.in_review: 主 Agent 正在处理这项审批。"
            );
        }
        let (view, rearm) = self
            .store
            .write(StoreStep::operation("agent.wait.activity"), move |journal| {
                journal
                    .database
                    .unit_of_work(|tx| human_wait::activity(tx, &input, UtcTimestamp::now()))
            })
            .await?;
        if let Some(run) = rearm {
            self.schedule_expiry(run).await?;
        }
        Ok(view)
    }

    async fn update_legacy_wait(
        self: &Arc<Self>,
        input: AgentWaitActivityRequest,
        id: ApprovalRequestId,
    ) -> anyhow::Result<AgentWaitActivityView> {
        let persistence = Arc::clone(&self.execution.persistence);
        let (wait, rearm) = tokio::task::spawn_blocking(move || {
            persistence.update_legacy_approval_wait(
                input.session_id,
                id,
                input.viewer,
                input.activity,
            )
        })
        .await??;
        if rearm {
            let weak = Arc::downgrade(self);
            let mut deadline = wait.deadline;
            let task = tokio::spawn(async move {
                loop {
                    sleep_until_check(deadline).await;
                    let Some(app) = weak.upgrade() else { return };
                    let persistence = Arc::clone(&app.execution.persistence);
                    drop(app);
                    match tokio::task::spawn_blocking(move || {
                        persistence.expire_legacy_approval_wait(id)
                    })
                    .await
                    {
                        Ok(Ok(Some(next))) => deadline = next,
                        Ok(Ok(None)) => return,
                        failed => {
                            aura_core_base::append_core_log(
                                "approval.legacy_wait_expiry_failed",
                                serde_json::json!({"approval_id": id, "error": format!("{failed:?}")}),
                            );
                            return;
                        }
                    }
                }
            });
            self.legacy_expiry_timers.install(id, task);
        }
        Ok(AgentWaitActivityView {
            deadline_at: wait.deadline,
            focused_until: wait.focused_until(UtcTimestamp::now()),
        })
    }

    pub async fn restore_waits(self: &Arc<Self>) -> anyhow::Result<()> {
        // A branch copy a previous process left unfinished goes on.
        self.resume_branch_copies().await?;
        let runs = self
            .store
            .read(StoreStep::operation("agent.expiry.restore"), recovery::active_runs)
            .await?;
        for run in runs {
            let turn = self.turn(run).await?;
            if turn.state == TurnTaskState::AwaitingApproval
                && self.resume_stale_approval(turn.session_id, Self::stop_deadline()).await?
            {
                continue;
            }
            self.schedule_expiry(run).await?;
        }
        self.wake_peer_queues().await?;
        Ok(())
    }

    pub(super) async fn schedule_expiry(self: &Arc<Self>, run: AgentRunId) -> anyhow::Result<()> {
        let Some(deadline) = self
            .store
            .read(StoreStep::run(run, "agent.expiry.deadline"), move |db| {
                query::wait_deadline(db, run)
            })
            .await
            .map_err(|error| anyhow::anyhow!("{error}"))?
        else {
            return Ok(());
        };
        let weak = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            let mut deadline = deadline;
            loop {
                sleep_until_check(deadline).await;
                let Some(app) = weak.upgrade() else {
                    return;
                };
                let expired = app
                    .store
                    .write(StoreStep::run(run, "agent.expiry.expire"), move |journal| {
                        journal.database.unit_of_work(|tx| {
                            let now = UtcTimestamp::now();
                            Ok(approval::expire(tx, run, now)? || query::expire(tx, run, now)?)
                        })
                    })
                    .await;
                match expired {
                    Ok(true) => {
                        if app.launch(run).is_err() {
                            let _ = app.launch(run);
                        }
                        return;
                    }
                    Ok(false) => {
                        match app
                            .store
                            .read(StoreStep::run(run, "agent.expiry.next_deadline"), move |db| {
                                query::wait_deadline(db, run)
                            })
                            .await
                        {
                            Ok(Some(next)) => deadline = next,
                            Ok(None) => return,
                            Err(error) => {
                                app.note_run_diagnostic(
                                    run,
                                    "expiry_timer",
                                    error.code(),
                                    error.to_string(),
                                );
                                return;
                            }
                        }
                    }
                    Err(error) => {
                        app.note_run_diagnostic(
                            run,
                            "expiry_timer",
                            error.code(),
                            error.to_string(),
                        );
                        return;
                    }
                }
            }
        });
        self.expiry_timers.install(run, task);
        Ok(())
    }
}
