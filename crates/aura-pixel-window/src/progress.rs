//! Work that runs before any app window exists, such as the launcher copying or tidying data: a
//! window appears only when the work outlasts a short delay, shows how far it got, and closes when
//! the work ends. A page with a [`STOP`] button lets the user ask the work to stop; the work
//! decides when it can.
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crate::{
    page::{Action, Id, Page},
    window::PixelWindow,
};

/// Work shorter than this never shows a window, so a quick copy does not flash one.
pub const SHOW_AFTER: Duration = Duration::from_millis(500);
/// The button a progress page offers when its work can stop.
pub const STOP: Id = "stop";

/// How far the work got, shared between the work and the window.
pub struct Progress {
    done: AtomicU64,
    total: AtomicU64,
    finished: AtomicBool,
    /// Work that is not a copy names its stage.
    stage: Mutex<Option<String>>,
    /// The work cannot say how far it got.
    unmeasured: AtomicBool,
    /// The user pressed [`STOP`].
    stop_asked: AtomicBool,
    started: Instant,
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            done: AtomicU64::new(0),
            total: AtomicU64::new(0),
            finished: AtomicBool::new(false),
            stage: Mutex::new(None),
            unmeasured: AtomicBool::new(false),
            stop_asked: AtomicBool::new(false),
            started: Instant::now(),
        }
    }
}

/// What the window shows at one moment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub done: u64,
    pub total: u64,
    pub stage: Option<String>,
    pub unmeasured: bool,
    pub stop_asked: bool,
    pub elapsed: Duration,
}

impl Snapshot {
    /// Done out of one, or `None` while the work cannot say.
    pub fn fraction(&self) -> Option<f64> {
        if self.unmeasured {
            return None;
        }
        if self.total == 0 {
            return Some(0.0);
        }
        Some((self.done as f64 / self.total as f64).min(1.0))
    }
}

impl Progress {
    pub fn set_total(&self, total: u64) {
        self.total.store(total, Ordering::Release);
    }

    pub fn advance(&self, amount: u64) {
        self.done.fetch_add(amount, Ordering::AcqRel);
    }

    /// Sets how far the work got outright, for work that reports its own totals.
    pub fn set_done(&self, done: u64) {
        self.done.store(done, Ordering::Release);
    }

    /// Names the work's current stage; `unmeasured` when it cannot say how far it got.
    pub fn set_stage(&self, stage: &str, unmeasured: bool) {
        if let Ok(mut current) = self.stage.lock() {
            *current = Some(stage.to_owned());
        }
        self.unmeasured.store(unmeasured, Ordering::Release);
    }

    /// Whether the user asked the work to stop.
    pub fn stop_asked(&self) -> bool {
        self.stop_asked.load(Ordering::Acquire)
    }

    pub fn ask_to_stop(&self) {
        self.stop_asked.store(true, Ordering::Release);
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            done: self.done.load(Ordering::Acquire),
            total: self.total.load(Ordering::Acquire),
            stage: self.stage.lock().ok().and_then(|stage| stage.clone()),
            unmeasured: self.unmeasured.load(Ordering::Acquire),
            stop_asked: self.stop_asked(),
            elapsed: self.started.elapsed(),
        }
    }

    fn finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
}

/// Runs `work` on this thread while a window shows the page `page` makes from its progress. The
/// window shows only if `work` is still running after [`SHOW_AFTER`], and closes when it ends.
pub fn with_progress_window<R>(
    page: impl Fn(&Snapshot) -> Page + Send + 'static,
    work: impl FnOnce(&Progress) -> R,
) -> R {
    let progress = Arc::new(Progress::default());
    let shown = Arc::clone(&progress);
    let window = thread::Builder::new().name("aura-progress-window".to_owned()).spawn(move || {
        let start = Instant::now();
        while start.elapsed() < SHOW_AFTER {
            if shown.finished() {
                return;
            }
            thread::sleep(Duration::from_millis(25));
        }
        // A window that cannot be shown leaves the work as it was.
        let first = page(&shown.snapshot());
        if let Ok(mut window) = PixelWindow::open(&first) {
            window.run_until(
                &first,
                || shown.finished(),
                |current| *current = page(&shown.snapshot()),
                |action| {
                    if action == Action::Button(STOP) {
                        shown.ask_to_stop();
                    }
                },
            );
        }
    });
    let result = work(&progress);
    progress.finished.store(true, Ordering::Release);
    if let Ok(window) = window {
        let _ = window.join();
    }
    result
}
