#![forbid(unsafe_code)]
//! In-process execution. Core supplies domain ports and owns the durable
//! dispatch claim, receipts and terminal state. This crate cannot open a DB,
//! spawn a process, select a fallback, or acquire authority.
use aura_backend_api::{BackendFuture, ExecutionBackend};
use aura_execution_contract::*;
use aura_security_contract::BackendClass;

/// Why a dispatch is refused when the domain facts it was prepared on no longer allow it. Nothing
/// was claimed or run, so the command can be prepared again.
pub const FACTS_CHANGED: &str = "prepared domain facts changed";

// A refusal reason that reads `<code>: <sentence>` reaches the model as it is, so each of the
// reasons below is one such line.

/// Why a broker built with another backend's descriptor is refused.
const WRONG_CLASS: &str = "broker.wrong_class: Aura's in-process runner was set up with the \
                           description of another kind of runner (an internal error in Aura). \
                           Nothing was run.";
/// Why a call this backend does not run is refused before it is prepared.
const UNSUPPORTED: &str = "broker.unsupported_operation: This call was not started: Aura's \
                           in-process runner does not run this kind of call (an internal error \
                           in Aura). Nothing was changed. Tell the user if it happens again.";
/// Why a call whose preparation cannot be recorded is refused.
const INVALID_PREPARATION: &str = "broker.invalid_preparation: This call was not started: Aura \
                                   could not record how it was prepared (an internal error in \
                                   Aura). Nothing was changed. Try it again; if it fails again, \
                                   tell the user.";

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
            return Err(BackendError::PreparationRejected { reason: WRONG_CLASS.into() });
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
                return Err(BackendError::PreparationRejected { reason: UNSUPPORTED.into() });
            }
            let preparation = self.port.inspect(&request.operation).await?;
            ExecutionPlan::new(PreparedExecution {
                operation_fingerprint: request.operation.fingerprint,
                backend: self.descriptor(),
                preparation,
            })
            .map_err(|_| BackendError::PreparationRejected { reason: INVALID_PREPARATION.into() })
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
                return Err(BackendError::DispatchRejected { reason: FACTS_CHANGED.into() });
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Each refusal reason is one line of a lowercase dotted code and a sentence that says what
    /// happened and what was changed, with nothing a reader of the line would not understand.
    #[test]
    fn each_refusal_reason_is_a_code_and_a_sentence() {
        for reason in [WRONG_CLASS, UNSUPPORTED, INVALID_PREPARATION] {
            let (code, sentence) = reason.split_once(": ").expect("a code, then a sentence");
            assert!(code.starts_with("broker."), "{code}");
            assert!(code.chars().all(|c| c.is_ascii_lowercase() || c == '.' || c == '_'), "{code}");
            assert!(sentence.contains("internal error") && sentence.ends_with('.'), "{sentence}");
            assert!(sentence.contains("Nothing was"), "{sentence}");
            assert!(reason.len() <= 512 && !reason.contains(['{', '}', '\n']), "{reason}");
        }
    }
}
