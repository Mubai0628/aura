//! Test support: a change to a terminal's record held in progress, so a test can order a takeover
//! against it.
use super::*;
use std::sync::mpsc;

/// Another thread holding the live terminal and then saving its record, as a capture did until
/// A takeover sets the live controller first, so it waits here and must then change the
/// record as saved, not a copy read before.
pub struct CaptureInProgress {
    release: mpsc::Sender<()>,
    thread: std::thread::JoinHandle<Result<()>>,
}

impl CaptureInProgress {
    /// Saves the record with its revision moved on, as a capture saves it, then lets go of the lock.
    pub fn finish(self) -> Result<()> {
        self.release.send(()).ok();
        self.thread.join().map_err(|_| anyhow::anyhow!("capture thread panicked"))?
    }
}

impl AgentTerminals {
    /// Returns once the lock is held; `bump` is how far the save moves the revision on.
    pub fn capture_in_progress_for_test(
        self: &Arc<Self>,
        session: SessionId,
        id: &str,
        bump: u64,
    ) -> Result<CaptureInProgress> {
        let entry = self.entry(id)?;
        let owner = self.clone();
        let id = id.to_owned();
        let (locked, held) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        let thread = std::thread::spawn(move || -> Result<()> {
            let _live = entry.lock().map_err(|_| anyhow::anyhow!("terminal.owner_lock"))?;
            locked.send(()).ok();
            released.recv().ok();
            let mut record = owner.record(session, &id)?;
            record.view.revision = Revision::new(record.view.revision.value() + bump);
            owner.journal.lock()?.database.unit_of_work(|tx| records::save(tx, &record))?;
            Ok(())
        });
        held.recv()?;
        Ok(CaptureInProgress { release, thread })
    }
}

impl CommandExecution {
    pub fn agent_terminal_record_for_test(
        &self,
        session: SessionId,
        id: &str,
    ) -> Result<AgentTerminalRecord> {
        self.agent_terminals.record(session, id)
    }

    pub fn agent_terminal_capture_in_progress_for_test(
        &self,
        session: SessionId,
        id: &str,
        bump: u64,
    ) -> Result<CaptureInProgress> {
        self.agent_terminals.capture_in_progress_for_test(session, id, bump)
    }
}
