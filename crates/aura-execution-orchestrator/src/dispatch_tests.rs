use super::*;
use aura_backend_api::BackendFuture;
use aura_contract_schema::{V2KernelFixture, v2_kernel_fixture};
use aura_domain_common::{ToolCallId, ToolInstanceId};
use aura_tool_contract::{ToolExecutionPlanCallV1, ToolVersion};
use aura_tool_scheduler::ToolInstanceExecutionGenerationKeyV1;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Fixture {
    facts: V2KernelFixture,
    reject: bool,
    lost_reply: bool,
    wait_for_cancel: bool,
    reserved: AtomicUsize,
    preparations: AtomicUsize,
    entered: AtomicBool,
    cancelled: AtomicBool,
    stopped: AtomicBool,
    cancels: AtomicUsize,
}
impl Fixture {
    fn new(reject: bool, lost_reply: bool, wait_for_cancel: bool) -> Arc<Self> {
        let mut facts = v2_kernel_fixture().expect("facts");
        facts.result.termination =
            TerminationEvidence::Confirmed { evidence_ref: facts.result.receipt.evidence_ref };
        facts.result.side_effect = SideEffectStateV1::NoEffect;
        facts.result.receipt.rollback = RollbackEvidence::NotApplicable;
        facts.result.warnings.clear();
        Arc::new(Self {
            facts,
            reject,
            lost_reply,
            wait_for_cancel,
            reserved: AtomicUsize::new(0),
            preparations: AtomicUsize::new(0),
            entered: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            cancels: AtomicUsize::new(0),
        })
    }
    fn selected(self: &Arc<Self>) -> SelectedCall {
        SelectedCall {
            operation: self.facts.dispatch.operation.clone(),
            descriptor: self.descriptor(),
            authorization: self.facts.authority.clone(),
            backend: self.clone(),
        }
    }
    fn handle(&self) -> ExecutionHandle {
        ExecutionHandle {
            attempt: self.facts.dispatch.binding.attempt.clone(),
            dispatch_fingerprint: aura_domain_common::jcs::canonical_sha256(
                &self.facts.dispatch.binding,
            )
            .expect("binding hash"),
        }
    }
}
impl DispatchStore for Fixture {
    fn reserve<'a>(
        &'a self,
        request: DispatchRequest,
        authority: &'a AuthorizationDecision,
    ) -> BackendFuture<'a, AuthorizedDispatch> {
        Box::pin(async move {
            if self.reserved.fetch_add(1, Ordering::SeqCst) != 0 {
                return Err(rejected("duplicate"));
            }
            AuthorizedDispatch::revalidate(
                request,
                authority,
                &self.descriptor(),
                &self.facts.cancellation,
                self.facts.authority.decided_at,
            )
            .map_err(|_| rejected("invalid reservation"))
        })
    }
    fn reject_prepared<'a>(
        &'a self,
        _: &'a DispatchRequest,
        _: &'a str,
    ) -> BackendFuture<'a, ExecutionResult> {
        Box::pin(async move {
            let mut result = self.facts.result.clone();
            result.outcome = if self.cancelled.load(Ordering::SeqCst) {
                ExecutionOutcome::Cancelled
            } else {
                ExecutionOutcome::Failed
            };
            result.side_effect = SideEffectStateV1::NoEffect;
            result.receipt.rollback = RollbackEvidence::NotApplicable;
            result.warnings = vec!["execution.not_dispatched".into()];
            Ok(result)
        })
    }
    fn request_cancel<'a>(&'a self, _: &'a AttemptRef) -> BackendFuture<'a, ()> {
        Box::pin(async move {
            self.cancelled.store(true, Ordering::SeqCst);
            Ok(())
        })
    }
}
impl ExecutionBackend for Fixture {
    fn descriptor(&self) -> BackendDescriptor {
        self.facts.dispatch.plan.prepared.backend.clone()
    }
    fn prepare(&self, _: PrepareRequest) -> BackendFuture<'_, ExecutionPlan> {
        Box::pin(async {
            self.preparations.fetch_add(1, Ordering::SeqCst);
            Ok(self.facts.dispatch.plan.clone())
        })
    }
    fn execute(&self, _: AuthorizedDispatch) -> BackendFuture<'_, ExecutionHandle> {
        Box::pin(async {
            self.entered.store(true, Ordering::SeqCst);
            if self.wait_for_cancel {
                while !self.cancelled.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                // Models cancellation reaching the owning Store before backend
                // task registration. A late claim must observe that fact.
                self.stopped.store(true, Ordering::SeqCst);
                return Err(rejected("cancelled before claim"));
            }
            if self.reject {
                return Err(rejected("pre-dispatch refusal"));
            }
            if self.lost_reply {
                return Err(BackendError::DispatchUnknown { attempt: self.handle().attempt });
            }
            Ok(self.handle())
        })
    }
    fn cancel(&self, handle: ExecutionHandle) -> BackendFuture<'_, CancelReceipt> {
        Box::pin(async move {
            assert!(
                self.cancelled.load(Ordering::SeqCst),
                "durable cancellation precedes backend signal"
            );
            self.cancels.fetch_add(1, Ordering::SeqCst);
            Ok(CancelReceipt { handle, termination: TerminationEvidence::Unknown })
        })
    }
    fn reconcile(&self, attempt: AttemptRef) -> BackendFuture<'_, ReconcileResult> {
        Box::pin(async move {
            if self.stopped.load(Ordering::SeqCst) {
                Ok(ReconcileResult::Completed { result: Box::new(self.facts.result.clone()) })
            } else {
                Ok(ReconcileResult::Unknown { attempt })
            }
        })
    }
}
fn plan() -> ToolExecutionPlanCallV1 {
    ToolExecutionPlanCallV1 {
        tool_call_id: ToolCallId::new(),
        ordinal: 1,
        dependency_tool_call_ids: vec![],
        execution_atomic_group: None,
        resolved_resource_claims: vec![],
        timeout_ms: 1000,
        max_retries: 0,
        local_concurrency_limit: Some(1),
        entity_revision: Revision::INITIAL,
    }
}
fn schedule(plan_call: &ToolExecutionPlanCallV1) -> ToolScheduleRequestV1<'_> {
    ToolScheduleRequestV1 {
        plan_call,
        generation: ToolInstanceExecutionGenerationKeyV1 {
            tool_instance_id: ToolInstanceId::from_bytes([1; 16]),
            tool_version: ToolVersion::new(None, Revision::INITIAL).expect("version"),
            execution_fingerprint: aura_domain_common::jcs::CanonicalHashV1::sha256(
                b"one instance",
            ),
            local_concurrency_limit: 1,
        },
        source_lock_key: None,
    }
}
async fn wait_entered(f: &Fixture) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !f.entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("backend entered");
}

#[tokio::test]
async fn cancellation_before_reservation_creates_no_attempt() {
    let f = Fixture::new(false, false, false);
    let engine = ExecutionOrchestrator::new(1).expect("scheduler");
    let (_, cancel) = watch::channel(true);
    let mut stages = DispatchStages::default();
    assert!(
        engine.execute(f.selected(), schedule(&plan()), &*f, cancel, &mut stages).await.is_err()
    );
    assert_eq!(f.reserved.load(Ordering::SeqCst), 0);
    assert!(!f.entered.load(Ordering::SeqCst));
    assert_eq!(stages.execute_ms, 0, "cancellation before reservation does not run the tool");
}

#[tokio::test]
async fn backend_state_is_prepared_only_after_waiting_for_the_resource_permit() {
    let f = Fixture::new(true, false, false);
    let engine = Arc::new(ExecutionOrchestrator::new(1).expect("single-slot scheduler"));
    let (_sender, mut cancel) = watch::channel(false);
    let held = engine
        .scheduler
        .acquire_dispatch_permit(schedule(&plan()), &mut cancel)
        .await
        .expect("occupy scheduler slot");
    let worker = {
        let f = f.clone();
        let engine = engine.clone();
        tokio::spawn(async move {
            let mut stages = DispatchStages::default();
            let result =
                engine.execute(f.selected(), schedule(&plan()), &*f, cancel, &mut stages).await;
            (result, stages)
        })
    };
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert_eq!(f.preparations.load(Ordering::SeqCst), 0);
    assert_eq!(f.reserved.load(Ordering::SeqCst), 0);
    drop(held);
    let (result, stages) = worker.await.expect("dispatch worker joined");
    assert!(matches!(result.expect("dispatch completed"), DispatchCompletion::Terminal(_)));
    assert!(
        stages.wait_ms >= 40,
        "the occupied slot is waiting time, not execution: {}ms",
        stages.wait_ms
    );
    assert_eq!(f.preparations.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn exact_predispatch_refusal_releases_lock_after_persisted_evidence() {
    let f = Fixture::new(true, false, false);
    let engine = ExecutionOrchestrator::new(1).expect("scheduler");
    let (_tx, mut cancel) = watch::channel(false);
    let plan = plan();
    let mut stages = DispatchStages::default();
    let result = engine
        .execute(f.selected(), schedule(&plan), &*f, cancel.clone(), &mut stages)
        .await
        .expect("dispatch");
    assert!(
        matches!(result, DispatchCompletion::Terminal(result) if result.outcome == ExecutionOutcome::Failed)
    );
    tokio::time::timeout(
        Duration::from_secs(1),
        engine.scheduler.acquire_dispatch_permit(schedule(&plan), &mut cancel),
    )
    .await
    .expect("lock released")
    .expect("permit");
    assert!(engine.quarantine.lock().expect("mutex").is_empty());
}

#[tokio::test]
async fn lost_reply_retains_lock_and_reconciliation_never_replays() {
    let f = Fixture::new(false, true, false);
    let engine = ExecutionOrchestrator::new(1).expect("scheduler");
    let (_tx, mut cancel) = watch::channel(false);
    let plan = plan();
    let mut stages = DispatchStages::default();
    assert!(matches!(
        engine
            .execute(f.selected(), schedule(&plan), &*f, cancel.clone(), &mut stages)
            .await
            .expect("dispatch"),
        DispatchCompletion::Unknown { .. }
    ));
    assert!(stages.wait_ms < 2_000 && stages.activate_ms < 2_000 && stages.execute_ms < 2_000);
    assert!(
        tokio::time::timeout(
            Duration::from_millis(50),
            engine.scheduler.acquire_dispatch_permit(schedule(&plan), &mut cancel)
        )
        .await
        .is_err()
    );
    f.stopped.store(true, Ordering::SeqCst);
    engine
        .reconcile_quarantined(f.facts.dispatch.operation.operation_id)
        .await
        .expect("same attempt reconciliation");
    tokio::time::timeout(
        Duration::from_secs(1),
        engine.scheduler.acquire_dispatch_permit(schedule(&plan), &mut cancel),
    )
    .await
    .expect("confirmed stop releases")
    .expect("permit");
    assert_eq!(f.reserved.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn early_cancel_is_persisted_before_backend_signal() {
    let f = Fixture::new(false, false, true);
    let engine = Arc::new(ExecutionOrchestrator::new(1).expect("scheduler"));
    let (tx, cancel) = watch::channel(false);
    let worker = {
        let f = f.clone();
        let engine = engine.clone();
        tokio::spawn(async move {
            let mut stages = DispatchStages::default();
            let result =
                engine.execute(f.selected(), schedule(&plan()), &*f, cancel, &mut stages).await;
            (result, stages)
        })
    };
    wait_entered(&f).await;
    tx.send(true).expect("cancel");
    let (outcome, stages) = worker.await.expect("task");
    let outcome = outcome.expect("dispatch");
    assert!(
        matches!(outcome, DispatchCompletion::Terminal(result) if result.outcome == ExecutionOutcome::Cancelled)
    );
    assert!(stages.execute_ms < 5_000, "cancellation is part of execution time, and it ends");
    assert!(f.stopped.load(Ordering::SeqCst));
    assert_eq!(f.cancels.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn dropping_execution_future_retains_lock_until_original_backend_stops() {
    let f = Fixture::new(false, false, true);
    let engine = Arc::new(ExecutionOrchestrator::new(1).expect("scheduler"));
    let (_tx, cancel) = watch::channel(false);
    let worker = {
        let f = f.clone();
        let engine = engine.clone();
        tokio::spawn(async move {
            let mut stages = DispatchStages::default();
            engine.execute(f.selected(), schedule(&plan()), &*f, cancel, &mut stages).await
        })
    };
    wait_entered(&f).await;
    worker.abort();
    assert!(matches!(worker.await, Err(error) if error.is_cancelled()));
    assert_eq!(engine.quarantine.lock().expect("mutex").len(), 1);
    f.cancelled.store(true, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(2), async {
        while !f.stopped.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("detached backend cleanup");
    engine.reconcile_quarantined(f.facts.dispatch.operation.operation_id).await.expect("reconcile");
    assert!(engine.quarantine.lock().expect("mutex").is_empty());
}
