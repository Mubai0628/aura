//! Admission is held through launch, so shutdown waits for every in-flight registration.
use super::{TerminalEndReason, TerminalKind, TerminalSession};
use anyhow::{Result, ensure};
use aura_domain_common::{SessionId, UtcTimestamp};
use aura_windows_user_contract::ManagedShell;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, RwLock},
};

#[derive(Clone, Debug)]
pub struct TerminalSummary {
    pub id: String,
    pub session: SessionId,
    pub kind: TerminalKind,
    pub shell: ManagedShell,
    pub process_id: u32,
    pub started_at: UtcTimestamp,
}
struct Entry {
    summary: TerminalSummary,
    terminal: Arc<TerminalSession>,
}
#[derive(Default)]
struct Lifecycle {
    stopped: bool,
    deleted_sessions: BTreeSet<SessionId>,
}
#[derive(Default)]
pub struct TerminalRegistry {
    lifecycle: RwLock<Lifecycle>,
    entries: Mutex<BTreeMap<String, Entry>>,
}
impl TerminalRegistry {
    pub fn open(
        &self,
        session: SessionId,
        launch: impl FnOnce() -> Result<TerminalSession>,
    ) -> Result<TerminalSummary> {
        let lifecycle =
            self.lifecycle.read().map_err(|_| anyhow::anyhow!("terminal.registry_lock"))?;
        ensure!(!lifecycle.stopped, "terminal.core_stopped; terminals cannot survive Core restart");
        ensure!(!lifecycle.deleted_sessions.contains(&session), "terminal.session_deleted");
        let mut random = [0u8; 16];
        getrandom::fill(&mut random)
            .map_err(|_| anyhow::anyhow!("terminal.identity_unavailable"))?;
        let id =
            format!("term_{}", random.iter().map(|byte| format!("{byte:02x}")).collect::<String>());
        let terminal = Arc::new(launch()?);
        let summary = TerminalSummary {
            id: id.clone(),
            session,
            kind: terminal.kind,
            shell: terminal.shell,
            process_id: terminal.identity().process_id,
            started_at: UtcTimestamp::now(),
        };
        let mut entries =
            self.entries.lock().map_err(|_| anyhow::anyhow!("terminal.registry_lock"))?;
        ensure!(!entries.contains_key(&id), "terminal.identity_collision");
        entries.insert(id, Entry { summary: summary.clone(), terminal });
        drop(lifecycle);
        Ok(summary)
    }
    pub fn get(&self, session: SessionId, id: &str) -> Result<Arc<TerminalSession>> {
        let entries = self.entries.lock().map_err(|_| anyhow::anyhow!("terminal.registry_lock"))?;
        let entry = entries
            .get(id)
            .filter(|entry| entry.summary.session == session)
            .ok_or_else(|| anyhow::anyhow!("terminal.not_found_in_session"))?;
        Ok(Arc::clone(&entry.terminal))
    }
    pub fn list(&self, session: SessionId) -> Result<Vec<TerminalSummary>> {
        Ok(self
            .entries
            .lock()
            .map_err(|_| anyhow::anyhow!("terminal.registry_lock"))?
            .values()
            .filter(|entry| entry.summary.session == session)
            .map(|entry| entry.summary.clone())
            .collect())
    }
    pub fn close_session(&self, session: SessionId) -> Result<()> {
        self.lifecycle
            .write()
            .map_err(|_| anyhow::anyhow!("terminal.registry_lock"))?
            .deleted_sessions
            .insert(session);
        let terminals = self.list(session)?;
        let mut first_error = None;
        for summary in terminals {
            if let Err(error) =
                self.get(session, &summary.id)?.close(TerminalEndReason::SessionDeleted)
            {
                first_error.get_or_insert(error);
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        self.entries
            .lock()
            .map_err(|_| anyhow::anyhow!("terminal.registry_lock"))?
            .retain(|_, entry| entry.summary.session != session);
        Ok(())
    }
    /// Remove only after confirmed stop; unconfirmed resources remain visible for recovery.
    pub fn close_and_remove(
        &self,
        session: SessionId,
        id: &str,
        reason: TerminalEndReason,
    ) -> Result<super::TerminalEnd> {
        let end = self.get(session, id)?.close(reason)?;
        self.entries.lock().map_err(|_| anyhow::anyhow!("terminal.registry_lock"))?.remove(id);
        Ok(end)
    }
    pub fn sessions(&self) -> Result<Vec<SessionId>> {
        Ok(self
            .entries
            .lock()
            .map_err(|_| anyhow::anyhow!("terminal.registry_lock"))?
            .values()
            .map(|v| v.summary.session)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect())
    }
    pub fn shutdown(&self) -> Result<()> {
        self.lifecycle.write().map_err(|_| anyhow::anyhow!("terminal.registry_lock"))?.stopped =
            true;
        let terminals = self
            .entries
            .lock()
            .map_err(|_| anyhow::anyhow!("terminal.registry_lock"))?
            .values()
            .map(|entry| Arc::clone(&entry.terminal))
            .collect::<Vec<_>>();
        let mut first_error = None;
        for terminal in terminals {
            if let Err(error) = terminal.close(TerminalEndReason::CoreStopped) {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}
impl Drop for TerminalRegistry {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!("terminal registry cleanup unconfirmed: {error}");
        }
    }
}
