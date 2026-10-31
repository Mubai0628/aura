//! A run whose write outlived its caller's bounded wait. Such a write was admitted and keeps
//! running under its own permit, so it may still commit: the run follows it to its real end and,
//! once it committed, is driven again from what really committed - each wake-up reloads the last
//! committed boundary. A busy store makes the run wait (background copying gives way meanwhile)
//! instead of ending its turn as interrupted.
use super::*;
use aura_core_base::{append_core_log, busy};
use aura_core_broker::agent_store::{self, ReceiptState, StoreError};

/// How long such a write is followed to its end.
const FOLLOW_WITHIN: std::time::Duration = std::time::Duration::from_secs(300);

impl AgentApplication {
    pub(super) async fn drive_past_late_writes(
        self: &Arc<Self>,
        run: AgentRunId,
        cancel: ModelCancellationToken,
        tool_cancel: watch::Receiver<bool>,
        stage: crate::agent_supervision::RunStageCell,
    ) -> anyhow::Result<()> {
        let deadline = tokio::time::Instant::now() + FOLLOW_WITHIN;
        loop {
            let error =
                match self.drive(run, cancel.clone(), tool_cancel.clone(), stage.clone()).await {
                    Ok(()) => return Ok(()),
                    Err(error) => error,
                };
            let Some(receipt) = late_write(&error) else {
                return Err(error);
            };
            if cancel.is_cancelled() {
                return Err(error);
            }
            let committed = {
                let _waiting = busy::wait_for_store();
                follow(&self.store, receipt, deadline).await
            };
            append_core_log(
                "agent.late_write_followed",
                serde_json::json!({ "agent_run_id": run, "committed": committed }),
            );
            if committed != Some(true) {
                return Err(error);
            }
        }
    }
}

/// The receipt of a write that started and did not answer within its caller's wait.
fn late_write(error: &anyhow::Error) -> Option<u64> {
    match error.downcast_ref::<StoreError>()? {
        StoreError::Unknown { receipt, .. } => *receipt,
        _ => None,
    }
}

/// Whether the write committed, once it really ended; `None` when it has not by `deadline`.
async fn follow(
    store: &agent_store::AgentStore,
    receipt: u64,
    deadline: tokio::time::Instant,
) -> Option<bool> {
    loop {
        match store.receipt(receipt).map(|entry| entry.state) {
            Some(ReceiptState::Settled { committed, .. }) => return Some(committed),
            Some(ReceiptState::InFlight) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            _ => return None,
        }
    }
}
