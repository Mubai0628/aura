//! Test-only observer of an actual Pending poll at the global runtime-capacity boundary.
use aura_domain_common::ToolCallId;
use aura_tool_scheduler::{
    ToolDispatchPermitV1, ToolDispatchReadinessPermitV1, ToolScheduleErrorV1,
};
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{Mutex, OnceLock},
    task::Poll,
};
use tokio::sync::{oneshot, watch};
type Waiters = BTreeMap<ToolCallId, oneshot::Sender<()>>;
fn waiters() -> &'static Mutex<Waiters> {
    static WAITERS: OnceLock<Mutex<Waiters>> = OnceLock::new();
    WAITERS.get_or_init(Mutex::default)
}
pub(super) struct Registration(ToolCallId);
pub(super) fn observe(call: ToolCallId) -> (Registration, oneshot::Receiver<()>) {
    let (tx, rx) = oneshot::channel();
    assert!(
        waiters().lock().expect("lock the test-only capacity observer").insert(call, tx).is_none()
    );
    (Registration(call), rx)
}
impl Drop for Registration {
    fn drop(&mut self) {
        waiters().lock().expect("lock the test-only capacity observer").remove(&self.0);
    }
}
pub(super) async fn activate(
    call: ToolCallId,
    readiness: ToolDispatchReadinessPermitV1,
    cancellation: &mut watch::Receiver<bool>,
) -> Result<ToolDispatchPermitV1, ToolScheduleErrorV1> {
    let future = readiness.activate_runtime_slot(cancellation);
    tokio::pin!(future);
    std::future::poll_fn(|cx| {
        let polled = future.as_mut().poll(cx);
        if polled.is_pending()
            && let Some(tx) =
                waiters().lock().expect("lock the test-only capacity observer").remove(&call)
        {
            let _ = tx.send(());
        }
        match polled {
            Poll::Pending => Poll::Pending,
            other => other,
        }
    })
    .await
}
