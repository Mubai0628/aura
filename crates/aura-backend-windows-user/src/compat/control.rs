use super::service::CompatExecutionBackend;
use crate::controller::{PortSection, WindowsUserCorePortSection};
use aura_domain_common::{SecurityOperationId, SessionId};
use aura_execution_contract::*;
use std::time::Duration;

impl CompatExecutionBackend {
    pub fn managed_operations(
        &self,
        session: SessionId,
    ) -> anyhow::Result<Vec<SecurityOperationId>> {
        Ok(self
            .active
            .lock()
            .map_err(|_| anyhow::anyhow!("compat.active_lock"))?
            .values()
            .filter(|v| v.session == session)
            .map(|v| v.handle.attempt.operation_id)
            .collect())
    }
    pub fn shutdown(&self) -> anyhow::Result<()> {
        for entry in self.active.lock().map_err(|_| anyhow::anyhow!("compat.active_lock"))?.values()
        {
            let _ = entry.cancel.send(true);
        }
        self.terminals.shutdown()
    }
    pub async fn stop_operation(&self, operation: SecurityOperationId) -> anyhow::Result<()> {
        let handle = self
            .active
            .lock()
            .map_err(|_| anyhow::anyhow!("compat.active_lock"))?
            .get(&operation.to_string())
            .map(|v| v.handle.clone());
        let termination = if let Some(handle) = handle {
            self.cancel_handle(handle)
                .await
                .map_err(|error| anyhow::anyhow!("{error:?}"))?
                .termination
        } else {
            match self
                .reconcile_attempt(AttemptRef {
                    operation_id: operation,
                    ordinal: std::num::NonZeroU32::MIN,
                })
                .await
            {
                Ok(ReconcileResult::Completed { result }) => result.termination,
                _ => TerminationEvidence::Unknown,
            }
        };
        anyhow::ensure!(
            matches!(termination, TerminationEvidence::Confirmed { .. }),
            "compat.stop_unconfirmed"
        );
        Ok(())
    }
    pub(super) async fn cancel_handle(
        &self,
        handle: ExecutionHandle,
    ) -> Result<CancelReceipt, BackendError> {
        let pending = {
            let active = self
                .active
                .lock()
                .map_err(|_| BackendError::DispatchUnknown { attempt: handle.attempt.clone() })?;
            if let Some(entry) = active.get(&handle.attempt.operation_id.to_string()) {
                if entry.handle != handle {
                    return Err(BackendError::DispatchRejected {
                        reason: "compat.cancel_binding".into(),
                    });
                }
                let _ = entry.cancel.send(true);
                Some(entry.done.clone())
            } else {
                None
            }
        };
        let termination = if let Some(mut done) = pending {
            tokio::time::timeout(Duration::from_secs(4), async {
                loop {
                    if let Some(value) = done.borrow().clone() {
                        return value;
                    }
                    if done.changed().await.is_err() {
                        return TerminationEvidence::Unknown;
                    }
                }
            })
            .await
            .unwrap_or(TerminationEvidence::Unknown)
        } else {
            match self.reconcile_attempt(handle.attempt.clone()).await? {
                ReconcileResult::Completed { result } => result.termination,
                _ => TerminationEvidence::Unknown,
            }
        };
        Ok(CancelReceipt { handle, termination })
    }
    pub(super) async fn reconcile_attempt(
        &self,
        attempt: AttemptRef,
    ) -> Result<ReconcileResult, BackendError> {
        let reader = self.port.clone();
        let key = attempt.clone();
        let durable = self
            .port
            .section(
                PortSection::read("compat.reconcile", None),
                Box::new(move || reader.reconcile(key)),
            )
            .await;
        if let Ok(result @ ReconcileResult::Completed { .. }) = durable {
            return Ok(result);
        }
        if let Ok(active) = self.active.lock()
            && let Some(entry) = active.get(&attempt.operation_id.to_string())
        {
            if entry.handle.attempt != attempt {
                return Err(BackendError::DispatchRejected {
                    reason: "compat.attempt_mismatch".into(),
                });
            }
            return Ok(ReconcileResult::Running { handle: entry.handle.clone() });
        }
        durable.map_err(|_| BackendError::DispatchUnknown { attempt })
    }
}
