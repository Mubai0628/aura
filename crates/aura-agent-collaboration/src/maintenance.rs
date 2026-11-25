use aura_domain_common::{AgentRunId, RuntimeInstanceId, SessionId};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::watch;

pub struct ProfileMaintenance {
    pending: Mutex<BTreeMap<SessionId, Option<AgentRunId>>>,
    stopped: watch::Sender<bool>,
    pub enabled: AtomicBool,
}
impl Default for ProfileMaintenance {
    fn default() -> Self {
        Self {
            pending: Mutex::new(BTreeMap::new()),
            stopped: watch::channel(false).0,
            enabled: AtomicBool::new(!cfg!(feature = "test-hooks")),
        }
    }
}
impl ProfileMaintenance {
    pub fn for_runtime(runtime: RuntimeInstanceId) -> Arc<Self> {
        type Owners = Mutex<BTreeMap<RuntimeInstanceId, Weak<ProfileMaintenance>>>;
        static OWNERS: OnceLock<Owners> = OnceLock::new();
        let mut owners =
            OWNERS.get_or_init(Mutex::default).lock().unwrap_or_else(|e| e.into_inner());
        owners.retain(|_, owner| owner.strong_count() > 0);
        if let Some(owner) = owners.get(&runtime).and_then(Weak::upgrade) {
            return owner;
        }
        let owner = Arc::new(Self::default());
        owners.insert(runtime, Arc::downgrade(&owner));
        owner
    }
    /// Only the first queued run starts a worker; later turns replace pending work with the latest.
    pub fn enqueue(&self, session: SessionId, run: AgentRunId) -> bool {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if *self.stopped.borrow() || !self.enabled.load(Ordering::Acquire) {
            return false;
        }
        if let Some(queued) = pending.get_mut(&session) {
            *queued = Some(run);
            return false;
        }
        if pending.len() >= 32 {
            return false;
        }
        pending.insert(session, Some(run));
        true
    }
    pub fn next(&self, session: SessionId) -> Option<AgentRunId> {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if *self.stopped.borrow() {
            pending.remove(&session);
            return None;
        }
        let next = pending.get_mut(&session).and_then(Option::take);
        if next.is_none() {
            pending.remove(&session);
        }
        next
    }
    pub fn stop(&self) {
        self.stopped.send_replace(true);
    }
    pub async fn cancelled(&self) {
        let mut rx = self.stopped.subscribe();
        while !*rx.borrow() {
            if rx.changed().await.is_err() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_agent_queue_coalesces_turns_and_keeps_one_runtime_owner() {
        let runtime = RuntimeInstanceId::new();
        let owner = ProfileMaintenance::for_runtime(runtime);
        assert!(Arc::ptr_eq(&owner, &ProfileMaintenance::for_runtime(runtime)));
        owner.enabled.store(true, Ordering::Release);
        let session = SessionId::new();
        let first = AgentRunId::new();
        let last = AgentRunId::new();
        assert!(owner.enqueue(session, first));
        assert!(!owner.enqueue(session, last));
        assert_eq!(owner.next(session), Some(last));
        assert_eq!(owner.next(session), None);
        owner.stop();
        assert!(!owner.enqueue(session, first));
    }
}
