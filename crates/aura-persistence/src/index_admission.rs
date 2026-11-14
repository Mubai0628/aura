//! Index work with a budget of its own, offered to the writer again while the index lane is full.
//! A full lane admits nothing and runs nothing, so asking again is safe; without it such work fails
//! whenever other index work fills the lane, as the online workspaces' does right after Core
//! starts, although its budget allows it to wait.
use std::time::{Duration, Instant};

use rusqlite::Connection;

use crate::{PersistenceCoordinator, PersistenceError, writer_queue::WorkClass};

/// How long the work waits before it asks for a place again.
const RETRY: Duration = Duration::from_millis(20);

impl PersistenceCoordinator {
    /// Runs the index job `job` makes once the index lane has room, before `deadline`. Each
    /// attempt makes the job again, since a refused one is dropped unrun. Every other answer,
    /// the job's own included, is returned as it came.
    pub(crate) fn dispatch_index_with_room<T, F>(
        &self,
        deadline: Instant,
        job: impl Fn() -> F,
    ) -> Result<T, PersistenceError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, PersistenceError> + Send + 'static,
    {
        loop {
            match self.dispatch_within(WorkClass::Index, deadline, job()) {
                Err(PersistenceError::WriterBusy { .. }) if Instant::now() + RETRY < deadline => {
                    std::thread::sleep(RETRY);
                }
                answer => return answer,
            }
        }
    }
}
