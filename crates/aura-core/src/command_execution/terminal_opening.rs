//! User-only opening ownership. Cancellation and native launch have one atomic gate.
use crate::{AsyncPriorityQueue, JsonRpcRequest, QueuePriority, RequestLedger};
use anyhow::{Context, Result, ensure};
use aura_backend_windows_user::compat::{TerminalKind, TerminalOptions};
use aura_domain_common::{CorrelationId, RuntimeInstanceId, ToolWireVersionV1};
use aura_ipc::{JsonRpcNotification, OutboundFrame, WireFrame};
use aura_runtime_contract::CancelDisposition;
use aura_windows_user_contract::{TerminalOpeningPhaseV1 as Phase, TerminalOpeningProgressV1};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::watch;

#[derive(Debug, thiserror::Error)]
#[error("终端打开已取消；未创建终端。")]
pub(crate) struct OpeningCancelled;

struct Opening {
    progress: TerminalOpeningProgressV1,
    phase: Mutex<Phase>,
    cancelled: watch::Sender<bool>,
    queue: AsyncPriorityQueue,
    connection: Weak<Mutex<RequestLedger>>,
}
impl Opening {
    fn publish(&self, phase: Phase) -> Result<()> {
        let progress = TerminalOpeningProgressV1 { phase, ..self.progress.clone() };
        let notification = JsonRpcNotification {
            jsonrpc: crate::JSON_RPC_VERSION.into(),
            method: "terminal.opening.v1".into(),
            params: serde_json::to_value(progress)?,
        };
        self.queue.try_send(OutboundFrame {
            priority: QueuePriority::P1,
            frame: WireFrame::Control(serde_json::to_vec(&notification)?),
        })?;
        Ok(())
    }
    fn cancel(&self) -> Result<CancelDisposition> {
        let mut phase = self.phase.lock().map_err(|_| anyhow::anyhow!("terminal.opening_lock"))?;
        match *phase {
            Phase::Launching => Ok(CancelDisposition::TooLateToCancel),
            Phase::WaitingRuntime | Phase::WaitingLaunch | Phase::Cancelled => {
                if *phase != Phase::Cancelled {
                    *phase = Phase::Cancelled;
                    self.cancelled.send_replace(true);
                    if self.publish(Phase::Cancelled).is_err() {
                        crate::append_core_log(
                            "terminal_opening_progress_unavailable",
                            serde_json::json!({"phase":"cancelled"}),
                        );
                    }
                }
                Ok(CancelDisposition::Accepted)
            }
        }
    }
    fn runtime_ready(&self) -> Result<()> {
        let mut phase = self.phase.lock().map_err(|_| anyhow::anyhow!("terminal.opening_lock"))?;
        match *phase {
            Phase::WaitingRuntime => {
                self.publish(Phase::WaitingLaunch)?;
                *phase = Phase::WaitingLaunch;
                Ok(())
            }
            Phase::WaitingLaunch => Ok(()),
            Phase::Cancelled => Err(OpeningCancelled.into()),
            Phase::Launching => anyhow::bail!("terminal.opening_invalid_transition"),
        }
    }
    fn before_launch(&self) -> Result<()> {
        let mut phase = self.phase.lock().map_err(|_| anyhow::anyhow!("terminal.opening_lock"))?;
        ensure!(*phase == Phase::WaitingLaunch, "terminal.open_cancelled");
        self.publish(Phase::Launching)?;
        *phase = Phase::Launching;
        Ok(())
    }
    fn is_cancelled(&self) -> bool {
        *self.cancelled.borrow()
    }
}

#[derive(Default)]
pub(crate) struct OpeningRegistry(Mutex<BTreeMap<CorrelationId, Arc<Opening>>>);
struct OpeningGuard(Arc<Opening>, bool);
impl Drop for OpeningGuard {
    fn drop(&mut self) {
        // An abandoned wait cannot create a late terminal. Launching is already
        // too late; its native worker and TerminalRegistry keep owning the child.
        if self.1 {
            let _ = self.0.cancel();
        }
    }
}
impl OpeningRegistry {
    fn begin(
        &self,
        mut progress: TerminalOpeningProgressV1,
        queue: AsyncPriorityQueue,
        cancelled: bool,
        connection: Weak<Mutex<RequestLedger>>,
    ) -> Result<OpeningGuard> {
        ensure!(
            matches!(progress.phase, Phase::WaitingRuntime | Phase::WaitingLaunch),
            "terminal.opening_invalid_initial_phase"
        );
        let phase = if cancelled { Phase::Cancelled } else { progress.phase };
        progress.phase = phase;
        let request = progress.request_id;
        let (sender, _) = watch::channel(cancelled);
        let opening = Arc::new(Opening {
            progress,
            phase: Mutex::new(phase),
            cancelled: sender,
            queue,
            connection,
        });
        let mut entries = self.0.lock().map_err(|_| anyhow::anyhow!("terminal.opening_lock"))?;
        ensure!(
            entries.len() < 128 && !entries.contains_key(&request),
            "terminal.opening_capacity_or_duplicate"
        );
        entries.insert(request, opening.clone());
        drop(entries);
        let guard = OpeningGuard(opening, true);
        guard.0.publish(phase)?;
        Ok(guard)
    }
    pub(crate) fn cancel(
        &self,
        request: CorrelationId,
        completed: bool,
        connection: &Arc<Mutex<RequestLedger>>,
    ) -> Result<Option<CancelDisposition>> {
        let connection = Arc::downgrade(connection);
        let opening = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("terminal.opening_lock"))?
            .get(&request)
            .filter(|opening| opening.connection.ptr_eq(&connection))
            .cloned();
        opening
            .map(
                |opening| {
                    if completed {
                        Ok(CancelDisposition::TooLateToCancel)
                    } else {
                        opening.cancel()
                    }
                },
            )
            .transpose()
    }
    /// Called after the request ledger reaches its terminal, keeping TooLate
    /// truthful between native start and the original request's final reply.
    pub(crate) fn finish(&self, request: CorrelationId) -> Result<()> {
        self.0.lock().map_err(|_| anyhow::anyhow!("terminal.opening_lock"))?.remove(&request);
        Ok(())
    }
    /// Connection loss reclaims only that connection's metadata and unstarted
    /// gates. Already-launching native work retains its TerminalRegistry owner.
    pub(crate) fn reclaim(&self, ledger: &Arc<Mutex<RequestLedger>>) -> Result<usize> {
        let connection = Arc::downgrade(ledger);
        let mut entries = self.0.lock().map_err(|_| anyhow::anyhow!("terminal.opening_lock"))?;
        let mut removed = Vec::new();
        entries.retain(|_, opening| {
            if opening.connection.ptr_eq(&connection) {
                removed.push(opening.clone());
                false
            } else {
                true
            }
        });
        drop(entries);
        for opening in &removed {
            opening.cancel()?;
        }
        Ok(removed.len())
    }
}

impl super::CommandExecution {
    pub(crate) async fn open_user_terminal(
        &self,
        runtime: RuntimeInstanceId,
        queue: AsyncPriorityQueue,
        request: &JsonRpcRequest,
        openings: &OpeningRegistry,
        ledger: &Arc<Mutex<RequestLedger>>,
    ) -> Result<serde_json::Value> {
        let input: aura_windows_user_contract::UserTerminalOpenRequestV1 =
            serde_json::from_value(request.params.clone())?;
        let projection = self.persistence.workspace_session_projection()?;
        let session = projection
            .sessions
            .iter()
            .find(|session| session.session_id == input.session_id)
            .context("terminal.session_unknown")?;
        let workspace = projection
            .workspaces
            .iter()
            .find(|workspace| workspace.workspace_id == session.workspace_id)
            .context("terminal.workspace_unknown")?;
        let mut opening = {
            // The same order as aura.cancel: queued intent cannot slip between
            // observing the ledger and installing this request's dedicated gate.
            let connection = Arc::downgrade(ledger);
            let ledger =
                ledger.lock().map_err(|_| anyhow::anyhow!("request ledger was poisoned"))?;
            openings.begin(
                TerminalOpeningProgressV1 {
                    contract_version: ToolWireVersionV1,
                    runtime_instance_id: runtime,
                    request_id: request.id,
                    session_id: input.session_id,
                    phase: if self.compat.is_runtime_check_pending() {
                        Phase::WaitingRuntime
                    } else {
                        Phase::WaitingLaunch
                    },
                },
                queue,
                ledger.cancellation_requested(request.id),
                connection,
            )?
        };
        if opening.0.is_cancelled() {
            return Err(OpeningCancelled.into());
        }
        let ready = opening.0.clone();
        let gate = opening.0.clone();
        let result = self
            .compat
            .open_terminal_cancellable(
                input.session_id,
                TerminalOptions {
                    shell: input.shell,
                    kind: TerminalKind::User,
                    working_directory: workspace.canonical_root.clone().into(),
                    size: aura_windows_platform::TerminalSize {
                        columns: input.columns,
                        rows: input.rows,
                    },
                    output_limit: 16 * 1024 * 1024,
                    secret_environment: Vec::new(),
                },
                opening.0.cancelled.subscribe(),
                Arc::new(move || ready.runtime_ready()),
                Arc::new(move || gate.before_launch()),
            )
            .await;
        // An ordinary verification/launch failure is a failure, not a user
        // cancellation. Drop cancels only an abandoned in-flight wait.
        opening.1 = false;
        if opening.0.is_cancelled() {
            return Err(OpeningCancelled.into());
        }
        let entry = result?;
        let terminal = self.compat.terminals.get(input.session_id, &entry.id)?;
        Ok(serde_json::to_value(super::terminal::view(&entry, &terminal)?)?)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[path = "terminal_opening/tests.rs"]
mod tests;
