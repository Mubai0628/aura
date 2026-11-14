use super::refusal::{
    PREPARATION_FAILED, REVALIDATION_FAILED, RUNTIME_UNAVAILABLE, SCOPE_CHANGED, UNSUPPORTED,
    reason,
};
use super::service::CompatExecutionBackend;
use crate::{
    controller::{Active, PortSection, WindowsUserCorePortSection},
    managed_operations,
};
use anyhow::{Result, ensure};
use aura_backend_api::{BackendFuture, ExecutionBackend};
use aura_domain_common::{UtcTimestamp, jcs::canonical_sha256};
use aura_execution_contract::*;
use aura_windows_user_contract::WindowsUserPreparation;
use tokio::sync::watch;

impl ExecutionBackend for CompatExecutionBackend {
    fn descriptor(&self) -> BackendDescriptor {
        self.descriptor_now()
    }
    fn prepare(&self, request: PrepareRequest) -> BackendFuture<'_, ExecutionPlan> {
        Box::pin(async move {
            let engine =
                self.check.wait().await.map_err(|error| BackendError::PreparationRejected {
                    reason: reason(RUNTIME_UNAVAILABLE, Some(&error)),
                })?;
            let descriptor = self.descriptor();
            if request.operation.validate().is_err()
                || !descriptor.supports_operation(&request.requirement, &request.operation)
            {
                return Err(BackendError::PreparationRejected {
                    reason: reason(UNSUPPORTED, None),
                });
            }
            let reader = self.port.clone();
            let operation = request.operation.clone();
            let facts = self
                .port
                .section(
                    PortSection::read("compat.scope", operation.content.identity.agent_run_id),
                    Box::new(move || reader.scope(&operation)),
                )
                .await
                .map_err(|error| BackendError::PreparationRejected {
                    reason: reason(SCOPE_CHANGED, Some(&error)),
                })?;
            let valid_until =
                UtcTimestamp::from_value(UtcTimestamp::now().value() + time::Duration::minutes(10));
            let preparation = self
                .preparation(&engine, &request.operation, facts, valid_until)
                .and_then(|preparation| {
                    Ok(ExecutionPlan::new(PreparedExecution {
                        operation_fingerprint: request.operation.fingerprint.clone(),
                        backend: descriptor,
                        preparation: serde_json::to_value(preparation)?,
                    })?)
                });
            preparation.map_err(|error| BackendError::PreparationRejected {
                reason: reason(PREPARATION_FAILED, Some(&error)),
            })
        })
    }
    fn execute(&self, dispatch: AuthorizedDispatch) -> BackendFuture<'_, ExecutionHandle> {
        Box::pin(async move {
            let request = dispatch.request().clone();
            let engine = self.check.wait().await.map_err(|error| rejected(Some(&error)))?;
            let facts = managed_operations::prepared_scope(
                self.port.clone(),
                &request,
                "compat.scope_before_start",
            )
            .await
            .map_err(|error| rejected(Some(&error)))?;
            let ready = || -> Result<WindowsUserPreparation> {
                request.plan.validate()?;
                let prepared: WindowsUserPreparation =
                    serde_json::from_value(request.plan.prepared.preparation.clone())?;
                ensure!(
                    prepared.valid_until > UtcTimestamp::now()
                        && request.plan.prepared.backend == self.descriptor(),
                    "compat.preparation_changed"
                );
                ensure!(
                    self.preparation(&engine, &request.operation, facts, prepared.valid_until)?
                        == prepared,
                    "compat.scope_changed"
                );
                Ok(prepared)
            };
            let prepared = ready().map_err(|error| rejected(Some(&error)))?;
            let handle = ExecutionHandle {
                attempt: request.binding.attempt.clone(),
                dispatch_fingerprint: canonical_sha256(&request.binding)
                    .map_err(|_| rejected(None))?,
            };
            let session =
                request.operation.content.identity.session_id.ok_or_else(|| rejected(None))?;
            let (cancel, cancelled) = watch::channel(false);
            let (complete, done) = watch::channel(None);
            let active = self.active.clone();
            let registered_handle = handle.clone();
            let registered_request = request.clone();
            let port = self.port.clone();
            let expires = self
                .port
                .section(
                    PortSection::write(
                        "compat.claim",
                        request.operation.content.identity.agent_run_id,
                    ),
                    Box::new(move || {
                        let mut register = || -> Result<()> {
                            let mut active =
                                active.lock().map_err(|_| anyhow::anyhow!("compat.active_lock"))?;
                            ensure!(
                                active.len() < 32
                                    && !active.contains_key(
                                        &registered_handle.attempt.operation_id.to_string()
                                    ),
                                "compat.replay_or_capacity"
                            );
                            active.insert(
                                registered_handle.attempt.operation_id.to_string(),
                                Active {
                                    handle: registered_handle.clone(),
                                    session,
                                    cancel: cancel.clone(),
                                    done: done.clone(),
                                },
                            );
                            Ok(())
                        };
                        port.claim(&registered_request, &mut register)
                    }),
                )
                .await
                .map_err(|error| rejected(Some(&error)))?;
            let port = self.port.clone();
            let active = self.active.clone();
            let proxy = self.proxy.clone();
            let key = handle.attempt.operation_id.to_string();
            tokio::spawn(async move {
                let evidence =
                    super::monitor::run(request, prepared, engine, proxy, expires, cancelled, port)
                        .await
                        .unwrap_or(TerminationEvidence::Unknown);
                let confirmed = matches!(evidence, TerminationEvidence::Confirmed { .. });
                if confirmed && let Ok(mut active) = active.lock() {
                    active.remove(&key);
                }
                let _ = complete.send(Some(evidence));
            });
            Ok(handle)
        })
    }
    fn cancel(&self, handle: ExecutionHandle) -> BackendFuture<'_, CancelReceipt> {
        Box::pin(self.cancel_handle(handle))
    }
    fn reconcile(&self, attempt: AttemptRef) -> BackendFuture<'_, ReconcileResult> {
        Box::pin(self.reconcile_attempt(attempt))
    }
}
/// A dispatch refused at its last check before launch, naming the cause found in `error`.
fn rejected(error: Option<&anyhow::Error>) -> BackendError {
    BackendError::DispatchRejected { reason: reason(REVALIDATION_FAILED, error) }
}
