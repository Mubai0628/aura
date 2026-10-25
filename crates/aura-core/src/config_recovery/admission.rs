//! Related recovery requests wait asynchronously, before the four general storage permits.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, OnceLock, Weak},
};

#[derive(Default)]
pub(super) struct ApplyAdmission {
    held: Mutex<BTreeSet<String>>,
    changed: tokio::sync::Notify,
}
impl ApplyAdmission {
    pub fn for_owner(owner: &Arc<aura_config::SettingsFacade>) -> Arc<Self> {
        static OWNERS: OnceLock<Mutex<BTreeMap<usize, Weak<ApplyAdmission>>>> = OnceLock::new();
        let mut owners =
            OWNERS.get_or_init(Mutex::default).lock().unwrap_or_else(|e| e.into_inner());
        owners.retain(|_, entry| entry.strong_count() > 0);
        let key = Arc::as_ptr(owner) as usize;
        let admission =
            owners.get(&key).and_then(Weak::upgrade).unwrap_or_else(|| Arc::new(Self::default()));
        owners.insert(key, Arc::downgrade(&admission));
        admission
    }
    pub async fn claim(
        self: &Arc<Self>,
        keys: BTreeSet<String>,
    ) -> Result<ApplyGuard, &'static str> {
        let deadline =
            tokio::time::Instant::now() + crate::service_rebuild::CONSTRUCTION_RESPONSIBILITY_WAIT;
        loop {
            // Enable before checking to avoid a release lost between check and registration.
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut held =
                    self.held.lock().map_err(|_| "config.recovery.ownership_unavailable")?;
                if held.is_disjoint(&keys) {
                    held.extend(keys.clone());
                    return Ok(ApplyGuard { owner: Arc::clone(self), keys });
                }
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return Err("config.recovery.construction_contended");
            }
        }
    }
}
pub(crate) struct ApplyGuard {
    owner: Arc<ApplyAdmission>,
    keys: BTreeSet<String>,
}
impl Drop for ApplyGuard {
    fn drop(&mut self) {
        if let Ok(mut held) = self.owner.held.lock() {
            for key in &self.keys {
                held.remove(key);
            }
        }
        self.owner.changed.notify_waiters();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        blocking_pool::BlockingAdmission,
        sync_boundary::{SyncBoundary, SyncSection, SyncSubject},
    };
    fn scope(key: &str) -> BTreeSet<String> {
        BTreeSet::from([key.to_owned()])
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn related_waiters_leave_all_general_permits_for_unrelated_control_and_recovery() {
        let owner = Arc::new(ApplyAdmission::default());
        let held = owner
            .claim(scope("consumer/agent"))
            .await
            .expect("obtain the actual bounded recovery admission");
        let sync = SyncBoundary::with_budget(std::time::Duration::from_secs(1));
        let mut waiters = Vec::new();
        for _ in 0..8 {
            let owner = Arc::clone(&owner);
            let sync = sync.clone();
            waiters.push(tokio::spawn(async move {
                let guard = owner
                    .claim(scope("consumer/agent"))
                    .await
                    .expect("obtain the actual bounded recovery admission");
                sync.run(SyncSection::ConfigRecovery, SyncSubject::new(), move || {
                    let _guard = guard;
                    1
                })
                .await
            }));
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(waiters.iter().all(|task| !task.is_finished()));
        assert_eq!(sync.available(), sync.capacity(), "waiters own no general blocking permit");
        let unrelated = owner
            .claim(scope("domain/ui"))
            .await
            .expect("obtain the actual bounded recovery admission");
        assert!(matches!(
            sync.run(SyncSection::ConfigRecovery, SyncSubject::new(), move || {
                let _guard = unrelated;
                2
            })
            .await,
            BlockingAdmission::Completed(2)
        ));
        assert!(matches!(
            sync.run(SyncSection::JournalRead, SyncSubject::new(), || 3).await,
            BlockingAdmission::Completed(3)
        ));
        drop(held);
        for task in waiters {
            assert!(matches!(
                task.await.expect("obtain the actual bounded recovery admission"),
                BlockingAdmission::Completed(1)
            ));
        }
    }
}
