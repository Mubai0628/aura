//! Core-owned operation lifetimes. Only explicit cancellation stops an active
//! request; a later permission change governs the next actual start.
use anyhow::{Result, ensure};
use aura_domain_common::{SecurityOperationId, SessionId};
use aura_security_contract::PermissionMode;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityCapability {
    ControlledWeb,
}
struct Entry {
    session: SessionId,
    mode: PermissionMode,
    started: bool,
    cancel: watch::Sender<bool>,
}
#[derive(Clone, Default)]
pub struct AgentActivities(Arc<Mutex<BTreeMap<SecurityOperationId, Entry>>>);
pub struct ActivityGuard {
    id: SecurityOperationId,
    registry: AgentActivities,
    pub cancel: watch::Receiver<bool>,
}
impl AgentActivities {
    pub fn register(
        &self,
        id: SecurityOperationId,
        session: SessionId,
        _capability: ActivityCapability,
        mode: PermissionMode,
    ) -> Result<ActivityGuard> {
        // Registration follows the transactional dispatch-authority check. Mode
        // defaults alone cannot veto an explicit Once/Session approval here.
        let mut all = self.0.lock().map_err(|_| anyhow::anyhow!("activity.lock"))?;
        ensure!(!all.contains_key(&id), "activity.duplicate");
        let (cancel, receiver) = watch::channel(false);
        all.insert(id, Entry { session, mode, started: false, cancel });
        Ok(ActivityGuard { id, registry: self.clone(), cancel: receiver })
    }
    pub fn started_mode(
        &self,
        id: SecurityOperationId,
        session: SessionId,
    ) -> Result<Option<PermissionMode>> {
        let all = self.0.lock().map_err(|_| anyhow::anyhow!("activity.lock"))?;
        let entry = all.get(&id).ok_or_else(|| anyhow::anyhow!("activity.missing"))?;
        ensure!(entry.session == session, "activity.session_mismatch");
        Ok(entry.started.then_some(entry.mode))
    }
    pub fn mark_started(&self, id: SecurityOperationId) -> Result<()> {
        let mut all = self.0.lock().map_err(|_| anyhow::anyhow!("activity.lock"))?;
        let entry = all.get_mut(&id).ok_or_else(|| anyhow::anyhow!("activity.missing"))?;
        entry.started = true;
        Ok(())
    }
    pub fn cancel(&self, id: SecurityOperationId) -> Result<()> {
        let all = self.0.lock().map_err(|_| anyhow::anyhow!("activity.lock"))?;
        if let Some(v) = all.get(&id) {
            let _ = v.cancel.send(true);
        }
        Ok(())
    }
}
impl Drop for ActivityGuard {
    fn drop(&mut self) {
        // The caller owns and must drop the actual request future before this
        // guard. A signal by itself is never considered completion.
        if let Ok(mut all) = self.registry.0.lock() {
            all.remove(&self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn started_authority_is_operation_scoped_and_manual_stop_stays_available() {
        let registry = AgentActivities::default();
        let session = SessionId::new();
        let id = SecurityOperationId::new();
        let old = registry
            .register(id, session, ActivityCapability::ControlledWeb, PermissionMode::FullAccess)
            .expect("old request");
        assert_eq!(registry.started_mode(id, session).expect("reserved"), None);
        registry.mark_started(id).expect("started");
        assert_eq!(
            registry.started_mode(id, session).expect("running"),
            Some(PermissionMode::FullAccess)
        );
        let approved_readonly = registry
            .register(
                SecurityOperationId::new(),
                session,
                ActivityCapability::ControlledWeb,
                PermissionMode::ReadOnly,
            )
            .expect("an approved request may run under ReadOnly");
        assert!(!*approved_readonly.cancel.borrow());
        assert!(registry.started_mode(id, SessionId::new()).is_err());
        assert!(!*old.cancel.borrow());
        registry.cancel(id).expect("manual stop");
        assert!(*old.cancel.borrow());
        drop(old);
        assert!(registry.started_mode(id, session).is_err());
    }
}
