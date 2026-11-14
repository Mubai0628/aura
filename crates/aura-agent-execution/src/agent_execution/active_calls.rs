//! Per-call cancellation. A permission change must never cancel the owning model run.
use super::*;
use std::collections::BTreeMap;
use tokio::sync::watch;

#[derive(Clone)]
pub struct ActiveCall {
    pub call: CallContext,
    pub cancel: watch::Sender<bool>,
    pub ended: watch::Receiver<bool>,
    lifetime: std::sync::Weak<CallLifetime>,
}

#[derive(Default)]
pub struct ActiveCalls(
    std::sync::Mutex<BTreeMap<ToolCallId, ActiveCall>>,
    /// The calls that wait for another call to finish with what they need, and what their rows
    /// say they wait for.
    std::sync::Mutex<BTreeMap<ToolCallId, &'static str>>,
    /// The calls recorded Unknown whose processes were not yet seen to stop, which hold their
    /// locks until then.
    std::sync::Mutex<BTreeMap<SecurityOperationId, Hold>>,
);

/// A call that holds its locks until its processes are seen to stop: the session's calls of its
/// family that wait behind it read `words`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hold {
    pub session: SessionId,
    pub family: &'static str,
    pub words: &'static str,
}

/// While held, the session's calls of its family that wait read what they wait for.
pub struct HoldMark {
    owner: Arc<ActiveCalls>,
    operation: SecurityOperationId,
}

impl Drop for HoldMark {
    fn drop(&mut self) {
        if let Ok(mut holds) = self.owner.2.lock() {
            holds.remove(&self.operation);
        }
    }
}

/// What the row of a file call says while another call holds its file.
pub const WAITING_FOR_FILE: &str = "在等另一个调用用完这个文件";

/// While held, its call's row says what the call waits for instead of 运行中.
pub struct WaitMark {
    owner: Arc<ActiveCalls>,
    id: ToolCallId,
}

impl Drop for WaitMark {
    fn drop(&mut self) {
        if let Ok(mut waiting) = self.owner.1.lock() {
            waiting.remove(&self.id);
        }
    }
}

pub struct CallLifetime {
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

    /// Marks call `id` as waiting for `what`, the words its row shows, until the mark is dropped.
    pub fn wait(self: &Arc<Self>, id: ToolCallId, what: &'static str) -> WaitMark {
        if let Ok(mut waiting) = self.1.lock() {
            waiting.insert(id, what);
        }
        WaitMark { owner: Arc::clone(self), id }
    }

    /// What each of `ids` that waits now is waiting for.
    pub fn waiting(
        &self,
        ids: impl IntoIterator<Item = ToolCallId>,
    ) -> BTreeMap<ToolCallId, &'static str> {
        let Ok(waiting) = self.1.lock() else {
            return BTreeMap::new();
        };
        ids.into_iter().filter_map(|id| waiting.get(&id).map(|what| (id, *what))).collect()
    }

    /// Marks `operation` as holding its locks until its processes are seen to stop.
    pub fn hold(self: &Arc<Self>, operation: SecurityOperationId, hold: Hold) -> HoldMark {
        if let Ok(mut holds) = self.2.lock() {
            holds.insert(operation, hold);
        }
        HoldMark { owner: Arc::clone(self), operation }
    }

    /// The holds of `session`'s calls.
    pub fn held(&self, session: SessionId) -> Vec<Hold> {
        let Ok(holds) = self.2.lock() else {
            return Vec::new();
        };
        holds.values().filter(|hold| hold.session == session).copied().collect()
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

#[cfg(test)]
mod wait_tests {
    use super::*;

    #[test]
    fn a_call_reads_as_waiting_only_while_its_mark_is_held() {
        let calls = Arc::new(ActiveCalls::default());
        let (one, two) = (ToolCallId::new(), ToolCallId::new());
        let mark = calls.wait(one, WAITING_FOR_FILE);
        assert_eq!(calls.waiting([one, two]), BTreeMap::from([(one, WAITING_FOR_FILE)]));
        drop(mark);
        assert!(calls.waiting([one, two]).is_empty());
    }

    #[test]
    fn a_hold_is_read_by_its_session_only_while_held() {
        let calls = Arc::new(ActiveCalls::default());
        let (session, other) = (SessionId::new(), SessionId::new());
        let hold = Hold { session, family: "git.", words: "waiting" };
        let mark = calls.hold(SecurityOperationId::new(), hold);
        assert_eq!(calls.held(session), vec![hold]);
        assert!(calls.held(other).is_empty());
        drop(mark);
        assert!(calls.held(session).is_empty());
    }
}
