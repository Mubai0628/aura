use super::*;
use aura_backend_api::{BackendFuture, ExecutionBackend};

/// Selection can inspect readiness, but every launch method refuses loudly.
pub(super) struct DescriptorOnlyBackend(pub BackendDescriptor);
impl ExecutionBackend for DescriptorOnlyBackend {
    fn descriptor(&self) -> BackendDescriptor {
        self.0.clone()
    }
    fn prepare(&self, _: PrepareRequest) -> BackendFuture<'_, ExecutionPlan> {
        Box::pin(async { Err(BackendError::PreparationRejected { reason: "no_launch".into() }) })
    }
    fn execute(&self, _: AuthorizedDispatch) -> BackendFuture<'_, ExecutionHandle> {
        Box::pin(async { Err(BackendError::PreparationRejected { reason: "no_launch".into() }) })
    }
    fn cancel(&self, handle: ExecutionHandle) -> BackendFuture<'_, CancelReceipt> {
        Box::pin(
            async move { Ok(CancelReceipt { handle, termination: TerminationEvidence::Unknown }) },
        )
    }
    fn reconcile(&self, attempt: AttemptRef) -> BackendFuture<'_, ReconcileResult> {
        Box::pin(async move { Err(BackendError::DispatchUnknown { attempt }) })
    }
}
pub(super) fn candidate(descriptor: BackendDescriptor) -> Arc<dyn ExecutionBackend> {
    Arc::new(DescriptorOnlyBackend(descriptor))
}
pub(super) async fn descriptor(f: &Fixture) -> BackendDescriptor {
    f.service().commands_required().wait_until_ready().await.expect("verify the managed package");
    f.service().commands_required().candidates_for(PermissionMode::FullAccess)[0].descriptor()
}
pub(super) fn legacy_native(base: &BackendDescriptor) -> BackendDescriptor {
    let mut legacy = base.clone();
    legacy.id.0 = aura_backend_windows_user::NATIVE_BACKEND_ID.into();
    // Deliberately give the alternative enough capabilities: refusal must be the
    // default selection rule, rather than an accidental capability mismatch.
    legacy
}
pub(super) fn legacy_restricted(base: &BackendDescriptor) -> BackendDescriptor {
    let mut legacy = base.clone();
    legacy.id.0 = "windows-user-full-v1".into();
    legacy.class = BackendClass::RestrictedUserOnline;
    legacy.guarantees.extend([
        EnforcementGuarantee::IdentityIsolation,
        EnforcementGuarantee::AuthorizedWritesOnly,
        EnforcementGuarantee::CredentialIsolation,
    ]);
    legacy
}
pub(super) async fn case(
    mode: PermissionMode,
    tool: &str,
    network: bool,
) -> (Fixture, CallContext) {
    let f = Fixture::with_exposure(mode, &[tool]).await;
    let batch = f.admit(&[(
        tool,
        json!({"script":"echo selection","requires_network":network,
        "working_directory":".","stdin":"","timeout_ms":1000,"max_output_bytes":4096}),
    )]);
    let context = batch.calls[&batch.batch.calls[0].tool_call_id].clone();
    (f, context)
}
pub(super) async fn select(
    f: &Fixture,
    context: &CallContext,
    candidates: Vec<Arc<dyn ExecutionBackend>>,
    explicit: Option<&str>,
) -> Result<Selection, BackendError> {
    let policy =
        context.policy(UtcTimestamp::now(), f.service().agent_execution_required().runtime);
    f.service().agent_execution_required().select_backend(&policy, &candidates, explicit).await
}
pub(super) fn assert_compat(selected: Result<Selection, BackendError>) {
    match selected {
        Ok(Selection::Ready(backend)) => {
            assert_eq!(
                backend.descriptor().id.0,
                aura_backend_windows_user::compat::COMPAT_BACKEND_ID
            );
            assert_eq!(backend.descriptor().class, BackendClass::Native);
        }
        other => panic!("the reviewed compat default must be selected: {other:?}"),
    }
}
pub(super) fn assert_unavailable(selected: Result<Selection, BackendError>) {
    assert!(
        matches!(selected, Ok(Selection::Denied { reason: PolicyDenyReason::BackendUnavailable })),
        "compat must not fall back to another backend: {selected:?}"
    );
}
