//! An overall time limit for tests that drive real terminals.
//!
//! A test whose wait never ends holds up the whole run, and a wait that blocks the test's own
//! thread cannot be timed out from inside the test. The limit is therefore kept by a thread of its
//! own: past it, the test's name and the last step it reached are printed and the run ends with a
//! failure status, so a hang becomes a failure that says where it stopped.
use std::{
    sync::{Arc, Condvar, Mutex, PoisonError},
    time::Duration,
};

/// Far beyond what any of these tests takes on a loaded machine, and far short of a stalled run.
const WITHIN: Duration = Duration::from_secs(300);

struct State {
    step: &'static str,
    finished: bool,
}

/// The running limit of one test; the test has finished when this is dropped.
pub(crate) struct Limit {
    shared: Arc<(Mutex<State>, Condvar)>,
}

/// Starts the limit for `test`.
pub(crate) fn limit(test: &'static str) -> Limit {
    limit_within(test, WITHIN)
}

fn limit_within(test: &'static str, within: Duration) -> Limit {
    let shared = Arc::new((Mutex::new(State { step: "start", finished: false }), Condvar::new()));
    let watched = Arc::clone(&shared);
    std::thread::spawn(move || {
        let (state, finished) = &*watched;
        let state = state.lock().unwrap_or_else(PoisonError::into_inner);
        let (state, _) = finished
            .wait_timeout_while(state, within, |state| !state.finished)
            .unwrap_or_else(PoisonError::into_inner);
        if !state.finished {
            eprintln!("{test} has not finished after {within:?}; its last step was {}", state.step);
            std::process::exit(101);
        }
    });
    Limit { shared }
}

impl Limit {
    /// The step the test has reached, named in the message if the limit runs out.
    pub(crate) fn step(&self, step: &'static str) {
        self.shared.0.lock().unwrap_or_else(PoisonError::into_inner).step = step;
    }
}

impl Drop for Limit {
    fn drop(&mut self) {
        let (state, finished) = &*self.shared;
        state.lock().unwrap_or_else(PoisonError::into_inner).finished = true;
        finished.notify_all();
    }
}

/// Only run by the test below, in a process of its own: a test that never finishes.
#[test]
#[ignore = "run by a_test_past_its_limit_ends_the_run_naming_its_last_step in its own process"]
fn stalled_probe() {
    let limit = limit_within("stalled_probe", Duration::from_secs(1));
    limit.step("waiting for nothing");
    std::thread::sleep(Duration::from_secs(30));
}

#[test]
fn a_test_past_its_limit_ends_the_run_naming_its_last_step() {
    let ran = std::process::Command::new(std::env::current_exe().expect("this test program"))
        .args(["test_watchdog::stalled_probe", "--exact", "--ignored", "--nocapture"])
        .output()
        .expect("the probe runs");
    let message = String::from_utf8_lossy(&ran.stderr);
    assert_eq!(ran.status.code(), Some(101), "{message}");
    assert!(
        message.contains(
            "stalled_probe has not finished after 1s; its last step was waiting for nothing"
        ),
        "{message}"
    );
}

#[test]
fn a_test_that_finishes_in_time_ends_nothing() {
    let limit = limit_within("finishes", Duration::from_millis(200));
    limit.step("done");
    drop(limit);
    std::thread::sleep(Duration::from_millis(400));
}
