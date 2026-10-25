//! Derived index and poll copies continue after the open budget.
//!
//! Opening the store repairs a bounded slice and then returns. This task keeps
//! copying on the journal owner so a large old stream does not wait for the
//! next process start. Each kind is its own transaction. A fault is logged once
//! per pause and does not roll the other kind back.
//!
//! OWNERSHIP: the task has an explicit owner ([`DerivedBackfill`]) with its own stop flag, and
//! every journal step it runs goes through the bounded Store owner. It used to take
//! `journal.lock()` and run SQLite directly inside an async task, so this background loop parked
//! a Tokio worker whenever another journal step was in the store - the heartbeat and every timer
//! on the same worker stopped with it.
use super::*;
use crate::agent_store::StoreStep;
use aura_persistence_v2::agent::{desktop, stream};
use std::sync::atomic::{AtomicBool, Ordering};

/// The one owner of the derived-backfill task.
#[derive(Default)]
pub(crate) struct DerivedBackfill {
    inner: Mutex<Option<BackfillHandle>>,
}

struct BackfillHandle {
    stop: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}

impl DerivedBackfill {
    #[cfg(test)]
    pub(super) async fn stop_and_join_for_test(&self, deadline: Instant) {
        let handle = self.inner.lock().expect("backfill owner").take();
        if let Some(mut handle) = handle {
            handle.stop.store(true, Ordering::Release);
            handle.task.abort();
            let result = tokio::time::timeout_at(deadline.into(), &mut handle.task)
                .await
                .expect("old backfill task must end");
            assert!(result.is_ok() || result.is_err_and(|error| error.is_cancelled()));
        }
    }

    /// Starts the task, replacing any previous one.
    pub(crate) fn start(&self, app: &Arc<AgentApplication>) {
        let stop = Arc::new(AtomicBool::new(false));
        let weak = Arc::downgrade(app);
        let task_stop = Arc::clone(&stop);
        let task = tokio::spawn(async move {
            // The cursor is owned by the task and shared with the bounded section: the section
            // runs on a blocking thread, which needs its own handle to it.
            let poll_cursor = Arc::new(Mutex::new(desktop::PollBackfillCursor::default()));
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                if task_stop.load(Ordering::Acquire) {
                    return;
                }
                let Some(app) = weak.upgrade() else {
                    return;
                };
                let streams = app
                    .store
                    .write(StoreStep::operation("agent.backfill.visible_stream"), |journal| {
                        journal.database.unit_of_work(|tx| {
                            stream::repair_outstanding(tx, UtcTimestamp::now(), 262_144)
                        })
                    })
                    .await;
                let streams = match streams {
                    Ok(copied) => Ok(copied),
                    Err(error) => {
                        note_backfill_fault("visible_stream", &error.to_string());
                        Err(())
                    }
                };
                let polls_cursor = Arc::clone(&poll_cursor);
                let polls = app
                    .store
                    .write(StoreStep::operation("agent.backfill.turn_poll"), move |journal| {
                        let mut cursor = polls_cursor.lock().map_err(|_| {
                            aura_persistence_core::PersistenceCoreError::Invalid(
                                "agent.backfill_cursor_lock",
                            )
                        })?;
                        journal.database.unit_of_work(|tx| {
                            desktop::backfill_turn_poll(tx, 262_144, &mut cursor)
                        })
                    })
                    .await;
                let polls = match polls {
                    Ok(copied) => Ok(copied),
                    Err(error) => {
                        note_backfill_fault("turn_poll", &error.to_string());
                        Err(())
                    }
                };
                if streams.is_err() {
                    let skipped = app
                        .store
                        .write(
                            StoreStep::operation("agent.backfill.visible_stream_skip"),
                            |journal| {
                                journal.database.unit_of_work(|tx| {
                                    let owner = stream::advance_repair_cursor(tx)?;
                                    if let Some(owner) = owner {
                                        note_backfill_owner("visible_stream_skip", &owner, None);
                                    }
                                    Ok::<u32, aura_persistence_core::PersistenceCoreError>(0)
                                })
                            },
                        )
                        .await;
                    if let Err(error) = skipped {
                        note_backfill_fault("visible_stream_skip", &error.to_string());
                    }
                }
                if polls.is_err() {
                    let owner = poll_cursor.lock().ok().and_then(|cursor| {
                        cursor.current().map(|(owner, revision)| (owner.to_owned(), revision))
                    });
                    if let Some((owner, revision)) = owner {
                        note_backfill_owner("turn_poll_skip", &owner, Some(revision));
                    }
                }
                let progressed = match (streams, polls) {
                    (Ok(streams), Ok(polls)) => streams.saturating_add(polls),
                    _ => 0,
                };
                if progressed == 0 {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        });
        if let Ok(mut inner) = self.inner.lock()
            && let Some(previous) = inner.replace(BackfillHandle { stop, task })
        {
            previous.stop.store(true, Ordering::Release);
            previous.task.abort();
        }
    }

    /// Asks the task to stop and reports whether one was really running.
    pub(crate) fn stop(&self) -> bool {
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        match inner.take() {
            Some(handle) => {
                handle.stop.store(true, Ordering::Release);
                handle.task.abort();
                true
            }
            None => false,
        }
    }

    #[allow(dead_code)] // inspected by the regression tests
    #[must_use]
    pub(crate) fn is_running(&self) -> bool {
        self.inner.lock().is_ok_and(|inner| inner.is_some())
    }
}

impl AgentApplication {
    pub fn start_derived_backfill(self: &Arc<Self>) {
        self.derived_backfill.start(self);
    }

    /// The derived-backfill owner, for the regressions and shutdown.
    #[allow(dead_code)] // its ownership is what the regression tests assert
    #[must_use]
    pub(crate) fn derived_backfill(&self) -> &DerivedBackfill {
        &self.derived_backfill
    }
}

fn note_backfill_fault(stage: &str, error: &str) {
    let message = error.chars().take(180).collect::<String>();
    crate::append_core_log(
        "derived_backfill",
        serde_json::json!({ "stage": stage, "error": message }),
    );
}

fn note_backfill_owner(stage: &str, owner: &str, revision: Option<i64>) {
    let owner = owner.chars().take(80).collect::<String>();
    crate::append_core_log(
        "derived_backfill",
        serde_json::json!({ "stage": stage, "owner": owner, "revision": revision }),
    );
}
