//! Explicit NativeApproved backend. Restricted remains preferred; this path is
//! only selected with an explicit preview or when Restricted is not Ready.
use super::{
    controller::{
        Active, ActiveMap, PortSection, ScopeFacts, WindowsUserCorePort, WindowsUserCorePortSection,
    },
    installation::InstallationBinding,
};
use anyhow::{Result, ensure};
use aura_backend_api::{BackendFuture, ExecutionBackend};
use aura_domain_common::{
    Revision, RuntimeInstanceId, SessionId, ToolWireVersionV1, UtcTimestamp, jcs::canonical_sha256,
};
use aura_execution_contract::*;
use aura_security_contract::{
    BackendClass, EnforcementGuarantee, PermissionMode, ResolvedCapability,
};
use aura_windows_user_contract::*;
use std::sync::Arc;
use tokio::sync::{Semaphore, watch};

pub const NATIVE_BACKEND_ID: &str = "windows-user-native-v1";
const NATIVE_PROTOCOL_VERSION: &str = "native-approved-v1";

pub struct NativeApprovedBackend {
    installation: Arc<InstallationBinding>,
    runtime: RuntimeInstanceId,
    port: Arc<dyn WindowsUserCorePort>,
    gate: Arc<Semaphore>,
    pub(crate) active: ActiveMap,
}

impl NativeApprovedBackend {
    pub fn new(
        installation: Arc<InstallationBinding>,
        runtime: RuntimeInstanceId,
        port: Arc<dyn WindowsUserCorePort>,
        gate: Arc<Semaphore>,
    ) -> Self {
        Self { installation, runtime, port, gate, active: Default::default() }
    }

    pub fn backend_id() -> BackendId {
        BackendId(NATIVE_BACKEND_ID.into())
    }

    pub fn shutdown(&self) -> Result<()> {
        let active = self.active.lock().map_err(|_| anyhow::anyhow!("native.active_lock"))?;
        for item in active.values() {
            let _ = item.cancel.send(true);
        }
        Ok(())
    }

    pub async fn stop_operation(&self, id: aura_domain_common::SecurityOperationId) -> Result<()> {
        let handle = self
            .active
            .lock()
            .map_err(|_| anyhow::anyhow!("native.active_lock"))?
            .get(&id.to_string())
            .map(|v| v.handle.clone());
        if let Some(handle) = handle {
            let receipt = self.cancel(handle).await.map_err(|e| anyhow::anyhow!("{e:?}"))?;
            ensure!(
                matches!(receipt.termination, TerminationEvidence::Confirmed { .. }),
                "native.stop_unconfirmed"
            );
        }
        Ok(())
    }

    pub async fn cancel_session(&self, session: SessionId) -> Result<Vec<TerminationEvidence>> {
        let pending = {
            let active = self.active.lock().map_err(|_| anyhow::anyhow!("native.active_lock"))?;
            active
                .values()
                .filter(|v| v.session == session)
                .map(|v| {
                    let _ = v.cancel.send(true);
                    v.done.clone()
                })
                .collect::<Vec<_>>()
        };
        let mut evidence = Vec::new();
        for mut done in pending {
            loop {
                if let Some(value) = done.borrow().clone() {
                    evidence.push(value);
                    break;
                }
                done.changed().await.map_err(|_| anyhow::anyhow!("native.termination_unknown"))?;
            }
        }
        Ok(evidence)
    }

    fn owner_sid() -> Result<String> {
        Ok(aura_windows_platform::current_user_token_identity()?.0)
    }

    fn shell_image(&self, shell: ManagedShell) -> Result<ExecutionFileBinding> {
        let path = match shell {
            ManagedShell::Cmd => {
                aura_windows_platform::windows_directory()?.join("System32/cmd.exe")
            }
            ManagedShell::PowerShell => self.installation.root.join("runtime/pwsh.exe"),
        };
        ensure!(path.is_file(), "native.shell_image_required");
        Ok(super::file_binding::capture(&path, true)?.0)
    }

    /// The live scope facts through the port's bounded owner.
    pub(crate) async fn scope_section(
        &self,
        operation: &SecuredOperation,
        name: &'static str,
    ) -> Result<ScopeFacts> {
        let port = Arc::clone(&self.port);
        let run = operation.content.identity.agent_run_id;
        let operation = operation.clone();
        self.port
            .section(PortSection::read(name, run), Box::new(move || port.scope(&operation)))
            .await
    }

    fn preparation(
        &self,
        operation: &SecuredOperation,
        valid_until: UtcTimestamp,
        facts: ScopeFacts,
    ) -> Result<WindowsUserPreparation> {
        let command: ManagedShellCommand =
            serde_json::from_value(operation.content.action.clone())?;
        command.validate()?;
        ensure!(
            operation.content.action_kind
                == match command.shell {
                    ManagedShell::PowerShell => "shell.powershell",
                    ManagedShell::Cmd => "shell.cmd",
                },
            "native.action_kind"
        );
        ensure!(
            operation.content.credentials.is_empty(),
            "native.credential_use_requires_core_binding"
        );
        ensure!(
            matches!(
                operation.content.network,
                OperationNetwork::None | OperationNetwork::BroadProcess
            ),
            "native.network_mode"
        );
        ensure!(facts.revision > 0 && !facts.read_roots.is_empty(), "native.mode_changed");
        ensure!(
            facts.mode != PermissionMode::ReadOnly || facts.write_roots.is_empty(),
            "native.readonly_scope"
        );
        let (working_directory, working) =
            super::file_binding::capture(std::path::Path::new(&command.working_directory), false)?;
        ensure!(working.info().is_directory, "native.working_directory_type");
        let roots =
            facts.read_roots.iter().map(super::file_binding::verify).collect::<Result<Vec<_>>>()?;
        ensure!(
            roots
                .iter()
                .any(|root| aura_windows_platform::final_path_within(root.info(), working.info())),
            "native.working_directory_scope"
        );
        ensure!(
            facts.write_roots.iter().all(|write| facts.read_roots.contains(write)),
            "native.write_scope"
        );
        Ok(WindowsUserPreparation {
            schema_version: ToolWireVersionV1,
            assurance_version: ASSURANCE_VERSION.into(),
            shell_environment_version: SHELL_ENVIRONMENT_VERSION.into(),
            installation_fingerprint: canonical_sha256(&(
                NATIVE_PROTOCOL_VERSION,
                NATIVE_BACKEND_ID,
                facts.mode,
            ))?,
            mode: facts.mode,
            permission_revision: facts.revision,
            valid_until,
            account_sid: Self::owner_sid()?,
            shell_image: self.shell_image(command.shell)?,
            working_directory,
            read_roots: facts.read_roots,
            write_roots: facts.write_roots,
        })
    }
}

impl ExecutionBackend for NativeApprovedBackend {
    fn descriptor(&self) -> BackendDescriptor {
        use EnforcementGuarantee::*;
        use ResolvedCapability::*;
        let id = Self::backend_id();
        let readiness = match Self::owner_sid() {
            Ok(_) => BackendReadiness::Ready,
            Err(_) => BackendReadiness::Unavailable { reason: "native.caller_identity".into() },
        };
        BackendDescriptor {
            id: id.clone(),
            revision: Revision::INITIAL,
            runtime_instance_id: self.runtime,
            class: BackendClass::Native,
            compatibility: [
                ExecutionCompatibility::ProcessTreeCancellation,
                ExecutionCompatibility::LongRunningProcess,
                ExecutionCompatibility::NativeWindowsToolchain,
            ]
            .into(),
            capabilities: [
                ProcessExecute,
                WorkspaceFileRead,
                WorkspaceFileWrite,
                WorkspaceFileCreate,
                WorkspaceFileDelete,
                WorkspaceFileMove,
                PublicNetworkRead,
                PrivateNetworkAccess,
                DataEgress,
                DownloadArtifact,
                ExternalServiceRead,
                ExternalServiceMutation,
            ]
            .into(),
            guarantees: [ChildProcessControl, BoundedOutput].into(),
            known_limitations: Default::default(),
            readiness,
            protocol_fingerprint: canonical_sha256(&(
                ASSURANCE_VERSION,
                SHELL_ENVIRONMENT_VERSION,
                NATIVE_PROTOCOL_VERSION,
                id.0.as_str(),
            ))
            .expect("literal protocol fingerprint"),
        }
    }

    fn prepare(&self, request: PrepareRequest) -> BackendFuture<'_, ExecutionPlan> {
        Box::pin(async move {
            let descriptor = self.descriptor();
            if request.operation.validate().is_err()
                || !descriptor.supports_operation(&request.requirement, &request.operation)
            {
                return Err(BackendError::PreparationRejected {
                    reason: "native.unsupported_or_unready".into(),
                });
            }
            let prepare = |facts: ScopeFacts| -> Result<_> {
                let valid_until = UtcTimestamp::from_value(
                    UtcTimestamp::now().value() + time::Duration::seconds(30),
                );
                let preparation = self.preparation(&request.operation, valid_until, facts)?;
                Ok(ExecutionPlan::new(PreparedExecution {
                    operation_fingerprint: request.operation.fingerprint.clone(),
                    backend: descriptor,
                    preparation: serde_json::to_value(preparation)?,
                })?)
            };
            // Boundary: the live scope read goes through the port's bounded owner.
            let facts =
                self.scope_section(&request.operation, "native.scope").await.map_err(|_| {
                    BackendError::PreparationRejected {
                        reason: "native.scope_or_image_invalid".into(),
                    }
                })?;
            prepare(facts).map_err(|_| BackendError::PreparationRejected {
                reason: "native.scope_or_image_invalid".into(),
            })
        })
    }

    fn execute(&self, dispatch: AuthorizedDispatch) -> BackendFuture<'_, ExecutionHandle> {
        Box::pin(async move {
            let request = dispatch.request().clone();
            // Boundary: the dispatch revalidation reads the live scope through the
            // port's bounded owner.
            let scope = crate::managed_operations::prepared_scope(
                self.port.clone(),
                &request,
                "native.scope_before_start",
            )
            .await
            .map_err(|_| BackendError::DispatchRejected {
                reason: "native.dispatch_revalidation_failed".into(),
            })?;
            let ready = |scope: ScopeFacts| -> Result<_> {
                request.plan.validate()?;
                let prepared: WindowsUserPreparation =
                    serde_json::from_value(request.plan.prepared.preparation.clone())?;
                ensure!(
                    prepared.valid_until > UtcTimestamp::now()
                        && request.plan.prepared.backend == self.descriptor(),
                    "native.preparation_expired_or_binding_changed"
                );
                ensure!(
                    self.preparation(&request.operation, prepared.valid_until, scope)? == prepared,
                    "native.scope_changed"
                );
                Ok(prepared)
            };
            let prepared = ready(scope).map_err(|_| BackendError::DispatchRejected {
                reason: "native.dispatch_revalidation_failed".into(),
            })?;
            let handle = ExecutionHandle {
                attempt: request.binding.attempt.clone(),
                dispatch_fingerprint: canonical_sha256(&request.binding).map_err(|_| {
                    BackendError::DispatchRejected {
                        reason: "native.dispatch_revalidation_failed".into(),
                    }
                })?,
            };
            let session = request.operation.content.identity.session_id.ok_or_else(|| {
                BackendError::DispatchRejected {
                    reason: "native.dispatch_revalidation_failed".into(),
                }
            })?;
            let (cancel, cancelled) = watch::channel(false);
            let (complete, done) = watch::channel(None);
            let key = handle.attempt.operation_id.to_string();
            // Boundary: the claim and the launch registration are one indivisible
            // section on the port's bounded owner.
            let claim_port = Arc::clone(&self.port);
            let claim_request = request.clone();
            let claim_key = key.clone();
            let claim_handle = handle.clone();
            let claim_cancel = cancel.clone();
            let claim_done = done.clone();
            let claim_active = Arc::clone(&self.active);
            let expires = self
                .port
                .section(
                    PortSection::write(
                        "native.claim",
                        request.operation.content.identity.agent_run_id,
                    ),
                    Box::new(move || {
                        let mut register = || -> Result<()> {
                            let mut active = claim_active
                                .lock()
                                .map_err(|_| anyhow::anyhow!("native.active_lock"))?;
                            ensure!(!active.contains_key(&claim_key), "native.replay");
                            active.insert(
                                claim_key.clone(),
                                Active {
                                    handle: claim_handle.clone(),
                                    session,
                                    cancel: claim_cancel.clone(),
                                    done: claim_done.clone(),
                                },
                            );
                            Ok(())
                        };
                        claim_port.claim(&claim_request, &mut register)
                    }),
                )
                .await
                .map_err(|_| BackendError::DispatchRejected {
                    reason: "native.dispatch_revalidation_failed".into(),
                })?;
            let port = self.port.clone();
            let gate = self.gate.clone();
            let active = self.active.clone();
            let runtime = self.installation.root.join("runtime");
            let observation =
                crate::observation::ExecutionObservation::capture(self.port.as_ref(), &request);
            tokio::spawn(async move {
                let evidence = super::native_execute::run(
                    request,
                    prepared,
                    runtime,
                    expires,
                    cancelled,
                    gate,
                    port,
                    observation,
                )
                .await;
                let evidence = evidence.unwrap_or(TerminationEvidence::Unknown);
                let confirmed = matches!(evidence, TerminationEvidence::Confirmed { .. });
                let _ = complete.send(Some(evidence));
                if confirmed && let Ok(mut map) = active.lock() {
                    map.remove(&key);
                }
            });
            Ok(handle)
        })
    }

    fn cancel(&self, handle: ExecutionHandle) -> BackendFuture<'_, CancelReceipt> {
        Box::pin(async move {
            let mut pending = {
                let map = self.active.lock().map_err(|_| BackendError::DispatchUnknown {
                    attempt: handle.attempt.clone(),
                })?;
                if let Some(active) = map.get(&handle.attempt.operation_id.to_string()) {
                    if active.handle != handle {
                        return Err(BackendError::DispatchRejected {
                            reason: "native.cancel_binding".into(),
                        });
                    }
                    let _ = active.cancel.send(true);
                    Some(active.done.clone())
                } else {
                    None
                }
            };
            let termination = if let Some(done) = &mut pending {
                loop {
                    if let Some(value) = done.borrow().clone() {
                        break value;
                    }
                    if done.changed().await.is_err() {
                        break TerminationEvidence::Unknown;
                    }
                }
            } else {
                // Boundary: the durable attempt read through the bounded owner.
                let port = Arc::clone(&self.port);
                let read_attempt = handle.attempt.clone();
                match self
                    .port
                    .section(
                        PortSection::read("native.cancel_reconcile", None),
                        Box::new(move || port.reconcile(read_attempt)),
                    )
                    .await
                {
                    Ok(ReconcileResult::Completed { result }) => result.termination,
                    _ => TerminationEvidence::Unknown,
                }
            };
            Ok(CancelReceipt { handle, termination })
        })
    }

    fn reconcile(&self, attempt: AttemptRef) -> BackendFuture<'_, ReconcileResult> {
        Box::pin(async move {
            // Boundary: both durable reads go through the port's bounded owner.
            let port = Arc::clone(&self.port);
            let read_attempt = attempt.clone();
            if let Ok(result @ ReconcileResult::Completed { .. }) = self
                .port
                .section(
                    PortSection::read("native.reconcile", None),
                    Box::new(move || port.reconcile(read_attempt)),
                )
                .await
            {
                return Ok(result);
            }
            if let Ok(map) = self.active.lock()
                && let Some(active) = map.get(&attempt.operation_id.to_string())
            {
                if active.handle.attempt != attempt {
                    return Err(BackendError::DispatchRejected {
                        reason: "native.attempt_mismatch".into(),
                    });
                }
                return Ok(ReconcileResult::Running { handle: active.handle.clone() });
            }
            let port = Arc::clone(&self.port);
            let read_attempt = attempt.clone();
            self.port
                .section(
                    PortSection::read("native.reconcile", None),
                    Box::new(move || port.reconcile(read_attempt)),
                )
                .await
                .map_err(|_| BackendError::DispatchUnknown { attempt })
        })
    }
}
