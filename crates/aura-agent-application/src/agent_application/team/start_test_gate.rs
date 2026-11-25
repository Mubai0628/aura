use super::*;
use std::sync::{Mutex, OnceLock, Weak};
use tokio::sync::Notify;

type Pair = (Weak<Notify>, Weak<Notify>);
fn gates() -> &'static Mutex<BTreeMap<SessionId, Pair>> {
    static GATES: OnceLock<Mutex<BTreeMap<SessionId, Pair>>> = OnceLock::new();
    GATES.get_or_init(Mutex::default)
}

impl AgentApplication {
    pub fn pause_next_temporary_start_for_test(
        &self,
        session: SessionId,
    ) -> (Arc<Notify>, Arc<Notify>) {
        let (entered, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
        let mut gates = gates().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        gates.retain(|_, (entered, release)| {
            entered.strong_count() > 0 && release.strong_count() > 0
        });
        gates.insert(session, (Arc::downgrade(&entered), Arc::downgrade(&release)));
        (entered, release)
    }
}

pub(super) async fn wait(session: SessionId) {
    let pair = gates().lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&session);
    if let Some((entered, release)) = pair
        && let (Some(entered), Some(release)) = (entered.upgrade(), release.upgrade())
    {
        entered.notify_one();
        release.notified().await;
    }
}
