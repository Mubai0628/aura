#![forbid(unsafe_code)]
//! In-process execution. Core supplies domain ports and owns the durable
//! dispatch claim, receipts and terminal state. This crate cannot open a DB,
//! spawn a process, select a fallback, or acquire authority.
use aura_backend_api::{BackendFuture, ExecutionBackend};
use aura_execution_contract::*;
use aura_security_contract::BackendClass;

pub trait BrokerPort: Send + Sync {
    /// Re-resolve domain facts, including final path/file identities. Called
    /// both during preparation and immediately before the durable claim.
    fn inspect(&self, operation: &SecuredOperation) -> BackendFuture<'_, serde_json::Value>;
    /// Whether the domain facts, resolved again now, still allow the prepared execution: the same
    /// facts, unless the domain knows which differences leave the operation as it was approved.
    fn still_prepared<'a>(
        &'a self,
        operation: &'a SecuredOperation,
        prepared: &'a serde_json::Value,
    ) -> BackendFuture<'a, bool> {
        Box::pin(async move { Ok(self.inspect(operation).await? == *prepared) })
    }
    /// Atomically compare the exact reserved binding and claim it once. A
    /// previously dispatched or unknown operation must return an error.
    fn claim(&self, dispatch: &AuthorizedDispatch) -> BackendFuture<'_, ()>;
    /// Invoke the domain service and commit receipt/result evidence in Core.
    /// A storage failure after invocation must remain dispatch-unknown.
    fn invoke(&self, request: &DispatchRequest) -> BackendFuture<'_, ()>;
    fn cancel(&self, handle: ExecutionHandle) -> BackendFuture<'_, CancelReceipt>;
    fn reconcile(&self, attempt: AttemptRef) -> BackendFuture<'_, ReconcileResult>;
}

pub struct CoreBroker<P> {
    descriptor: BackendDescriptor,
    port: P,
}

impl<P: BrokerPort> CoreBroker<P> {
    pub fn new(descriptor: BackendDescriptor, port: P) -> Result<Self, BackendError> {
        if descriptor.class != BackendClass::CoreBroker {
            return Err(BackendError::PreparationRejected { reason: "broker class".into() });
        }
        Ok(Self { descriptor, port })
    }
}

impl<P: BrokerPort> ExecutionBackend for CoreBroker<P> {
    fn descriptor(&self) -> BackendDescriptor {
        self.descriptor.clone()
    }

    fn prepare(&self, request: PrepareRequest) -> BackendFuture<'_, ExecutionPlan> {
        Box::pin(async move {
            if request.operation.validate().is_err()
                || !self.descriptor.supports_operation(&request.requirement, &request.operation)
            {
                return Err(BackendError::PreparationRejected {
                    reason: "unsupported operation".into(),
                });
            }
            let preparation = self.port.inspect(&request.operation).await?;
            ExecutionPlan::new(PreparedExecution {
                operation_fingerprint: request.operation.fingerprint,
                backend: self.descriptor(),
                preparation,
            })
            .map_err(|_| BackendError::PreparationRejected { reason: "invalid preparation".into() })
        })
    }

    fn execute(&self, dispatch: AuthorizedDispatch) -> BackendFuture<'_, ExecutionHandle> {
        Box::pin(async move {
            let request = dispatch.request();
            if request.plan.validate().is_err()
                || request.plan.prepared.backend != self.descriptor
                || !self
                    .port
                    .still_prepared(&request.operation, &request.plan.prepared.preparation)
                    .await?
            {
                return Err(BackendError::DispatchRejected {
                    reason: "prepared domain facts changed".into(),
                });
            }
            self.port.claim(&dispatch).await?;
            // Never re-enter a domain after claim, even if it panics, disappears
            // or returns without being able to persist its result.
            self.port.invoke(request).await.map_err(|_| BackendError::DispatchUnknown {
                attempt: request.binding.attempt.clone(),
            })?;
            Ok(ExecutionHandle {
                attempt: request.binding.attempt.clone(),
                dispatch_fingerprint: aura_domain_common::jcs::canonical_sha256(&request.binding)
                    .map_err(|_| BackendError::DispatchUnknown {
                    attempt: request.binding.attempt.clone(),
                })?,
            })
        })
    }

    fn cancel(&self, handle: ExecutionHandle) -> BackendFuture<'_, CancelReceipt> {
        self.port.cancel(handle)
    }
    fn reconcile(&self, attempt: AttemptRef) -> BackendFuture<'_, ReconcileResult> {
        self.port.reconcile(attempt)
    }
}
