//! Core-side Backend. Authority and final Result stay behind CorePort; this
//! controller owns only process, pipe and temporary filesystem resources.
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
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::sync::{Semaphore, watch};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeFacts {
    pub mode: PermissionMode,
    pub revision: u64,
    pub read_roots: Vec<ExecutionFileBinding>,
    pub write_roots: Vec<ExecutionFileBinding>,
}
/// The five budgets one launch spends, named so they cannot be confused with each
/// other.
///
/// The incident this comes from had exactly one budget — a thirty-second
/// "preparation is valid until" that the Runner checked *after* the access-control
/// work had already run. Two CMD launches spent 30.6 s and 32.9 s in that work, so
/// they expired before their script process existed, while the PowerShell launches
/// at 25.6 s and 27.3 s got through. Nothing was wrong with CMD, the script, or the
/// user's approval: a preparation budget was being spent as if it were a launch
/// window, and then the launch was refused for it.
///
/// Raising that number would have hidden the defect rather than fixing it, because
/// the real question is never "how long did preparation take" — it is "is the
/// authority to start still live *now*". So the budgets are separate facts:
///
/// * [`PREPARATION_TRUST`] bounds how long the *facts* a preparation captured may be
///   reused. It is a plan bound, not a credential: nothing about starting a process
///   is decided by it.
/// * [`LAUNCH_ELIGIBILITY`] is the short window in which a finished preparation may
///   actually dispatch. It is decided **after** the resources exist, so no amount of
///   preparation can consume it.
/// * [`QUEUE_BUDGET`] bounds the wait for the shared gate, before any resource work.
/// * [`STARTUP_HANDSHAKE`] bounds the pipe, process and Runner-ready handshake.
/// * the user's own `timeout_ms` bounds the script itself, and is deliberately not
///   part of any of the above.
pub mod budgets {
    use std::time::Duration;

    /// How long one preparation's captured facts may be reused.
    pub const PREPARATION_TRUST: Duration = Duration::from_secs(10 * 60);
    /// How long the access-control work itself may take, measured from the moment the
    /// launch starts touching resources.
    ///
    /// This is the budget the incident was missing. `PREPARATION_TRUST` says how long a
    /// preparation's facts stay usable and is checked once, before any resource work, so
    /// it cannot bound that work. The tree walk is synchronous and cannot be cancelled
    /// from outside, so it has to check its own clock and its own cancellation signal as
    /// it goes: a deadline checked only before the walk is not a bound on the walk. Ten
    /// minutes bounds pathology, it is not a performance target — a real workspace
    /// finishes in seconds, and one that cannot be prepared inside this has to fail with
    /// its partial work reported rather than run forever.
    pub const PREPARATION_BUDGET: Duration = Duration::from_secs(10 * 60);
    /// How long a finished preparation may dispatch before it is stale.
    pub const LAUNCH_ELIGIBILITY: Duration = Duration::from_secs(30);
    /// How long a launch waits for its share of the shared gate.
    pub const QUEUE_BUDGET: Duration = Duration::from_secs(5 * 60);
    /// How long the pipe, process and Runner-ready handshake may take.
    pub const STARTUP_HANDSHAKE: Duration = Duration::from_secs(30);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotStarted {
    Cancelled,
    Expired,
    Rejected,
}
pub type LaunchLease = Box<dyn Send + Sync>;
pub type LaunchFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<LaunchLease>> + Send + 'a>>;

/// The bounded sub-stages of one launch, in the order they happen.
///
/// The Core-level `tool.stage` log separates prepare, resource wait, activation, execute
/// and commit, which is enough to see that a call spent 15-20 seconds before its script
/// process existed and not enough to say where. These are the stages inside that span: no
/// field here is a guess, and a stage that was never reached is simply absent from the
/// log rather than reported as zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchStageKind {
    /// ACLs, credential and scope re-verification before anything is created.
    Verified,
    /// Scratch creation and the workspace/scratch ACL grant walk.
    ResourcesAllocated,
    /// The execution pipe server exists.
    PipeCreated,
    /// The restricted account's Runner process was created.
    ProcessCreated,
    /// Core is connected to the Runner's pipe.
    PipeConnected,
    /// The Runner reported Ready after verifying its own identity.
    RunnerReady,
    /// Ancestor metadata access was granted to the authenticated Runner token.
    WorkspaceGranted,
    /// The attempt lease was claimed immediately before Execute.
    LaunchClaimed,
    /// The Execute frame was written; the process may now run the script.
    Dispatched,
}

/// One measured launch sub-stage. `ms` is this process's monotonic measurement.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchStage {
    pub stage: LaunchStageKind,
    pub ms: u64,
}

/// Which bounded owner section one synchronous port call belongs to.
///
/// The port is the only thing that knows whether a call reads or writes the authoritative
/// store, and Core is the only thing that knows how to run it without occupying a reactor
/// worker. This is the value the two sides agree on, so a backend future can hand a
/// synchronous section to the owner that answers for it instead of running it inline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortSection {
    /// The step's own name, e.g. `runner.scope`.
    pub name: &'static str,
    /// The agent run the call belongs to, when it carries one.
    pub run: Option<aura_domain_common::AgentRunId>,
    /// Whether the section writes the authoritative store.
    pub write: bool,
}

impl PortSection {
    #[must_use]
    pub const fn read(name: &'static str, run: Option<aura_domain_common::AgentRunId>) -> Self {
        Self { name, run, write: false }
    }

    #[must_use]
    pub const fn write(name: &'static str, run: Option<aura_domain_common::AgentRunId>) -> Self {
        Self { name, run, write: true }
    }
}

/// One port section, already handed to the owner that will run it.
pub type PortFuture<'a, T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send + 'a>>;

pub trait WindowsUserCorePort: Send + Sync {
    /// Captures information for the actual worker. No row is published until its real start.
    /// This optional hook does not change dispatch, permission, or reconciliation semantics.
    fn execution_observer(
        &self,
        _request: &DispatchRequest,
    ) -> Option<Arc<dyn crate::ShellExecutionObserver>> {
        None
    }
    /// Runs one synchronous section of this port on the owner that answers for it.
    ///
    /// Core overrides this to run the section on its **single bounded owner**, so a journal
    /// lock and its SQL never occupy the reactor worker polling a backend future. The
    /// default runs the work where it was called, which is what a port with no store
    /// underneath needs; a product port must not rely on that default.
    ///
    /// The answer is type-erased here so the seam stays **object safe**: the backend holds
    /// `Arc<dyn WindowsUserCorePort>`. Callers use [`WindowsUserCorePortSection::section`],
    /// which keeps the type and does the one checked downcast.
    fn section_erased(
        &self,
        _section: PortSection,
        work: Box<dyn FnOnce() -> Result<Box<dyn std::any::Any + Send>> + Send + 'static>,
    ) -> PortFuture<'_, Box<dyn std::any::Any + Send>> {
        Box::pin(async move { work() })
    }
    fn scope(&self, operation: &SecuredOperation) -> Result<ScopeFacts>;
    /// A fresh scope for a previously authorized immutable preparation.
    fn prepared_scope(&self, request: &DispatchRequest) -> Result<ScopeFacts> {
        self.scope(&request.operation)
    }
    /// Core holds the shared mode barrier, validates live authority and claims
    /// the stored Attempt, then registers cancellation before releasing it.
    fn claim(
        &self,
        request: &DispatchRequest,
        register: &mut dyn FnMut() -> Result<()>,
    ) -> Result<UtcTimestamp>;
    /// Revalidate immediately before user-code start. Keep the owned lease
    /// until Started, or until a failed launch has been reaped. Reservation and
    /// approval alone never confer the right to start under an obsolete mode.
    fn begin_launch<'a>(&'a self, request: &'a DispatchRequest) -> LaunchFuture<'a>;
    fn started(&self, request: &DispatchRequest, process_id: u32, at: UtcTimestamp) -> Result<()>;
    fn progress(&self, request: &DispatchRequest, update: &RunnerProgress) -> Result<()>;
    fn diagnostic(&self, request: &DispatchRequest, reason: &str) -> Result<()>;
    /// Only variable names are observable; filtered values never leave the launcher.
    fn filtered_environment_names(
        &self,
        _request: &DispatchRequest,
        _names: &[String],
    ) -> Result<()> {
        Ok(())
    }
    /// One bounded launch sub-stage, as the backend measured it.
    ///
    /// The default is a no-op, so a port that does not record timings is not forced to
    /// pretend it does. A caller reporting a stage must ignore the write's own failure:
    /// a journal that cannot record a measurement is a reason to lose the measurement,
    /// never a reason to abort a launch the user asked for.
    fn launch_stage(&self, _request: &DispatchRequest, _stage: LaunchStage) -> Result<()> {
        Ok(())
    }
    /// The bounded workload counts one launch observed, recorded once per launch.
    ///
    /// The stage timings say where the seconds went; these counts say why. Same contract
    /// as `launch_stage`: a port that records nothing is not forced to pretend otherwise,
    /// and a caller must ignore the write's failure rather than fail a launch over a
    /// measurement. Counts are per launch and are never carried over from a previous call.
    fn launch_work(
        &self,
        _request: &DispatchRequest,
        _work: crate::work::LaunchWorkV1,
    ) -> Result<()> {
        Ok(())
    }
    fn background(&self, request: &DispatchRequest, update: &RunnerBackgroundUpdate) -> Result<()>;
    fn runtime_stopped(
        &self,
        request: &DispatchRequest,
        stopped: bool,
        restored: bool,
    ) -> Result<TerminationEvidence>;
    fn complete(
        &self,
        request: &DispatchRequest,
        observation: Option<&RunnerObservation>,
        tree_stopped: bool,
        cleanup_complete: bool,
        not_started: Option<NotStarted>,
        failure_code: Option<&str>,
    ) -> Result<TerminationEvidence>;
    fn reconcile(&self, attempt: AttemptRef) -> Result<ReconcileResult>;
}

/// The typed form of [`WindowsUserCorePort::section_erased`].
///
/// It is an extension trait rather than a method on the port so the port stays object safe:
/// a generic method would make `Arc<dyn WindowsUserCorePort>` impossible. The downcast can
/// only fail if a port returns a different type than the work it was handed, which is an
/// internal invariant of this seam and is reported as a typed error rather than a panic.
pub trait WindowsUserCorePortSection {
    fn section<T: Send + 'static>(
        &self,
        section: PortSection,
        work: Box<dyn FnOnce() -> Result<T> + Send + 'static>,
    ) -> PortFuture<'_, T>;
}

impl<P: WindowsUserCorePort + ?Sized> WindowsUserCorePortSection for P {
    fn section<T: Send + 'static>(
        &self,
        section: PortSection,
        work: Box<dyn FnOnce() -> Result<T> + Send + 'static>,
    ) -> PortFuture<'_, T> {
        Box::pin(async move {
            let erased = self
                .section_erased(
                    section,
                    Box::new(move || {
                        work().map(|value| Box::new(value) as Box<dyn std::any::Any + Send>)
                    }),
                )
                .await?;
            erased
                .downcast::<T>()
                .map(|value| *value)
                .map_err(|_| anyhow::anyhow!("port.section_type_mismatch"))
        })
    }
}

/// The one bounded fact a failed launch may report about why it failed.
///
/// Accepted codes are the launch path's own dot-namespaced identifiers
/// (`runner.launch_window_expired`, `command.mode_changed_before_start`, …):
/// ASCII alphanumerics plus `. _ : -`, at most 128 bytes — the same shape the
/// Runner's own rejection reasons are held to. Anything else (OS error text,
/// serde details, anything that could carry a path, SDDL, nonce or script
/// fragment) collapses to `None`; the stage events and the three independent
/// receipt facts still say where the launch stopped.
pub(crate) fn bounded_failure_code(error: &anyhow::Error) -> Option<String> {
    error.chain().map(|cause| cause.to_string()).find(|message| {
        !message.is_empty()
            && message.len() <= 128
            && message.bytes().all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
    })
}

pub(crate) struct Active {
    pub handle: ExecutionHandle,
    pub session: SessionId,
    pub cancel: watch::Sender<bool>,
    pub done: watch::Receiver<Option<TerminationEvidence>>,
}
pub(crate) type ActiveMap = Arc<Mutex<BTreeMap<String, Active>>>;

pub struct WindowsUserBackend {
    pub(crate) installation: Arc<super::installation::InstallationBinding>,
    pub(crate) mode: PermissionMode,
    runtime: RuntimeInstanceId,
    pub(crate) port: Arc<dyn WindowsUserCorePort>,
    pub(crate) gate: Arc<Semaphore>,
    pub(crate) active: ActiveMap,
}
impl WindowsUserBackend {
    /// The live scope facts, read through the port's bounded owner.
    ///
    /// `scope` takes the persistence authority and reads the workspace projection, the
    /// workspace row and the session permission state, so it is a synchronous section and
    /// must not run on the reactor worker polling a backend future.
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

    pub async fn stop_operation(&self, id: aura_domain_common::SecurityOperationId) -> Result<()> {
        let handle = self
            .active
            .lock()
            .map_err(|_| anyhow::anyhow!("runner.active_lock"))?
            .get(&id.to_string())
            .map(|v| v.handle.clone());
        if let Some(handle) = handle {
            let receipt = self.cancel(handle).await.map_err(|e| anyhow::anyhow!("{e:?}"))?;
            ensure!(
                matches!(receipt.termination, TerminationEvidence::Confirmed { .. }),
                "runner.stop_unconfirmed"
            );
        }
        Ok(())
    }
    pub fn shutdown(&self) -> Result<()> {
        self.installation.resources.shutdown()
    }
    pub fn permission_mode(&self) -> PermissionMode {
        self.mode
    }
    /// A binding problem that must stop preparation before a runner starts.
    /// An installation that is not published yet is left to the existing setup path.
    ///
    /// The answer is stable whether the identity mismatches or cannot be read at
    /// all: both mean this process is not the Core the installation binds, and
    /// both are reported through [`aura_execution_contract::classify_runner_code`]
    /// as an identity boundary rather than as a generic backend failure.
    fn core_binding_problem(&self) -> Option<String> {
        let installed = self.installation.advertised().ok()?;
        match self.installation.core_binding_reject(
            &installed.fingerprint,
            &installed.manifest.core_image,
            &installed.manifest.owner_sid,
        ) {
            Ok(reject) => reject,
            Err(_) => Some("runner.core_binding_unreadable".into()),
        }
    }
    /// Whether this backend's own readiness is an identity boundary.
    ///
    /// Callers use this instead of matching reason prefixes themselves, so the
    /// selector, preparation and the readiness card cannot disagree about whether
    /// an unreadable Core identity blocks every ordinary backend.
    ///
    /// Only a repair answer can be an identity boundary. A not-yet-published
    /// installation is missing Setup, not a mismatched identity, and the
    /// classified reason decides which of the two binding facts it is — a
    /// mismatch or an identity that could not be read at all.
    #[must_use]
    pub fn descriptor_is_identity_boundary(descriptor: &BackendDescriptor) -> bool {
        matches!(
            descriptor.readiness,
            BackendReadiness::RepairRequired { ref reason }
                if aura_execution_contract::classify_runner_code(reason)
                    .kind
                    .is_identity_boundary()
        )
    }

    /// The exact readiness reason of a descriptor, if it is not Ready.
    #[must_use]
    pub fn descriptor_readiness_reason(descriptor: &BackendDescriptor) -> Option<&str> {
        match &descriptor.readiness {
            BackendReadiness::Ready => None,
            BackendReadiness::SetupRequired { reason }
            | BackendReadiness::RepairRequired { reason }
            | BackendReadiness::Unavailable { reason } => Some(reason.as_str()),
        }
    }
    /// Stable registration identity. This must remain free of installation or
    /// host-resource I/O so Core can publish its pipe before Backend readiness
    /// is inspected.
    pub fn backend_id(&self) -> BackendId {
        BackendId(
            match self.mode {
                PermissionMode::ReadOnly => "windows-user-readonly-v1",
                PermissionMode::WorkspaceReadWrite => "windows-user-workspace-v1",
                PermissionMode::FullAccess => "windows-user-full-v1",
            }
            .into(),
        )
    }
    /// One shared gate covers both accounts: concurrent ACL leases on the same
    /// files must not overwrite each other's changes. Scheduling remains Core's.
    pub fn new(
        installation: Arc<super::installation::InstallationBinding>,
        mode: PermissionMode,
        runtime: RuntimeInstanceId,
        port: Arc<dyn WindowsUserCorePort>,
        gate: Arc<Semaphore>,
    ) -> Self {
        Self { installation, mode, runtime, port, gate, active: Default::default() }
    }
    fn installed(&self) -> Result<Arc<super::installation::Installation>> {
        Self::bind_account(self.installation.current()?, self.mode)
    }

    fn advertised_installation(&self) -> Result<Arc<super::installation::Installation>> {
        Self::bind_account(self.installation.advertised()?, self.mode)
    }

    fn bind_account(
        installation: Arc<super::installation::Installation>,
        mode: PermissionMode,
    ) -> Result<Arc<super::installation::Installation>> {
        installation.account(mode == PermissionMode::FullAccess)?;
        Ok(installation)
    }
    fn preparation(
        &self,
        operation: &SecuredOperation,
        installation: &super::installation::Installation,
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
            "runner.action_kind"
        );
        ensure!(
            operation.content.credentials.is_empty(),
            "runner.credential_use_requires_core_binding"
        );
        let expected_network = if self.mode == PermissionMode::FullAccess {
            OperationNetwork::BroadProcess
        } else {
            OperationNetwork::None
        };
        ensure!(operation.content.network == expected_network, "runner.network_mode");
        ensure!(
            facts.mode == self.mode && facts.revision > 0 && !facts.read_roots.is_empty(),
            "runner.mode_changed"
        );
        ensure!(
            self.mode != PermissionMode::ReadOnly || facts.write_roots.is_empty(),
            "runner.readonly_scope"
        );
        let (working_directory, working) =
            super::file_binding::capture(std::path::Path::new(&command.working_directory), false)?;
        ensure!(working.info().is_directory, "runner.working_directory_type");
        let roots =
            facts.read_roots.iter().map(super::file_binding::verify).collect::<Result<Vec<_>>>()?;
        ensure!(
            roots
                .iter()
                .any(|root| aura_windows_platform::final_path_within(root.info(), working.info())),
            "runner.working_directory_scope"
        );
        ensure!(
            facts.write_roots.iter().all(|write| facts.read_roots.contains(write)),
            "runner.write_scope"
        );
        Ok(WindowsUserPreparation {
            schema_version: ToolWireVersionV1,
            assurance_version: ASSURANCE_VERSION.into(),
            shell_environment_version: SHELL_ENVIRONMENT_VERSION.into(),
            installation_fingerprint: installation.fingerprint.clone(),
            mode: facts.mode,
            permission_revision: facts.revision,
            valid_until,
            account_sid: installation.account(self.mode == PermissionMode::FullAccess)?.sid.clone(),
            shell_image: match command.shell {
                ManagedShell::PowerShell => installation.manifest.powershell_image.clone(),
                ManagedShell::Cmd => installation.manifest.cmd_image.clone(),
            },
            working_directory,
            read_roots: facts.read_roots,
            write_roots: facts.write_roots,
        })
    }
    pub async fn cancel_session(&self, session: SessionId) -> Result<Vec<TerminationEvidence>> {
        let pending = {
            let active = self.active.lock().map_err(|_| anyhow::anyhow!("runner.active_lock"))?;
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
                done.changed().await.map_err(|_| anyhow::anyhow!("runner.termination_unknown"))?;
            }
        }
        Ok(evidence)
    }
}
impl ExecutionBackend for WindowsUserBackend {
    fn descriptor(&self) -> BackendDescriptor {
        use EnforcementGuarantee::*;
        use ResolvedCapability::*;
        let installed = self.advertised_installation();
        let online = self.mode == PermissionMode::FullAccess;
        let id = self.backend_id();
        let mut guarantees = [
            IdentityIsolation,
            AuthorizedWritesOnly,
            CredentialIsolation,
            ChildProcessControl,
            BoundedOutput,
        ]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
        if !online {
            guarantees.insert(DirectNetworkBlockedV1);
        }
        if self.mode == PermissionMode::ReadOnly {
            guarantees.insert(ReadOnlyUserFiles);
        }
        let mut capabilities = [ProcessExecute, WorkspaceFileRead]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        if self.mode != PermissionMode::ReadOnly {
            capabilities.extend([
                WorkspaceFileWrite,
                WorkspaceFileCreate,
                WorkspaceFileDelete,
                WorkspaceFileMove,
            ]);
        }
        if online {
            capabilities.extend([
                PublicNetworkRead,
                PrivateNetworkAccess,
                DataEgress,
                DownloadArtifact,
                ExternalServiceRead,
                ExternalServiceMutation,
            ]);
        }
        BackendDescriptor {
            id: id.clone(),
            revision: installed.as_ref().map_or(Revision::INITIAL, |v| v.manifest.revision),
            runtime_instance_id: self.runtime,
            class: if online {
                BackendClass::RestrictedUserOnline
            } else {
                BackendClass::RestrictedUserOffline
            },
            compatibility: [
                ExecutionCompatibility::ProcessTreeCancellation,
                ExecutionCompatibility::LongRunningProcess,
                ExecutionCompatibility::NativeWindowsToolchain,
            ]
            .into(),
            capabilities,
            guarantees,
            known_limitations: if online {
                Default::default()
            } else {
                [BackendKnownLimitation::DnscacheServiceMediatedDns].into()
            },
            readiness: match installed {
                Ok(installed) => match self.installation.core_binding_reject(
                    &installed.fingerprint,
                    &installed.manifest.core_image,
                    &installed.manifest.owner_sid,
                ) {
                    Ok(None) => BackendReadiness::Ready,
                    Ok(Some(reason)) => BackendReadiness::RepairRequired { reason },
                    Err(_) => BackendReadiness::RepairRequired {
                        reason: "runner.core_binding_unreadable".into(),
                    },
                },
                Err(_) => {
                    if self.installation.root.join("execution.json").exists() {
                        BackendReadiness::RepairRequired {
                            reason: "runner.installation_or_resources_changed".into(),
                        }
                    } else {
                        BackendReadiness::SetupRequired {
                            reason: "runner.production_installation_required".into(),
                        }
                    }
                }
            },
            protocol_fingerprint: canonical_sha256(&(
                ASSURANCE_VERSION,
                SHELL_ENVIRONMENT_VERSION,
                RUNNER_WIRE_VERSION,
                id.0.as_str(),
            ))
            .expect("literal protocol fingerprint"),
        }
    }
    fn prepare(&self, request: PrepareRequest) -> BackendFuture<'_, ExecutionPlan> {
        Box::pin(async move {
            if let Some(reason) = self.core_binding_problem() {
                return Err(BackendError::PreparationRejected { reason });
            }
            let descriptor = self.descriptor();
            if request.operation.validate().is_err()
                || !descriptor.supports_operation(&request.requirement, &request.operation)
            {
                return Err(BackendError::PreparationRejected {
                    reason: "runner.unsupported_or_unready".into(),
                });
            }
            let prepare = |facts: ScopeFacts| -> Result<_> {
                let installed = self.installed()?;
                // The plan's own bound. It says how long these captured facts may be
                // reused — not how long the launch has to start, which is decided
                // after the resources exist. A preparation that takes longer than this
                // is a preparation whose facts can no longer be trusted; it is not a
                // launch that ran out of time while doing its own work.
                let valid_until = UtcTimestamp::from_value(
                    UtcTimestamp::now().value()
                        + time::Duration::seconds(
                            i64::try_from(budgets::PREPARATION_TRUST.as_secs()).unwrap_or(i64::MAX),
                        ),
                );
                let preparation =
                    self.preparation(&request.operation, &installed, valid_until, facts)?;
                Ok(ExecutionPlan::new(PreparedExecution {
                    operation_fingerprint: request.operation.fingerprint.clone(),
                    backend: descriptor,
                    preparation: serde_json::to_value(preparation)?,
                })?)
            };
            // Boundary: the live scope read takes the persistence authority and the
            // journal inside the port, so it goes through the port's bounded owner.
            let facts =
                self.scope_section(&request.operation, "runner.scope").await.map_err(|_| {
                    BackendError::PreparationRejected {
                        reason: "runner.scope_or_installation_invalid".into(),
                    }
                })?;
            prepare(facts).map_err(|_| BackendError::PreparationRejected {
                reason: "runner.scope_or_installation_invalid".into(),
            })
        })
    }
    fn execute(&self, dispatch: AuthorizedDispatch) -> BackendFuture<'_, ExecutionHandle> {
        Box::pin(async move {
            if let Some(reason) = self.core_binding_problem() {
                return Err(BackendError::DispatchRejected { reason });
            }
            let request = dispatch.request().clone();
            // Boundary: the dispatch revalidation reads the live scope through the
            // port's bounded owner before anything else is decided.
            let scope = crate::managed_operations::prepared_scope(
                self.port.clone(),
                &request,
                "runner.scope_before_start",
            )
            .await
            .map_err(|_| BackendError::DispatchRejected {
                reason: "runner.dispatch_revalidation_failed".into(),
            })?;
            let ready = |scope: ScopeFacts| -> Result<_> {
                request.plan.validate()?;
                let prepared: WindowsUserPreparation =
                    serde_json::from_value(request.plan.prepared.preparation.clone())?;
                ensure!(
                    prepared.valid_until > UtcTimestamp::now()
                        && request.plan.prepared.backend == self.descriptor(),
                    "runner.preparation_expired"
                );
                let installed = self.installed()?;
                ensure!(
                    self.preparation(&request.operation, &installed, prepared.valid_until, scope)?
                        == prepared,
                    "runner.scope_changed"
                );
                Ok((prepared, installed))
            };
            let (prepared, installed) =
                ready(scope).map_err(|_| BackendError::DispatchRejected {
                    reason: "runner.dispatch_revalidation_failed".into(),
                })?;
            let handle = ExecutionHandle {
                attempt: request.binding.attempt.clone(),
                dispatch_fingerprint: canonical_sha256(&request.binding).map_err(|_| {
                    BackendError::DispatchRejected {
                        reason: "runner.dispatch_revalidation_failed".into(),
                    }
                })?,
            };
            let session = request.operation.content.identity.session_id.ok_or_else(|| {
                BackendError::DispatchRejected {
                    reason: "runner.dispatch_revalidation_failed".into(),
                }
            })?;
            let (cancel, cancelled) = watch::channel(false);
            let (complete, done) = watch::channel(None);
            let key = handle.attempt.operation_id.to_string();
            // Boundary: the claim - the mode barrier, the live authority, the
            // dispatch authority, the claimed Attempt and the launch registration - is one
            // indivisible section, and it runs whole on the port's bounded owner. A failed
            // registration still leaves the claimed Attempt as it was: never dispatched
            // again under another ordinal.
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
                        "runner.claim",
                        request.operation.content.identity.agent_run_id,
                    ),
                    Box::new(move || {
                        let mut register = || -> Result<()> {
                            let mut active = claim_active
                                .lock()
                                .map_err(|_| anyhow::anyhow!("runner.active_lock"))?;
                            ensure!(!active.contains_key(&claim_key), "runner.replay");
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
                    reason: "runner.dispatch_revalidation_failed".into(),
                })?;
            // The supervisor binds to installation facts, not to the open handle:
            // it never re-verifies the package, and chain regressions build the
            // same facts from a synthetic layout.
            let facts = {
                let online = self.mode == PermissionMode::FullAccess;
                let account =
                    installed.account(online).map_err(|_| BackendError::DispatchRejected {
                        reason: "runner.dispatch_revalidation_failed".into(),
                    })?;
                super::supervisor::LaunchFacts {
                    root: installed.root.clone(),
                    installation_id: installed.manifest.installation_id.clone(),
                    owner_sid: installed.manifest.owner_sid.clone(),
                    account: account.clone(),
                    runner_image: installed.manifest.runner_image.clone(),
                }
            };
            let installation = self.installation.clone();
            let port = self.port.clone();
            let gate = self.gate.clone();
            let active = self.active.clone();
            let observation =
                crate::observation::ExecutionObservation::capture(self.port.as_ref(), &request);
            tokio::spawn(async move {
                let evidence = super::supervisor::run(
                    request,
                    prepared,
                    facts,
                    expires,
                    cancelled,
                    gate,
                    port,
                    Arc::new(super::supervisor::SystemBoundary::new(installation)),
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
                            reason: "runner.cancel_binding".into(),
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
                        PortSection::read("runner.cancel_reconcile", None),
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
            // Boundary: both durable reads go through the bounded owner.
            let port = Arc::clone(&self.port);
            let read_attempt = attempt.clone();
            if let Ok(result @ ReconcileResult::Completed { .. }) = self
                .port
                .section(
                    PortSection::read("runner.reconcile", None),
                    Box::new(move || port.reconcile(read_attempt)),
                )
                .await
            {
                return Ok(result);
            }
            if let Ok(map) = self.active.lock()
                && let Some(active) = map.get(&attempt.operation_id.to_string())
            {
                ensure_attempt(&active.handle, &attempt)?;
                return Ok(ReconcileResult::Running { handle: active.handle.clone() });
            }
            let port = Arc::clone(&self.port);
            let read_attempt = attempt.clone();
            self.port
                .section(
                    PortSection::read("runner.reconcile", None),
                    Box::new(move || port.reconcile(read_attempt)),
                )
                .await
                .map_err(|_| BackendError::DispatchUnknown { attempt })
        })
    }
}
fn ensure_attempt(
    handle: &ExecutionHandle,
    attempt: &AttemptRef,
) -> std::result::Result<(), BackendError> {
    if handle.attempt != *attempt {
        return Err(BackendError::DispatchRejected { reason: "runner.attempt_mismatch".into() });
    }
    Ok(())
}
