//! The signal that a workspace or file projection changed, for the views that follow it.
use super::*;

#[derive(Clone)]
pub(super) struct WorkspaceProjectionSignals {
    pub(super) revision: Arc<AtomicU64>,
    subscribers: Arc<Mutex<Vec<Sender<Revision>>>>,
}

impl WorkspaceProjectionSignals {
    pub(super) fn new() -> Self {
        Self {
            revision: Arc::new(AtomicU64::new(Revision::INITIAL.value())),
            subscribers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub(super) fn subscribe(&self) -> Result<Receiver<Revision>, WorkspaceError> {
        let (sender, receiver) = mpsc::channel();
        self.subscribers
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace projection subscribers"))?
            .push(sender);
        Ok(receiver)
    }

    pub(super) fn bump(&self) -> Result<Revision, WorkspaceError> {
        let next = self
            .revision
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| current.checked_add(1))
            .map_err(|_| WorkspaceError::RevisionExhausted)?
            .checked_add(1)
            .ok_or(WorkspaceError::RevisionExhausted)?;
        let revision = Revision::new(next);
        let mut subscribers = self
            .subscribers
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace projection subscribers"))?;
        subscribers.retain(|subscriber| subscriber.send(revision).is_ok());
        Ok(revision)
    }
}
