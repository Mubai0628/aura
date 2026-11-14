//! Main is quitting: `aura.db` writes its log back into the file and empties it, as the execution
//! store does at a normal exit, so a log that a large write left behind is not kept. The same step
//! marks a run owed before the next start when enough of the file is free, or in use beyond what
//! the last run kept (`slimming::note_at_exit`); it reads only the page counts.
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::{PersistenceCoordinator, PersistenceError, WorkClass};

/// How long the step may take, waiting for the writer and for readers to let go of the log
/// included: Main stops Core 3 s after asking it to prepare its exit.
const EXIT_BUDGET: Duration = Duration::from_millis(1_500);

/// What a normal exit did to `aura.db`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ExitWriteBack {
    /// The log was written back into the file and emptied.
    pub log_emptied: bool,
    /// A run is owed before the next start.
    pub slimming_owed: bool,
}

impl PersistenceCoordinator {
    /// On the writer, within `EXIT_BUDGET`: writes the log back and empties it, then marks a run
    /// owed when one is. The store goes on serving until Core ends.
    pub fn prepare_exit(&self) -> Result<ExitWriteBack, PersistenceError> {
        let database = self.database_path().to_path_buf();
        self.dispatch_within(WorkClass::Sync, Instant::now() + EXIT_BUDGET, move |connection| {
            let busy = connection
                .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get::<_, i64>(0))?;
            let slimming_owed = crate::slimming::note_at_exit(connection, &database)?;
            Ok(ExitWriteBack { log_emptied: busy == 0, slimming_owed })
        })
    }
}
