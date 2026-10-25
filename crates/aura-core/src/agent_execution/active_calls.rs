//! Per-call cancellation. A permission change must never cancel the owning model run.
use super::*;
use std::collections::BTreeMap;
use tokio::sync::watch;

#[derive(Clone)]
pub(super) struct ActiveCall {
    pub call: CallContext,
    pub cancel: watch::Sender<bool>,
    pub ended: watch::Receiver<bool>,
    lifetime: std::sync::Weak<CallLifetime>,
}

#[derive(Default)]
pub(super) struct ActiveCalls(std::sync::Mutex<BTreeMap<ToolCallId, ActiveCall>>);

pub(super) struct CallLifetime {
    owner: Arc<ActiveCalls>,
    id: ToolCallId,
    ended: watch::Sender<bool>,
    relay: tokio::task::JoinHandle<()>,
}

impl Drop for CallLifetime {
    fn drop(&mut self) {
        self.relay.abort();
        self.ended.send_replace(true);
        if let Ok(mut calls) = self.owner.0.lock() {
            calls.remove(&self.id);
        }
    }
}

impl ActiveCalls {
    pub fn register(
        self: &Arc<Self>,
        call: &CallContext,
        mut parent: watch::Receiver<bool>,
    ) -> anyhow::Result<(Arc<CallLifetime>, watch::Receiver<bool>)> {
        let (cancel, token) = watch::channel(*parent.borrow());
        let (ended, observed) = watch::channel(false);
        let id = call.validated.call.tool_call_id;
        let mut calls =
            self.0.lock().map_err(|_| anyhow::anyhow!("agent.active_calls_unavailable"))?;
        anyhow::ensure!(!calls.contains_key(&id), "agent.call_already_active");
        let relay_cancel = cancel.clone();
        let relay = tokio::spawn(async move {
            while parent.changed().await.is_ok() {
                if *parent.borrow_and_update() {
                    relay_cancel.send_replace(true);
                    break;
                }
            }
        });
        let lifetime = Arc::new(CallLifetime { owner: self.clone(), id, ended, relay });
        calls.insert(
            id,
            ActiveCall {
                call: call.clone(),
                cancel,
                ended: observed,
                lifetime: Arc::downgrade(&lifetime),
            },
        );
        Ok((lifetime, token))
    }

    /// The blocking worker owns the real end, even if its awaiting future is dropped.
    pub fn retain_worker(&self, id: ToolCallId) -> anyhow::Result<Arc<CallLifetime>> {
        self.0
            .lock()
            .map_err(|_| anyhow::anyhow!("agent.active_calls_unavailable"))?
            .get(&id)
            .and_then(|call| call.lifetime.upgrade())
            .ok_or_else(|| anyhow::anyhow!("agent.call_not_active"))
    }

    pub fn session(&self, session: SessionId) -> anyhow::Result<Vec<ActiveCall>> {
        Ok(self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("agent.active_calls_unavailable"))?
            .values()
            .filter(|entry| entry.call.operation.content.identity.session_id == Some(session))
            .cloned()
            .collect())
    }
}
