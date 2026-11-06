//! The background retry of the credentials completed deletions left: it keeps trying until none is
//! left, one retry runs at a time, and a deletion that ends while it is stopping is still retried.
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aura_persistence::CredentialMutationJournalRecord;

use super::{CredentialCleanup, RetriedDeletes, RetryState, retry_in_background};
use crate::p05_credential::{CredentialDeleteChildOutcome, P05CredentialJournalError};

/// Answers each retry, and each check after a retry that left nothing, from a script.
#[derive(Clone, Default)]
struct Scripted {
    left_after_retry: Arc<Mutex<VecDeque<usize>>>,
    pending_when_stopping: Arc<Mutex<VecDeque<bool>>>,
    retries: Arc<AtomicUsize>,
}

impl Scripted {
    fn new(left_after_retry: &[usize], pending_when_stopping: &[bool]) -> Self {
        Self {
            left_after_retry: Arc::new(Mutex::new(left_after_retry.iter().copied().collect())),
            pending_when_stopping: Arc::new(Mutex::new(
                pending_when_stopping.iter().copied().collect(),
            )),
            retries: Arc::default(),
        }
    }

    fn retries(&self) -> usize {
        self.retries.load(Ordering::SeqCst)
    }
}

impl CredentialCleanup for Scripted {
    fn delete_published_child(
        &self,
        _: CredentialMutationJournalRecord,
    ) -> Result<CredentialDeleteChildOutcome, P05CredentialJournalError> {
        unreachable!("the background retry goes through retry_pending_deletes")
    }

    fn retry_pending_deletes(&self) -> Result<RetriedDeletes, P05CredentialJournalError> {
        self.retries.fetch_add(1, Ordering::SeqCst);
        let pending = self.left_after_retry.lock().expect("script").pop_front().unwrap_or(0);
        Ok(RetriedDeletes { removed: 0, pending })
    }

    fn has_pending_deletes(&self) -> bool {
        self.pending_when_stopping.lock().expect("script").pop_front().unwrap_or(false)
    }
}

/// Delays that record whether a retry thread ever waited on them, and when they are dropped.
struct Watched {
    waited: Arc<AtomicBool>,
    dropped: Arc<AtomicBool>,
}

impl Iterator for Watched {
    type Item = Duration;

    fn next(&mut self) -> Option<Duration> {
        self.waited.store(true, Ordering::SeqCst);
        None
    }
}

impl Drop for Watched {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

fn soon() -> impl Iterator<Item = Duration> + Send + 'static {
    std::iter::repeat(Duration::from_millis(1))
}

fn running(state: &RetryState) -> bool {
    state.running.load(Ordering::SeqCst)
}

fn wait_for(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "the background retry did not get there");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn the_retry_keeps_trying_until_nothing_is_left_and_can_start_again_after() {
    let cleanup = Scripted::new(&[2, 1, 0], &[]);
    let state = Arc::new(RetryState::default());
    retry_in_background(cleanup.clone(), Arc::clone(&state), soon());
    wait_for(|| cleanup.retries() == 3 && !running(&state));

    cleanup.left_after_retry.lock().expect("script").push_back(0);
    retry_in_background(cleanup.clone(), Arc::clone(&state), soon());
    wait_for(|| cleanup.retries() == 4 && !running(&state));
}

#[test]
fn a_retry_asked_for_while_one_runs_does_not_start_a_second() {
    let cleanup = Scripted::new(&[], &[]);
    let state = Arc::new(RetryState::default());
    state.running.store(true, Ordering::SeqCst);
    let waited = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicBool::new(false));
    retry_in_background(
        cleanup.clone(),
        Arc::clone(&state),
        Watched { waited: Arc::clone(&waited), dropped: Arc::clone(&dropped) },
    );
    // Given to a thread, the delays are dropped only after that thread waited on them.
    assert!(dropped.load(Ordering::SeqCst), "the delays went to a second thread");
    assert!(!waited.load(Ordering::SeqCst), "a second thread waited on the delays");
    assert!(running(&state), "the running retry keeps its place");
    assert_eq!(cleanup.retries(), 0);
}

#[test]
fn a_deletion_that_ends_while_the_retry_stops_is_still_retried() {
    let cleanup = Scripted::new(&[0, 0], &[true, false]);
    let state = Arc::new(RetryState::default());
    retry_in_background(cleanup.clone(), Arc::clone(&state), soon());
    wait_for(|| cleanup.retries() == 2 && !running(&state));
}
