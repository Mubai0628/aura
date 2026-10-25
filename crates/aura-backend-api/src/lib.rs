#![forbid(unsafe_code)]

use aura_execution_contract::{
    AttemptRef, AuthorizedDispatch, BackendDescriptor, BackendError, CancelReceipt,
    ExecutionHandle, ExecutionPlan, PrepareRequest, ReconcileResult,
};
use std::{future::Future, pin::Pin};

pub type BackendFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, BackendError>> + Send + 'a>>;

/// Backend implementations own execution resources, never authority, database
/// connections or final domain state. There is deliberately no fallback or
/// retry default. Preparation must not dispatch user actions; an uncertain
/// execute result must be reconciled against the same Attempt.
pub trait ExecutionBackend: Send + Sync {
    fn descriptor(&self) -> BackendDescriptor;
    fn prepare(&self, request: PrepareRequest) -> BackendFuture<'_, ExecutionPlan>;
    fn execute(&self, dispatch: AuthorizedDispatch) -> BackendFuture<'_, ExecutionHandle>;
    fn cancel(&self, handle: ExecutionHandle) -> BackendFuture<'_, CancelReceipt>;
    fn reconcile(&self, attempt: AttemptRef) -> BackendFuture<'_, ReconcileResult>;
}

// A registry must be able to hold heterogeneous backend implementations.
const _: fn(&dyn ExecutionBackend) = |_| {};
