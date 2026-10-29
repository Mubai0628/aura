//! Derived index and poll copies continue after the open budget.
//!
//! Opening the store repairs a bounded slice and then returns. This task keeps
//! copying on the journal owner so a large old stream does not wait for the
//! next process start. Each kind is its own transaction. A fault is logged once
//! per pause and does not roll the other kind back.
//!
//! Finding what is left to copy only reads, on the reader connection, so a pass with nothing to
//! copy never takes the write section. Streams are found by one sweep over the steps without
//! coverage, in slices of [`SCAN_SLICE`], and then from the events appended since the last pass.
//! Finding them used to read every event of every step that never streamed text, inside the
//! write section, on every pass: 0.3–1.1 s on a store of a million events, 60 s under load.
//!
//! OWNERSHIP: the task has an explicit owner ([`DerivedBackfill`]) with its own stop flag, and
//! every journal step it runs goes through the bounded Store owner. It used to take
//! `journal.lock()` and run SQLite directly inside an async task, so this background loop parked
//! a Tokio worker whenever another journal step was in the store - the heartbeat and every timer
//! on the same worker stopped with it.
use super::*;
use crate::agent_store::StoreStep;
use aura_persistence_core::PersistenceCoreError;
use aura_persistence_v2::agent::{desktop, stream};
use std::sync::atomic::{AtomicBool, Ordering};

/// How long one discovery read may sweep before other reads get the connection back.
const SCAN_SLICE: std::time::Duration = std::time::Duration::from_millis(50);
/// A wait for the store this long is someone's: copying gives way to it until it ends.
const GIVE_WAY_AFTER: std::time::Duration = std::time::Duration::from_millis(500);

/// The one owner of the derived-backfill task.
#[derive(Default)]
pub(crate) struct DerivedBackfill {
    inner: Mutex<Option<BackfillHandle>>,
}

struct BackfillHandle {
    stop: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}

/// How one kind's pass ended. Only a failed copy has an owner to move past.
enum Pass {
    Done(u32),
    ScanFailed,
    CopyFailed,
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
            // The cursor and the scan are owned by the task and shared with the bounded sections:
            // a section runs on a blocking thread, which needs its own handle to them.
            let poll_cursor = Arc::new(Mutex::new(desktop::PollBackfillCursor::default()));
            let repair_scan = Arc::new(Mutex::new(stream::RepairScan::default()));
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                if task_stop.load(Ordering::Acquire) {
                    return;
                }
                let Some(app) = weak.upgrade() else {
                    return;
                };
                if crate::busy::store_waiting_after(GIVE_WAY_AFTER) {
                    drop(app);
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    continue;
                }
                let streams = copy_streams(&app, &repair_scan).await;
                let polls = copy_polls(&app, &poll_cursor).await;
                if matches!(streams, Pass::CopyFailed) {
                    skip_failed_stream(&app, &repair_scan).await;
                }
                if matches!(polls, Pass::CopyFailed) {
                    // A section that is still running holds the cursor; nothing is logged then.
                    let owner = poll_cursor.try_lock().ok().and_then(|cursor| {
                        cursor.current().map(|(owner, revision)| (owner.to_owned(), revision))
                    });
                    if let Some((owner, revision)) = owner {
                        note_backfill_owner("turn_poll_skip", &owner, Some(revision));
                    }
                }
                let progressed = match (streams, polls) {
                    (Pass::Done(streams), Pass::Done(polls)) => streams.saturating_add(polls),
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

/// The stream kind: a read finds what is left, and only a copy takes the write section.
async fn copy_streams(app: &Arc<AgentApplication>, scan: &Arc<Mutex<stream::RepairScan>>) -> Pass {
    let reading = Arc::clone(scan);
    let found = app
        .store
        .read(StoreStep::operation("agent.backfill.visible_stream_scan"), move |db| {
            let mut scan = lock(&reading)?;
            stream::discover_outstanding(db, &mut scan, Some(Instant::now() + SCAN_SLICE))?;
            Ok((scan.has_outstanding(), scan.sweeping()))
        })
        .await;
    let (outstanding, sweeping) = match found {
        Ok(found) => found,
        Err(error) => {
            note_backfill_fault("visible_stream_scan", &error.to_string());
            return Pass::ScanFailed;
        }
    };
    // A sweep still under way is progress: its next slice follows without the idle pause.
    let swept = u32::from(sweeping);
    if !outstanding {
        return Pass::Done(swept);
    }
    let copying = Arc::clone(scan);
    let copied = app
        .store
        .write(StoreStep::operation("agent.backfill.visible_stream"), move |journal| {
            let mut scan = lock(&copying)?;
            stream::repair_pass(&mut journal.database, &mut scan, UtcTimestamp::now(), 262_144)
        })
        .await;
    match copied {
        Ok(copied) => Pass::Done(copied.saturating_add(swept)),
        Err(error) => {
            note_backfill_fault("visible_stream", &error.to_string());
            Pass::CopyFailed
        }
    }
}

/// The poll kind, with the same read before any write section.
async fn copy_polls(
    app: &Arc<AgentApplication>,
    cursor: &Arc<Mutex<desktop::PollBackfillCursor>>,
) -> Pass {
    let pending = app
        .store
        .read(StoreStep::operation("agent.backfill.turn_poll_scan"), |db| {
            desktop::turn_poll_outstanding(db)
        })
        .await;
    match pending {
        Ok(true) => {}
        Ok(false) => return Pass::Done(0),
        Err(error) => {
            note_backfill_fault("turn_poll_scan", &error.to_string());
            return Pass::ScanFailed;
        }
    }
    let copying = Arc::clone(cursor);
    let polls = app
        .store
        .write(StoreStep::operation("agent.backfill.turn_poll"), move |journal| {
            let mut cursor = lock(&copying)?;
            journal
                .database
                .unit_of_work(|tx| desktop::backfill_turn_poll(tx, 262_144, &mut cursor))
        })
        .await;
    match polls {
        Ok(copied) => Pass::Done(copied),
        Err(error) => {
            note_backfill_fault("turn_poll", &error.to_string());
            Pass::CopyFailed
        }
    }
}

/// Moves the stream round-robin past the owner whose copy failed, so the next pass serves
/// another one; the failed owner comes round again in its turn.
async fn skip_failed_stream(app: &Arc<AgentApplication>, scan: &Arc<Mutex<stream::RepairScan>>) {
    let skipping = Arc::clone(scan);
    let skipped = app
        .store
        .write(StoreStep::operation("agent.backfill.visible_stream_skip"), move |journal| {
            let owner = lock(&skipping)?.current().map(str::to_owned);
            if let Some(owner) = &owner {
                journal.database.unit_of_work(|tx| stream::advance_repair_cursor(tx, owner))?;
            }
            Ok(owner)
        })
        .await;
    match skipped {
        Ok(Some(owner)) => note_backfill_owner("visible_stream_skip", &owner, None),
        Ok(None) => {}
        Err(error) => note_backfill_fault("visible_stream_skip", &error.to_string()),
    }
}

fn lock<T>(shared: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>, PersistenceCoreError> {
    shared.lock().map_err(|_| PersistenceCoreError::Invalid("agent.backfill_cursor_lock"))
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
