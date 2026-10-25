//! The backend's lifetime owns observation; an IPC/orchestrator waiter never ends it.
use crate::agent_execution::{AgentExecution, CallContext};
use aura_backend_api::{BackendFuture, ExecutionBackend};
use aura_execution_contract::*;
use std::sync::Arc;

tokio::task_local! {
    static FILE_OBSERVER: aura_fs_tools::FileExecutionObserver;
}

pub(crate) fn current_file_observer() -> Option<aura_fs_tools::FileExecutionObserver> {
    FILE_OBSERVER.try_with(Clone::clone).ok()
}

#[cfg(test)]
pub(crate) async fn scope_for_test<F: std::future::Future>(
    observer: aura_fs_tools::FileExecutionObserver,
    future: F,
) -> F::Output {
    FILE_OBSERVER.scope(observer, future).await
}

pub(crate) fn observe_current_execution() -> Option<Box<dyn Send>> {
    current_file_observer().and_then(|observer| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observer(&[]))).ok()
    })
}

pub(crate) struct ObservedBackend {
    pub delegate: Arc<dyn ExecutionBackend>,
    pub service: Arc<AgentExecution>,
    pub call: CallContext,
}

pub(crate) struct EndOwner(pub Arc<super::AgentActivityLease>);
impl Drop for EndOwner {
    fn drop(&mut self) {
        self.0.end();
    }
}

fn execution_lifetime_guard(lease: Arc<super::AgentActivityLease>, shell: bool) -> Box<dyn Send> {
    if shell {
        // The verified foreground end can precede the managed worker's background cleanup.
        Box::new(EndOwner(lease))
    } else {
        // A nested file phase must not end the outer blocking worker's still-live observation.
        Box::new(lease)
    }
}

impl ExecutionBackend for ObservedBackend {
    fn descriptor(&self) -> BackendDescriptor {
        self.delegate.descriptor()
    }
    fn prepare(&self, request: PrepareRequest) -> BackendFuture<'_, ExecutionPlan> {
        self.delegate.prepare(request)
    }
    fn cancel(&self, handle: ExecutionHandle) -> BackendFuture<'_, CancelReceipt> {
        self.delegate.cancel(handle)
    }
    fn reconcile(&self, attempt: AttemptRef) -> BackendFuture<'_, ReconcileResult> {
        self.delegate.reconcile(attempt)
    }
    fn execute(&self, dispatch: AuthorizedDispatch) -> BackendFuture<'_, ExecutionHandle> {
        Box::pin(async move {
            let attempt = dispatch.request().binding.attempt.clone();
            let lease =
                Arc::new(super::AgentActivityLease::new(self.service.activity_index.clone()));
            let observer: aura_fs_tools::FileExecutionObserver = {
                let service = Arc::clone(&self.service);
                let call = self.call.clone();
                let shell = super::tool_group_for(&call.operation.content.action_kind)
                    == aura_activity_domain::ActivityToolGroup::Shell;
                let lease = Arc::clone(&lease);
                Arc::new(move |requests| {
                    if !requests.is_empty() {
                        let projected = super::resources::project(requests);
                        if let Ok(mut facts) = call.observed_resources.try_lock() {
                            *facts = projected;
                        }
                    }
                    service.record_task_resources(&call);
                    let published = service.publish_agent_tool_activity(
                        &call,
                        service.runtime_generation(),
                        attempt.operation_id,
                        attempt.ordinal.get(),
                    );
                    lease.started(published);
                    // The actual executor holds the end responsibility, not its async caller.
                    execution_lifetime_guard(Arc::clone(&lease), shell)
                })
            };
            FILE_OBSERVER.scope(observer, self.delegate.execute(dispatch)).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_nested_file_phase_cannot_end_the_real_worker_after_caller_drop() {
        let index = aura_activity_runtime::ActivityIndex::default();
        let caller = Arc::new(crate::agent_activity::AgentActivityLease::new(index.clone()));
        let worker = Arc::clone(&caller);
        let record = crate::agent_activity::tests_support::synthetic_record();
        let published = crate::agent_activity::PublishedActivity {
            id: record.id,
            generation: record.generation,
        };
        assert!(index.publish(record).was_recorded());
        worker.started(published);
        let nested_phase = execution_lifetime_guard(Arc::clone(&worker), false);
        drop(caller);
        drop(nested_phase);
        assert_eq!(index.len(), 1, "the actual blocking worker still owns this running tool");
        drop(worker);
        assert_eq!(index.len(), 0);
    }
}
