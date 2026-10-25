use super::*;
use aura_backend_api::ExecutionBackend;
use aura_domain_common::{RuntimeInstanceId, UtcTimestamp};
use aura_execution_contract::{
    AttemptRef, BackendReadiness, DispatchRequest, ReconcileResult, SecuredOperation,
    TerminationEvidence,
};
use aura_security_contract::{BackendClass, EnforcementGuarantee};
use aura_windows_user_contract::{RunnerBackgroundUpdate, RunnerObservation, RunnerProgress};
use std::sync::Arc;

struct UnusedPort;

impl WindowsUserCorePort for UnusedPort {
    fn begin_launch<'a>(&'a self, _: &'a DispatchRequest) -> LaunchFuture<'a> {
        unreachable!("descriptor must not launch")
    }
    fn scope(&self, _: &SecuredOperation) -> anyhow::Result<ScopeFacts> {
        unreachable!("descriptor must not request operation scope")
    }
    fn claim(
        &self,
        _: &DispatchRequest,
        _: &mut dyn FnMut() -> anyhow::Result<()>,
    ) -> anyhow::Result<UtcTimestamp> {
        unreachable!("descriptor must not claim an execution")
    }
    fn started(&self, _: &DispatchRequest, _: u32, _: UtcTimestamp) -> anyhow::Result<()> {
        unreachable!("descriptor must not start an execution")
    }
    fn progress(&self, _: &DispatchRequest, _: &RunnerProgress) -> anyhow::Result<()> {
        unreachable!("descriptor must not report progress")
    }
    fn diagnostic(&self, _: &DispatchRequest, _: &str) -> anyhow::Result<()> {
        unreachable!("descriptor must not report diagnostics")
    }
    fn background(&self, _: &DispatchRequest, _: &RunnerBackgroundUpdate) -> anyhow::Result<()> {
        unreachable!("descriptor must not report background state")
    }
    fn runtime_stopped(
        &self,
        _: &DispatchRequest,
        _: bool,
        _: bool,
    ) -> anyhow::Result<TerminationEvidence> {
        unreachable!("descriptor must not stop a runtime")
    }
    fn complete(
        &self,
        _: &DispatchRequest,
        _: Option<&RunnerObservation>,
        _: bool,
        _: bool,
        _: Option<NotStarted>,
        _: Option<&str>,
    ) -> anyhow::Result<TerminationEvidence> {
        unreachable!("descriptor must not complete an execution")
    }
    fn reconcile(&self, _: AttemptRef) -> anyhow::Result<ReconcileResult> {
        unreachable!("descriptor must not reconcile an execution")
    }
}

fn native(root: std::path::PathBuf) -> NativeApprovedBackend {
    NativeApprovedBackend::new(
        Arc::new(installation::InstallationBinding::new(root)),
        RuntimeInstanceId::new(),
        Arc::new(UnusedPort),
        Arc::new(tokio::sync::Semaphore::new(1)),
    )
}

#[test]
fn native_registration_id_does_not_open_installation() {
    let backend = native(std::path::PathBuf::from(r"Z:\aura-native-must-not-read"));
    assert_eq!(NativeApprovedBackend::backend_id().0, NATIVE_BACKEND_ID);
    assert_eq!(backend.descriptor().id.0, NATIVE_BACKEND_ID);
}

#[test]
fn native_is_ready_without_restricted_installation() {
    let root = tempfile::tempdir().expect("temporary native root");
    let descriptor = native(root.path().to_path_buf()).descriptor();
    assert_eq!(descriptor.id.0, NATIVE_BACKEND_ID);
    assert_eq!(descriptor.class, BackendClass::Native);
    assert!(matches!(descriptor.readiness, BackendReadiness::Ready));
    assert!(!descriptor.guarantees.contains(&EnforcementGuarantee::IdentityIsolation));
    assert!(!descriptor.guarantees.contains(&EnforcementGuarantee::DirectNetworkBlockedV1));
    assert!(!descriptor.guarantees.contains(&EnforcementGuarantee::AuthorizedWritesOnly));
    assert!(descriptor.guarantees.contains(&EnforcementGuarantee::ChildProcessControl));
}

#[test]
fn native_shutdown_does_not_open_installation() {
    let backend = native(std::path::PathBuf::from(r"Z:\aura-native-must-not-read"));
    backend.shutdown().expect("native shutdown is local bookkeeping");
}
