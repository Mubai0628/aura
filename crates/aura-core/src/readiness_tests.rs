use super::*;
use aura_backend_api::{BackendFuture, ExecutionBackend};
use aura_execution_contract::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture {
    descriptor: Mutex<BackendDescriptor>,
    descriptor_reads: AtomicUsize,
    descriptor_delay: std::time::Duration,
}
impl ExecutionBackend for Fixture {
    fn descriptor(&self) -> BackendDescriptor {
        self.descriptor_reads.fetch_add(1, Ordering::AcqRel);
        std::thread::sleep(self.descriptor_delay);
        self.descriptor.lock().expect("fixture").clone()
    }
    fn prepare(&self, _: PrepareRequest) -> BackendFuture<'_, ExecutionPlan> {
        panic!("readiness must not execute")
    }
    fn execute(&self, _: AuthorizedDispatch) -> BackendFuture<'_, ExecutionHandle> {
        panic!("readiness must not execute")
    }
    fn cancel(&self, _: ExecutionHandle) -> BackendFuture<'_, CancelReceipt> {
        panic!("readiness must not execute")
    }
    fn reconcile(&self, _: AttemptRef) -> BackendFuture<'_, ReconcileResult> {
        panic!("readiness must not execute")
    }
}

#[test]
fn registered_execution_capability_tracks_live_backend_without_blocking_broker() {
    let store = ReadinessStore::default();
    let process = |snapshot: RuntimeReadiness| {
        snapshot
            .capabilities
            .into_iter()
            .find(|v| v.capability == "process.execution")
            .expect("process")
            .readiness
    };
    store.update("strict_appcontainer", BackendLifecycle::Ready, "legacy lifecycle");
    assert_eq!(process(store.snapshot(CoreHealth::Ready)), CapabilityReadiness::Unavailable);
    let mut descriptor =
        aura_contract_schema::v2_kernel_fixture().expect("fixture").dispatch.plan.prepared.backend;
    descriptor.id = BackendId("registered-unit-backend".into());
    descriptor.class = aura_security_contract::BackendClass::RestrictedUserOffline;
    descriptor.capabilities.insert(aura_security_contract::ResolvedCapability::ProcessExecute);
    descriptor.readiness = BackendReadiness::SetupRequired { reason: "not installed".into() };
    let backend = Arc::new(Fixture {
        descriptor: Mutex::new(descriptor),
        descriptor_reads: AtomicUsize::new(0),
        descriptor_delay: std::time::Duration::ZERO,
    });
    store
        .register_backend(BackendId("registered-unit-backend".into()), backend.clone())
        .expect("register");
    assert_eq!(backend.descriptor_reads.load(Ordering::Acquire), 0);
    assert!(
        store
            .register_backend(BackendId("registered-unit-backend".into()), backend.clone())
            .is_err()
    );
    assert_eq!(backend.descriptor_reads.load(Ordering::Acquire), 0);
    let unavailable = store.snapshot(CoreHealth::Ready);
    assert_eq!(process(unavailable.clone()), CapabilityReadiness::Unavailable);
    assert!(
        unavailable
            .capabilities
            .iter()
            .filter(|v| v.capability != "process.execution")
            .all(|v| v.readiness == CapabilityReadiness::Ready)
    );
    backend.descriptor.lock().expect("fixture").readiness = BackendReadiness::Ready;
    let ready = store.snapshot(CoreHealth::Ready);
    assert!(ready.revision > unavailable.revision);
    assert_eq!(process(ready), CapabilityReadiness::Ready);
    assert_eq!(
        process(store.snapshot(CoreHealth::RecoveryRequired)),
        CapabilityReadiness::Unavailable
    );
    backend.descriptor.lock().expect("fixture").readiness =
        BackendReadiness::RepairRequired { reason: "resource changed".into() };
    assert_eq!(process(store.snapshot(CoreHealth::Ready)), CapabilityReadiness::Unavailable);
}

#[tokio::test(flavor = "current_thread")]
async fn slow_backend_readiness_does_not_block_runtime_tasks() {
    let store = ReadinessStore::default();
    let mut descriptor =
        aura_contract_schema::v2_kernel_fixture().expect("fixture").dispatch.plan.prepared.backend;
    descriptor.id = BackendId("slow-unit-backend".into());
    descriptor.class = aura_security_contract::BackendClass::RestrictedUserOffline;
    descriptor.readiness = BackendReadiness::Ready;
    let backend = Arc::new(Fixture {
        descriptor: Mutex::new(descriptor),
        descriptor_reads: AtomicUsize::new(0),
        descriptor_delay: std::time::Duration::from_millis(100),
    });
    store.register_backend(BackendId("slow-unit-backend".into()), backend).expect("register");
    let runtime_progress = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&runtime_progress);
    let progress = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        observed.store(1, Ordering::Release);
    });
    let snapshot = store.snapshot_nonblocking(CoreHealth::Ready).await;
    progress.await.expect("runtime progress");
    assert_eq!(runtime_progress.load(Ordering::Acquire), 1);
    assert!(snapshot.backends.iter().any(|backend| backend.backend == "slow-unit-backend"));
}
