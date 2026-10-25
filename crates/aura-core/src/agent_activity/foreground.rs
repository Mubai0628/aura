//! The actual Shell worker owns this sink; the dispatch waiter owns no end responsibility.
use aura_backend_windows_user::ShellExecutionObserver;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

struct State {
    observer: Option<aura_fs_tools::FileExecutionObserver>,
    running: Option<Box<dyn Send>>,
}

pub(crate) struct ForegroundObservation {
    state: Mutex<State>,
    finished: AtomicBool,
    index: aura_activity_runtime::ActivityIndex,
    background: Arc<crate::command_execution::background_observations::BackgroundObservations>,
    attempt: aura_execution_contract::AttemptRef,
}

impl ForegroundObservation {
    pub(crate) fn new(
        observer: Option<aura_fs_tools::FileExecutionObserver>,
        index: aura_activity_runtime::ActivityIndex,
        background: Arc<crate::command_execution::background_observations::BackgroundObservations>,
        attempt: aura_execution_contract::AttemptRef,
    ) -> Self {
        Self {
            state: Mutex::new(State { observer, running: None }),
            finished: AtomicBool::new(false),
            index,
            background,
            attempt,
        }
    }
}

impl ShellExecutionObserver for ForegroundObservation {
    fn started(&self) {
        if self.finished.load(Ordering::Acquire) {
            return;
        }
        let observer = match self.state.try_lock() {
            Ok(mut state) => state.observer.take(),
            Err(_) => {
                self.index.note_missed_publication();
                return;
            }
        };
        let Some(observer) = observer else {
            return;
        };
        let running = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observer(&[])));
        let Ok(running) = running else {
            self.index.note_missed_publication();
            return;
        };
        if self.finished.load(Ordering::Acquire) {
            drop(running);
            return;
        }
        if let Ok(mut state) = self.state.try_lock() {
            if !self.finished.load(Ordering::Acquire) {
                state.running = Some(running);
            }
        } else {
            self.index.note_missed_publication();
        }
    }

    fn foreground_finished(&self) {
        self.finished.store(true, Ordering::Release);
        let retired = match self.state.try_lock() {
            Ok(mut state) => Some((state.observer.take(), state.running.take())),
            Err(_) => {
                self.index.note_missed_publication();
                None
            }
        };
        // End callbacks run outside this information mutex.
        drop(retired);
    }

    fn worker_finished(&self) {
        self.foreground_finished();
        let (id, generation) = super::shell_background_identity(&self.attempt);
        self.index.end(id, generation);
        if self.background.finish_worker(self.attempt.operation_id) {
            self.index.note_missed_publication();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_activity::{AgentActivityLease, PublishedActivity, execution::EndOwner};

    fn fixture() -> (aura_activity_runtime::ActivityIndex, ForegroundObservation) {
        let index = aura_activity_runtime::ActivityIndex::default();
        let lease = Arc::new(AgentActivityLease::new(index.clone()));
        let output = index.clone();
        let callback: aura_fs_tools::FileExecutionObserver = Arc::new(move |_| {
            let record = crate::agent_activity::tests_support::synthetic_record();
            let published = PublishedActivity { id: record.id, generation: record.generation };
            assert!(output.publish(record).was_recorded());
            lease.started(published);
            Box::new(EndOwner(Arc::clone(&lease)))
        });
        let observation = ForegroundObservation::new(
            Some(callback),
            index.clone(),
            Arc::new(
                crate::command_execution::background_observations::BackgroundObservations::default(
                ),
            ),
            aura_execution_contract::AttemptRef {
                operation_id: aura_domain_common::SecurityOperationId::new(),
                ordinal: std::num::NonZeroU32::MIN,
            },
        );
        (index, observation)
    }

    #[test]
    fn worker_keeps_the_row_after_its_dispatch_waiter_has_returned() {
        let (index, observer) = fixture();
        assert_eq!(index.len(), 0, "claimed or queued is not running");
        let worker = Arc::new(observer);
        let waiter = Arc::clone(&worker);
        worker.started();
        worker.started();
        assert_eq!(index.len(), 1, "real start is unique");
        drop(waiter);
        assert_eq!(index.len(), 1, "returning a handle must not end a live worker");
        worker.foreground_finished();
        assert_eq!(index.len(), 0, "foreground completion ends its row before background cleanup");
        worker.started();
        assert_eq!(index.len(), 0, "late start must not resurrect it");
    }

    #[test]
    fn worker_error_and_not_started_paths_leave_no_false_rows() {
        let (index, observer) = fixture();
        drop(observer);
        assert_eq!(index.len(), 0);
        let (index, observer) = fixture();
        observer.started();
        assert_eq!(index.len(), 1);
        drop(observer);
        assert_eq!(index.len(), 0, "a worker error owns the same cleanup responsibility");
    }

    #[test]
    fn a_held_observer_never_blocks_the_worker_or_claims_complete_coverage() {
        let (index, observer) = fixture();
        let hold = observer.state.lock().expect("hold information state");
        observer.started();
        observer.foreground_finished();
        drop(hold);
        assert_eq!(index.len(), 0);
        let view = aura_activity_runtime::ActivityVisibility::for_session(
            aura_domain_common::RuntimeInstanceId::new(),
            aura_domain_common::SessionId::new(),
            aura_domain_common::WorkspaceId::new(),
        );
        assert_eq!(
            index.snapshot(&view).header.coverage.state,
            aura_activity_domain::ActivityCoverageState::Partial
        );
    }
}
