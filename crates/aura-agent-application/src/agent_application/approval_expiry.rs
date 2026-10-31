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
use aura_persistence_v2::agent::{approval, query, recovery};
use std::collections::BTreeMap;

/// The timers one Core owns, at most one per run.
#[derive(Default)]
pub(crate) struct ExpiryTimers {
    inner: Mutex<BTreeMap<AgentRunId, tokio::task::JoinHandle<()>>>,
}

impl ExpiryTimers {
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
    pub(super) fn install(&self, run: AgentRunId, task: tokio::task::JoinHandle<()>) {
        if let Ok(mut timers) = self.inner.lock()
            && let Some(previous) = timers.insert(run, task)
        {
            previous.abort();
        }
    }

    #[allow(dead_code)] // the timers owner is inspected by the regression tests
    pub(super) fn cancel(&self, run: AgentRunId) {
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

impl AgentApplication {
    pub async fn restore_waits(self: &Arc<Self>) -> anyhow::Result<()> {
        // A branch copy a previous process left unfinished goes on.
        self.resume_branch_copies().await?;
        let runs = self
            .store
            .read(StoreStep::operation("agent.expiry.restore"), recovery::active_runs)
            .await?;
        for run in runs {
            // A question left waiting by the previous process is shown again: its countdown
            // starts over instead of ending as soon as Core is back.
            self.store
                .write(StoreStep::run(run, "agent.expiry.restart_question"), move |journal| {
                    journal
                        .database
                        .unit_of_work(|tx| query::restart_countdown(tx, run, UtcTimestamp::now()))
                })
                .await?;
            self.schedule_expiry(run).await?;
        }
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
                let delay = (deadline.value() - UtcTimestamp::now().value())
                    .whole_milliseconds()
                    .clamp(50, 600_000) as u64;
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
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
