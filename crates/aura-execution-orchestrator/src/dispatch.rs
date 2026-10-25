//! Policy selection precedes scheduling. After reservation the backend is fixed;
//! lost replies never become a retry or an implicit switch to another backend.
use aura_backend_api::{BackendFuture, ExecutionBackend};
use aura_domain_common::{Revision, SecurityOperationId, UtcTimestamp};
use aura_execution_contract::*;
use aura_permission_engine::{policy::PolicyInput, selector};
use aura_security_contract::{
    AuthorizationDecision, AuthorizationProposal, PolicyAuthorization, PolicyDenyReason,
};
use aura_tool_scheduler::{ToolDispatchPermitV1, ToolExecutionSchedulerV1, ToolScheduleRequestV1};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::watch;

/// Implemented by the application over owning Stores. reserve must atomically
/// reload live mode/source/cancellation, consume authority and create the one
/// Attempt. A prepared rejection must prove the durable Attempt was not claimed.
///
/// Every method here performs the application's real synchronous critical section - the
/// session permission authority and then the journal - and this trait used to be synchronous, so
/// the reactive task that polls the dispatch future was the thread that waited on those locks.
/// The methods are awaitable now, and the application runs each section **whole** on its own
/// bounded synchronous owner, so the wait no longer occupies the reactor. The three methods are
/// still one ordered section each: nothing is split into a snapshot that could go stale between
/// reading the authority and committing against it.
pub trait DispatchStore: Send + Sync {
    fn reserve<'a>(
        &'a self,
        request: DispatchRequest,
        authorization: &'a AuthorizationDecision,
    ) -> BackendFuture<'a, AuthorizedDispatch>;
    fn reject_prepared<'a>(
        &'a self,
        request: &'a DispatchRequest,
        reason: &'a str,
    ) -> BackendFuture<'a, ExecutionResult>;
    fn request_cancel<'a>(&'a self, attempt: &'a AttemptRef) -> BackendFuture<'a, ()>;
}
/// The outcome of choosing a backend for one call.
///
/// `Debug` is written by hand rather than derived: `SelectedCall` holds a
/// `dyn ExecutionBackend` that cannot derive it, and the useful fact about a
/// selection is the descriptor it chose, not the object behind it. A caller that has
/// to explain an unexpected answer can then name what it got instead of only that it
/// was wrong.
pub enum Selection {
    Ready(Box<SelectedCall>),
    /// Both payloads are boxed: a proposal now carries the consent facts for a
    /// session approval, and an unboxed variant of that size would make every
    /// `Ready` and `Denied` value pay for it.
    ApprovalRequired {
        proposal: Box<AuthorizationProposal>,
        backend: Box<BackendDescriptor>,
    },
    Denied {
        reason: PolicyDenyReason,
    },
}

impl std::fmt::Debug for Selection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ready(selected) => formatter
                .debug_struct("Ready")
                .field("backend", &selected.descriptor().id.0)
                .field("class", &selected.descriptor().class)
                .finish(),
            Self::ApprovalRequired { backend, proposal } => formatter
                .debug_struct("ApprovalRequired")
                .field("backend", &backend.id.0)
                .field("class", &backend.class)
                .field("reasons", &proposal.reasons)
                .finish(),
            Self::Denied { reason } => {
                formatter.debug_struct("Denied").field("reason", reason).finish()
            }
        }
    }
}
pub struct SelectedCall {
    operation: SecuredOperation,
    descriptor: BackendDescriptor,
    authorization: AuthorizationDecision,
    backend: Arc<dyn ExecutionBackend>,
}
impl SelectedCall {
    /// The backend this call was selected for. Reading it back is what separates
    /// "selection chose a backend" from "the call really ran there".
    #[must_use]
    pub const fn descriptor(&self) -> &BackendDescriptor {
        &self.descriptor
    }

    /// Core's atomic, database-only control group reserves several validated
    /// calls in one owning transaction. This never authorizes a process launch.
    pub async fn into_control_reservation(
        self,
    ) -> Result<(DispatchRequest, AuthorizationDecision), BackendError> {
        if self.descriptor.class != aura_security_contract::BackendClass::CoreBroker {
            return Err(rejected("orchestrator.control_backend"));
        }
        let plan = self.prepare().await?;
        let request = dispatch_request(self.operation, plan, &self.authorization)?;
        Ok((request, self.authorization))
    }
    async fn prepare(&self) -> Result<ExecutionPlan, BackendError> {
        let plan = self
            .backend
            .prepare(PrepareRequest {
                operation: self.operation.clone(),
                requirement: self.authorization.scope.backend_requirement.clone(),
            })
            .await?;
        plan.validate().map_err(|_| rejected("orchestrator.plan_invalid"))?;
        if plan.prepared.backend != self.descriptor
            || plan.prepared.operation_fingerprint != self.operation.fingerprint
        {
            return Err(rejected("orchestrator.preparation_changed"));
        }
        Ok(plan)
    }
}
pub enum DispatchCompletion {
    Terminal(Box<ExecutionResult>),
    Unknown { attempt: AttemptRef },
}
struct HeldExecution {
    _permit: ToolDispatchPermitV1,
    backend: Arc<dyn ExecutionBackend>,
    attempt: AttemptRef,
}
type Quarantine = Arc<Mutex<BTreeMap<SecurityOperationId, Vec<HeldExecution>>>>;
/// Even cancelling/dropping the caller's future cannot release a process lock
/// without termination evidence. The owner can later reconcile the same attempt.
struct ExecutionLease {
    execution: Option<HeldExecution>,
    quarantine: Quarantine,
}
impl ExecutionLease {
    fn confirm(&mut self) {
        self.execution.take();
    }
}
impl Drop for ExecutionLease {
    fn drop(&mut self) {
        if let Some(execution) = self.execution.take() {
            // A poisoned bookkeeping mutex is not permission to release locks.
            let mut map = self.quarantine.lock().unwrap_or_else(|v| v.into_inner());
            // Keep every guard even if a broken Store violates uniqueness.
            map.entry(execution.attempt.operation_id).or_default().push(execution);
        }
    }
}
pub struct ExecutionOrchestrator {
    scheduler: ToolExecutionSchedulerV1,
    quarantine: Quarantine,
}
impl ExecutionOrchestrator {
    /// A managed process runtime keeps Job/ACL ownership independently of the
    /// Tool's scheduling permit. Recording a lost reply must not retain task
    /// locks forever or prevent the model from issuing its completion decision.
    pub fn release_managed_call(&self, attempt: &AttemptRef) -> Result<(), BackendError> {
        let mut map = self.quarantine.lock().unwrap_or_else(|v| v.into_inner());
        if let Some(held) = map.get(&attempt.operation_id) {
            if held.iter().any(|v| {
                v.attempt != *attempt
                    || !matches!(
                        v.backend.descriptor().class,
                        aura_security_contract::BackendClass::RestrictedUserOffline
                            | aura_security_contract::BackendClass::RestrictedUserOnline
                            | aura_security_contract::BackendClass::Native
                    )
            }) {
                return Err(rejected("orchestrator.unmanaged_execution"));
            }
            map.remove(&attempt.operation_id);
        }
        Ok(())
    }
    pub fn new(concurrency: u32) -> Result<Self, aura_tool_scheduler::ToolScheduleErrorV1> {
        Ok(Self {
            scheduler: ToolExecutionSchedulerV1::new(concurrency)?,
            quarantine: Default::default(),
        })
    }

    /// Applies a committed execution-strategy value to the one global capacity.
    /// The scheduler keeps ownership, so no second limiter can be layered on top.
    pub fn set_global_concurrency(
        &self,
        concurrency: u32,
    ) -> Result<(), aura_tool_scheduler::ToolScheduleErrorV1> {
        self.scheduler.set_global_limit(concurrency)
    }

    /// The committed global limit and the number of calls currently holding a
    /// slot, for diagnostics and projection.
    #[must_use]
    pub fn global_capacity(&self) -> (u32, u32) {
        self.scheduler.global_capacity()
    }
    pub async fn select(
        &self,
        input: &PolicyInput<'_>,
        backends: &[Arc<dyn ExecutionBackend>],
        explicit: Option<&BackendId>,
    ) -> Result<Selection, BackendError> {
        let descriptors = backends.iter().map(|v| v.descriptor()).collect::<Vec<_>>();
        let candidates = match selector::select(input, &descriptors, explicit, None, false) {
            Ok(v) => v,
            Err(reason) => return Ok(Selection::Denied { reason }),
        };
        // No fallback after a chosen backend fails preparation. A fresh preview
        // may select again only before any Attempt has been reserved.
        let Some(candidate) = candidates.into_iter().next() else {
            return Ok(Selection::Denied { reason: PolicyDenyReason::BackendUnavailable });
        };
        let authorization = match candidate.authorization {
            PolicyAuthorization::Allow { authorization } => authorization,
            PolicyAuthorization::Ask { proposal } => {
                return Ok(Selection::ApprovalRequired {
                    proposal: Box::new(proposal),
                    backend: Box::new(candidate.backend),
                });
            }
            PolicyAuthorization::Deny { reason } => return Ok(Selection::Denied { reason }),
        };
        let backend = backends
            .iter()
            .zip(&descriptors)
            .find(|(_, d)| **d == candidate.backend)
            .map(|(b, _)| b.clone())
            .ok_or_else(|| rejected("orchestrator.backend_changed"))?;
        Ok(Selection::Ready(Box::new(SelectedCall {
            operation: input.operation.clone(),
            descriptor: candidate.backend,
            authorization,
            backend,
        })))
    }
    pub async fn acquire_control_permit(
        &self,
        schedule: ToolScheduleRequestV1<'_>,
        cancellation: &mut watch::Receiver<bool>,
    ) -> Result<ToolDispatchPermitV1, BackendError> {
        self.scheduler
            .acquire_dispatch_permit(schedule, cancellation)
            .await
            .map_err(|_| rejected("orchestrator.control_cancelled"))
    }
    pub async fn execute(
        &self,
        call: SelectedCall,
        schedule: ToolScheduleRequestV1<'_>,
        store: &dyn DispatchStore,
        cancellation: watch::Receiver<bool>,
        stages: &mut DispatchStages,
    ) -> Result<DispatchCompletion, BackendError> {
        self.execute_observed(call, schedule, store, cancellation, stages, &|_| {}).await
    }
    /// [`Self::execute`], with the one observation a caller may need from inside it.
    ///
    /// WHY THIS SEAM EXISTS
    /// "Real execution started" is a fact only this function knows: it is the instant the
    /// scheduling slot has been granted **and** the real `Attempt` has been reserved. A caller
    /// that publishes an activity before calling this would report approval, capacity and
    /// resource waiting as `running`; a caller that publishes after it returns would miss the
    /// whole backend call. So the exact `AttemptRef` the reservation produced, and the instant
    /// it happened, are handed to `started` here and nowhere else.
    ///
    /// WHAT IT DOES NOT CHANGE
    /// `started` is an observation only. It cannot return an error, cannot change the plan, the
    /// attempt, the authorization or the cancellation path, and every existing exit from this
    /// function is unchanged. The default [`Self::execute`] passes a no-op observer, so no
    /// other caller changes behaviour.
    pub async fn execute_observed(
        &self,
        call: SelectedCall,
        schedule: ToolScheduleRequestV1<'_>,
        store: &dyn DispatchStore,
        mut cancellation: watch::Receiver<bool>,
        stages: &mut DispatchStages,
        started: &(dyn Fn(&AttemptRef) + Send + Sync),
    ) -> Result<DispatchCompletion, BackendError> {
        let wait_at = tokio::time::Instant::now();
        let permit = match self.scheduler.acquire_dispatch_permit(schedule, &mut cancellation).await
        {
            Ok(permit) => permit,
            Err(_) => {
                stages.wait_ms = wait_at.elapsed().as_millis();
                return Err(rejected("orchestrator.cancelled_or_schedule_invalid"));
            }
        };
        stages.wait_ms = wait_at.elapsed().as_millis();
        let activate_at = tokio::time::Instant::now();
        // Every return after the slot is held — cancel, prepare, or reserve —
        // closes the activation clock. A later error must not drop that time.
        let activated = ActivateClock { stages, started: activate_at };
        if *cancellation.borrow() {
            drop(activated);
            return Err(rejected("orchestrator.cancelled_before_reservation"));
        }
        // State preparation belongs to the acquired resource generation. Policy
        // has already allowed the call; reserve rechecks it without prompting.
        // Preparing before the lock would freeze stale state for queued calls.
        let plan = match call.prepare().await {
            Ok(plan) => plan,
            Err(error) => {
                drop(activated);
                return Err(error);
            }
        };
        if *cancellation.borrow() {
            drop(activated);
            return Err(rejected("orchestrator.cancelled_before_reservation"));
        }
        let request = match dispatch_request(call.operation, plan, &call.authorization) {
            Ok(request) => request,
            Err(error) => {
                drop(activated);
                return Err(error);
            }
        };
        let dispatch = match store.reserve(request.clone(), &call.authorization).await {
            Ok(dispatch) => dispatch,
            Err(error) => {
                drop(activated);
                return Err(error);
            }
        };
        if dispatch.request() != &request {
            drop(activated);
            return Err(BackendError::DispatchUnknown { attempt: request.binding.attempt.clone() });
        }
        // THE REAL START. A slot is held and the exact attempt is reserved; nothing below this
        // line can return without an attempt having existed. The ordinal is the reservation's
        // own, never a constant.
        started(&request.binding.attempt);
        let attempt = request.binding.attempt.clone();
        let handle = match aura_domain_common::jcs::canonical_sha256(&request.binding) {
            Ok(dispatch_fingerprint) => {
                ExecutionHandle { attempt: attempt.clone(), dispatch_fingerprint }
            }
            Err(_) => {
                drop(activated);
                return Err(BackendError::DispatchUnknown { attempt: attempt.clone() });
            }
        };
        let runtime_capacity = permit.runtime_capacity();
        let mut lease = ExecutionLease {
            execution: Some(HeldExecution {
                _permit: permit,
                backend: call.backend.clone(),
                attempt: attempt.clone(),
            }),
            quarantine: self.quarantine.clone(),
        };
        drop(activated);
        let _execute_clock = ExecuteClock { stages, started: tokio::time::Instant::now() };
        let backend = call.backend.clone();
        // Dropping this JoinHandle detaches, so backend cleanup and durable
        // completion continue. ExecutionLease quarantines the scheduling guard.
        let deadline =
            tokio::time::Instant::now() + permit_timeout(&lease) + Duration::from_secs(45);
        let dispatch_cancellation = cancellation.clone();
        let mut dispatched = tokio::spawn(async move {
            runtime_capacity.scope(dispatch_cancellation, deadline, backend.execute(dispatch)).await
        });
        let mut cancel_sent = false;
        loop {
            if *cancellation.borrow() && !cancel_sent {
                store.request_cancel(&attempt).await?;
                // Awaiting the same backend's cancel never selects another one.
                let _ = tokio::time::timeout(
                    Duration::from_secs(45),
                    call.backend.cancel(handle.clone()),
                )
                .await;
                cancel_sent = true;
            }
            if dispatched.is_finished() {
                match (&mut dispatched).await {
                    Ok(Ok(actual)) if actual == handle => {
                        cancel_sent = false;
                        break;
                    }
                    Ok(Err(BackendError::DispatchRejected { reason })) => {
                        let result = store.reject_prepared(&request, &reason).await?;
                        if confirmed(&result, &attempt) {
                            lease.confirm();
                        }
                        return Ok(DispatchCompletion::Terminal(Box::new(result)));
                    }
                    _ => return Ok(DispatchCompletion::Unknown { attempt }),
                }
            }
            if tokio::time::Instant::now() >= deadline {
                store.request_cancel(&attempt).await?;
                let _ = tokio::time::timeout(
                    Duration::from_secs(45),
                    call.backend.cancel(handle.clone()),
                )
                .await;
                return Ok(DispatchCompletion::Unknown { attempt });
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        loop {
            if *cancellation.borrow() && !cancel_sent {
                store.request_cancel(&attempt).await?;
                let _ = tokio::time::timeout(
                    Duration::from_secs(45),
                    call.backend.cancel(handle.clone()),
                )
                .await;
                cancel_sent = true;
            }
            match call.backend.reconcile(attempt.clone()).await? {
                ReconcileResult::Completed { result } if result.receipt.attempt == attempt => {
                    result
                        .validate()
                        .map_err(|_| BackendError::DispatchUnknown { attempt: attempt.clone() })?;
                    if confirmed(&result, &attempt) {
                        lease.confirm();
                    }
                    return Ok(DispatchCompletion::Terminal(result));
                }
                ReconcileResult::Running { handle: running } if running == handle => {}
                _ => return Ok(DispatchCompletion::Unknown { attempt }),
            }
            if tokio::time::Instant::now() >= deadline {
                store.request_cancel(&attempt).await?;
                let _ = tokio::time::timeout(Duration::from_secs(45), call.backend.cancel(handle))
                    .await;
                return Ok(DispatchCompletion::Unknown { attempt });
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
    /// No replay: only the original backend and exact attempt may release a
    /// quarantined lock, and only after confirmed termination has been stored.
    pub async fn reconcile_quarantined(
        &self,
        id: SecurityOperationId,
    ) -> Result<ReconcileResult, BackendError> {
        let held = {
            let map = self.quarantine.lock().unwrap_or_else(|v| v.into_inner());
            let held =
                map.get(&id).ok_or_else(|| rejected("orchestrator.no_quarantined_attempt"))?;
            held.iter().map(|v| (v.backend.clone(), v.attempt.clone())).collect::<Vec<_>>()
        };
        let mut result = None;
        for (backend, attempt) in &held {
            let observed = backend.reconcile(attempt.clone()).await?;
            if !matches!(&observed, ReconcileResult::Completed { result } if confirmed(result, attempt))
            {
                return Ok(observed);
            }
            result = Some(observed);
        }
        let mut map = self.quarantine.lock().unwrap_or_else(|v| v.into_inner());
        if map.get(&id).is_some_and(|v| v.len() == held.len()) {
            map.remove(&id);
        }
        result.ok_or_else(|| rejected("orchestrator.no_quarantined_attempt"))
    }
}
fn dispatch_request(
    operation: SecuredOperation,
    plan: ExecutionPlan,
    authority: &AuthorizationDecision,
) -> Result<DispatchRequest, BackendError> {
    Ok(DispatchRequest {
        binding: DispatchBinding {
            attempt: AttemptRef {
                operation_id: operation.operation_id,
                ordinal: std::num::NonZeroU32::MIN,
            },
            operation_fingerprint: operation.fingerprint.clone(),
            authorization_fingerprint: authority
                .fingerprint()
                .map_err(|_| rejected("orchestrator.authorization_invalid"))?,
            plan_fingerprint: plan.fingerprint.clone(),
            cancellation_revision: Revision::INITIAL,
        },
        operation,
        plan,
    })
}
fn permit_timeout(lease: &ExecutionLease) -> Duration {
    lease.execution.as_ref().expect("active execution lease")._permit.timeout()
}
fn confirmed(result: &ExecutionResult, attempt: &AttemptRef) -> bool {
    result.validate().is_ok()
        && result.receipt.attempt == *attempt
        && matches!(
            result.termination,
            TerminationEvidence::Confirmed { .. } | TerminationEvidence::RuntimeManaged { .. }
        )
        && result.completed_at <= UtcTimestamp::now()
}
/// How long one dispatch spent waiting for a slot, preparing it, and running.
#[derive(Clone, Copy, Debug, Default)]
pub struct DispatchStages {
    pub wait_ms: u128,
    pub activate_ms: u128,
    pub execute_ms: u128,
}

/// Records activation time on every exit, including `?`-shaped failures.
struct ActivateClock<'a> {
    stages: &'a mut DispatchStages,
    started: tokio::time::Instant,
}

impl Drop for ActivateClock<'_> {
    fn drop(&mut self) {
        self.stages.activate_ms = self.started.elapsed().as_millis();
    }
}

/// Records execution time on every exit after the backend call has started,
/// including a cancel or timeout that returns without another clock update.
struct ExecuteClock<'a> {
    stages: &'a mut DispatchStages,
    started: tokio::time::Instant,
}

impl Drop for ExecuteClock<'_> {
    fn drop(&mut self) {
        self.stages.execute_ms = self.started.elapsed().as_millis();
    }
}

fn rejected(reason: &str) -> BackendError {
    BackendError::DispatchRejected { reason: reason.into() }
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
