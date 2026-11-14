//! How the execution layer reaches the team: one port, bound to the application once it runs. It
//! also keeps what the team knows only while Aura runs: which conversations wait in
//! `agents.wait`, which have a model answer under way, how far each conversation's tokens have
//! been counted, and what the watchdog saw.
use aura_domain_common::{SessionId, ToolCallId, TurnTaskId};
use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex, PoisonError, RwLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::Notify;

/// One `agents.*` or `agent.report` call, as the execution layer admitted it.
#[derive(Clone, Debug)]
pub struct TeamCall {
    pub session_id: SessionId,
    pub turn_task_id: TurnTaskId,
    pub tool_call_id: ToolCallId,
    pub tool: String,
    pub arguments: serde_json::Value,
    /// The tools of the calling conversation's table, by name.
    pub own_tools: Vec<String>,
}

pub type TeamFuture = Pin<Box<dyn Future<Output = Result<serde_json::Value, String>> + Send>>;
pub type WatchFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

/// Runs team calls. The error is the text the model is shown.
pub trait TeamTools: Send + Sync {
    fn call(self: Arc<Self>, call: TeamCall) -> TeamFuture;

    /// One pass of the watchdog, `elapsed` after the last one.
    fn watch(self: Arc<Self>, elapsed: Duration) -> WatchFuture;
}

/// A conversation's tokens counted up to an event of the store.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    /// The newest event counted; later ones are not.
    pub after: i64,
    pub tokens: u64,
}

/// At most this many conversations' tallies are kept; past it they are counted again from the
/// start when next asked.
const TALLIES_MAX: usize = 256;

#[derive(Default)]
pub struct TeamPort {
    tools: RwLock<Option<Weak<dyn TeamTools>>>,
    news: Arc<Notify>,
    waiting: Marks,
    answering: Marks,
    tallies: Mutex<HashMap<SessionId, Tally>>,
    sizes: crate::FolderSizes,
    watchdog: crate::Watchdog,
    watching: AtomicBool,
}

impl TeamPort {
    /// Binds the port to the application now running; a later application replaces it.
    pub fn bind(&self, tools: Weak<dyn TeamTools>) {
        let mut bound = self.tools.write().unwrap_or_else(PoisonError::into_inner);
        let same = bound.as_ref().is_some_and(|current| Weak::ptr_eq(current, &tools));
        if !same {
            *bound = Some(tools);
        }
    }

    #[must_use]
    pub fn tools(&self) -> Option<Arc<dyn TeamTools>> {
        self.tools.read().unwrap_or_else(PoisonError::into_inner).as_ref().and_then(Weak::upgrade)
    }

    /// What the watchdog saw of the running sub-agents.
    #[must_use]
    pub fn watchdog(&self) -> &crate::Watchdog {
        &self.watchdog
    }

    /// True the first time only: the caller starts the watchdog's clock.
    #[must_use]
    pub fn start_watching(&self) -> bool {
        !self.watching.swap(true, Ordering::SeqCst)
    }

    /// The worktree folders' sizes counted so far.
    #[must_use]
    pub fn sizes(&self) -> &crate::FolderSizes {
        &self.sizes
    }

    /// Woken whenever a sub-agent hands in a report or a turn of one ends.
    #[must_use]
    pub fn news(&self) -> Arc<Notify> {
        Arc::clone(&self.news)
    }

    /// Marks `session` as waiting in `agents.wait` until the guard is dropped. The wait runs
    /// before its call is admitted, so no running call shows it.
    #[must_use]
    pub fn waiting(&self, session: SessionId) -> Mark {
        self.waiting.mark(session)
    }

    #[must_use]
    pub fn is_waiting(&self, session: SessionId) -> bool {
        self.waiting.has(session)
    }

    /// Marks a model answer of `session` as under way until the guard is dropped: from its wait
    /// for the endpoint to its last event.
    #[must_use]
    pub fn answering(&self, session: SessionId) -> Mark {
        self.answering.mark(session)
    }

    #[must_use]
    pub fn is_answering(&self, session: SessionId) -> bool {
        self.answering.has(session)
    }

    /// The provider sent something for `session`'s answer under way.
    pub fn heard(&self, session: SessionId) {
        self.answering.touch(session);
    }

    /// How long `session`'s answer under way has been, and how long since its provider last sent
    /// anything; none while no answer is under way.
    #[must_use]
    pub fn answer_times(&self, session: SessionId) -> Option<(Duration, Duration)> {
        self.answering.times(session)
    }

    /// How far `session`'s tokens have been counted; nothing yet is the default.
    #[must_use]
    pub fn tally(&self, session: SessionId) -> Tally {
        self.tallies
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&session)
            .copied()
            .unwrap_or_default()
    }

    /// Moves `session`'s tally from `from` to `to`, unless another count moved it first; returns
    /// the tally kept. Two counts that started at the same place never add the same events twice.
    pub fn settle_tally(&self, session: SessionId, from: Tally, to: Tally) -> Tally {
        let mut tallies = self.tallies.lock().unwrap_or_else(PoisonError::into_inner);
        let current = tallies.get(&session).copied().unwrap_or_default();
        if current != from {
            return current;
        }
        if tallies.len() >= TALLIES_MAX && !tallies.contains_key(&session) {
            tallies.clear();
        }
        tallies.insert(session, to);
        to
    }
}

/// Conversations marked while a guard of theirs lives; one may be marked more than once at a time.
#[derive(Default)]
struct Marks(Arc<Mutex<HashMap<SessionId, Marked>>>);

/// How many guards mark a conversation, since when, and when it was last heard from.
struct Marked {
    count: usize,
    since: Instant,
    heard: Instant,
}

impl Marks {
    fn mark(&self, session: SessionId) -> Mark {
        let now = Instant::now();
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(session)
            .or_insert(Marked { count: 0, since: now, heard: now })
            .count += 1;
        Mark { marks: Arc::clone(&self.0), session }
    }

    fn has(&self, session: SessionId) -> bool {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).contains_key(&session)
    }

    fn touch(&self, session: SessionId) {
        if let Some(marked) =
            self.0.lock().unwrap_or_else(PoisonError::into_inner).get_mut(&session)
        {
            marked.heard = Instant::now();
        }
    }

    fn times(&self, session: SessionId) -> Option<(Duration, Duration)> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&session)
            .map(|marked| (marked.since.elapsed(), marked.heard.elapsed()))
    }
}

/// While it lives, its conversation stays marked: waiting in `agents.wait`, or with a model answer
/// under way.
pub struct Mark {
    marks: Arc<Mutex<HashMap<SessionId, Marked>>>,
    session: SessionId,
}

impl Drop for Mark {
    fn drop(&mut self) {
        let mut marks = self.marks.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(marked) = marks.get_mut(&self.session) {
            marked.count -= 1;
            if marked.count == 0 {
                marks.remove(&self.session);
            }
        }
    }
}
