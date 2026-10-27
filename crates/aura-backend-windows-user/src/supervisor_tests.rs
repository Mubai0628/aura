//! Chain regressions around the real supervisor control flow.
//!
//! The supervisor runs unmodified against a scripted boundary: the clock, the
//! installation/credential verification and the Runner transport stand at the
//! OS/process/clock edges, while resource allocation, the ACL grant walks, the
//! ancestor grants, the runtime restore and the Job objects are the real
//! production implementations on real temporary NTFS objects. The test doubles
//! never re-implement a supervisor: they only answer the questions the real one
//! asks at those edges.
#![allow(clippy::unwrap_used)]
#![allow(clippy::too_many_lines)]
use super::boundary::{RunnerReader, RunnerTransport, RunnerWriter, TransportFuture};
use super::*;
use crate::controller::ScopeFacts;
use crate::file_lease::FileLease;
use crate::{LaunchFuture, LaunchLease, LaunchWorkV1};
use aura_domain_common::{ArtifactId, ToolWireVersionV1, jcs::CanonicalSha256HashV1};
use aura_execution_contract::{
    AttemptRef, DispatchBinding, ExecutionPlan, PreparedExecution, ReconcileResult,
    SecuredOperation, TerminationEvidence,
};
use aura_security_contract::PermissionMode;
use aura_windows_platform::{ExecutionMetadataAccess, ProcessIdentity, RootJob, SecretBytes};
use aura_windows_user_contract::{
    ExecutionAccount, RunnerObservation, RunnerProgress, RunnerStopReason,
};
use std::{
    collections::{BTreeMap, VecDeque},
    num::NonZeroU32,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

// ---------------------------------------------------------------------------
// Test doubles at the OS/process/clock edges.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CompleteRecord {
    pub(crate) observation: Option<RunnerObservation>,
    pub(crate) tree_stopped: bool,
    pub(crate) cleanup: bool,
    pub(crate) not_started: Option<NotStarted>,
    pub(crate) failure_code: Option<String>,
}

#[derive(Default)]
struct PortState {
    stages: Vec<LaunchStage>,
    work: Vec<LaunchWorkV1>,
    started: Vec<u32>,
    completes: Vec<CompleteRecord>,
    runtime_stops: Vec<(bool, bool)>,
    diagnostics: Vec<String>,
    begin_launch_calls: usize,
    begin_launch_gate: Option<Arc<Notify>>,
    begin_launch_allowed: bool,
    begin_launch_error: Option<String>,
}

/// The Core port with recording: every fact the supervisor publishes lands in
/// an ordered event list the test asserts on. `begin_launch` can be gated or
/// failed to model a revoked or tightened authority.
pub(crate) struct RecordingPort {
    state: Mutex<PortState>,
    scope_facts: ScopeFacts,
}

impl RecordingPort {
    fn new(scope_facts: ScopeFacts) -> Self {
        Self { state: Mutex::new(PortState::default()), scope_facts }
    }

    pub(crate) fn configure_begin_launch(&self, gate: Option<Arc<Notify>>, error: Option<String>) {
        let mut state = self.state.lock().unwrap();
        state.begin_launch_gate = gate;
        state.begin_launch_error = error;
    }

    pub(crate) fn allow_begin_launch(&self) {
        self.state.lock().unwrap().begin_launch_allowed = true;
    }

    fn stages(&self) -> Vec<LaunchStageKind> {
        self.state.lock().unwrap().stages.iter().map(|stage| stage.stage).collect()
    }

    pub(crate) fn work_records(&self) -> Vec<LaunchWorkV1> {
        self.state.lock().unwrap().work.clone()
    }

    pub(crate) fn completes(&self) -> Vec<CompleteRecord> {
        self.state.lock().unwrap().completes.clone()
    }

    pub(crate) fn begin_launch_calls(&self) -> usize {
        self.state.lock().unwrap().begin_launch_calls
    }
}

impl WindowsUserCorePort for RecordingPort {
    fn scope(&self, _: &SecuredOperation) -> Result<ScopeFacts> {
        Ok(self.scope_facts.clone())
    }

    fn claim(
        &self,
        _: &DispatchRequest,
        _: &mut dyn FnMut() -> Result<()>,
    ) -> Result<UtcTimestamp> {
        unreachable!("the chain drives supervisor::run directly; claim belongs to the controller")
    }

    fn begin_launch<'a>(&'a self, _: &'a DispatchRequest) -> LaunchFuture<'a> {
        Box::pin(async move {
            let gate = {
                let mut state = self.state.lock().unwrap();
                state.begin_launch_calls += 1;
                state.begin_launch_gate.clone()
            };
            if let Some(gate) = gate {
                loop {
                    if self.state.lock().unwrap().begin_launch_allowed {
                        break;
                    }
                    gate.notified().await;
                }
            }
            let error = self.state.lock().unwrap().begin_launch_error.clone();
            if let Some(error) = error {
                return Err(anyhow::anyhow!(error));
            }
            Ok(Box::new(()) as LaunchLease)
        })
    }

    fn started(&self, _: &DispatchRequest, process_id: u32, _: UtcTimestamp) -> Result<()> {
        self.state.lock().unwrap().started.push(process_id);
        Ok(())
    }

    fn progress(&self, _: &DispatchRequest, _: &RunnerProgress) -> Result<()> {
        Ok(())
    }

    fn diagnostic(&self, _: &DispatchRequest, reason: &str) -> Result<()> {
        self.state.lock().unwrap().diagnostics.push(reason.to_owned());
        Ok(())
    }

    fn launch_stage(&self, _: &DispatchRequest, stage: LaunchStage) -> Result<()> {
        self.state.lock().unwrap().stages.push(stage);
        Ok(())
    }

    fn launch_work(&self, _: &DispatchRequest, work: LaunchWorkV1) -> Result<()> {
        self.state.lock().unwrap().work.push(work);
        Ok(())
    }

    fn background(&self, _: &DispatchRequest, _: &RunnerBackgroundUpdate) -> Result<()> {
        Ok(())
    }

    fn runtime_stopped(
        &self,
        _: &DispatchRequest,
        stopped: bool,
        restored: bool,
    ) -> Result<TerminationEvidence> {
        self.state.lock().unwrap().runtime_stops.push((stopped, restored));
        Ok(TerminationEvidence::Confirmed { evidence_ref: ArtifactId::new() })
    }

    fn complete(
        &self,
        _: &DispatchRequest,
        observation: Option<&RunnerObservation>,
        tree_stopped: bool,
        cleanup_complete: bool,
        not_started: Option<NotStarted>,
        failure_code: Option<&str>,
    ) -> Result<TerminationEvidence> {
        self.state.lock().unwrap().completes.push(CompleteRecord {
            observation: observation.cloned(),
            tree_stopped,
            cleanup: cleanup_complete,
            not_started,
            failure_code: failure_code.map(str::to_owned),
        });
        Ok(TerminationEvidence::Confirmed { evidence_ref: ArtifactId::new() })
    }

    fn reconcile(&self, _: aura_execution_contract::AttemptRef) -> Result<ReconcileResult> {
        unreachable!("the chain drives supervisor::run directly")
    }
}

/// Scripted Runner state shared between the boundary (which hands transports
/// out) and the test (which plays the Runner's side of the wire).
pub(crate) struct ScriptedRunner {
    state: Mutex<RunnerState>,
    notify: Notify,
    identity: ProcessIdentity,
}

struct RunnerState {
    /// Frames the Runner sends to the supervisor.
    outbound: VecDeque<RunnerCoreMessage>,
    /// Frames the supervisor wrote to the Runner.
    inbound: Vec<CoreRunnerMessage>,
    connect_open: bool,
    on_ready: Option<Arc<dyn Fn() + Send + Sync>>,
    on_execute: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl ScriptedRunner {
    pub(crate) fn send(&self, message: RunnerCoreMessage) {
        self.state.lock().unwrap().outbound.push_back(message);
        self.notify.notify_waiters();
    }

    pub(crate) fn execute_count(&self) -> usize {
        self.state
            .lock()
            .unwrap()
            .inbound
            .iter()
            .filter(|message| matches!(message, CoreRunnerMessage::Execute { .. }))
            .count()
    }

    pub(crate) fn cancel_count(&self) -> usize {
        self.state
            .lock()
            .unwrap()
            .inbound
            .iter()
            .filter(|message| matches!(message, CoreRunnerMessage::Cancel { .. }))
            .count()
    }

    fn pop_next(&self) -> Option<RunnerCoreMessage> {
        self.state.lock().unwrap().outbound.pop_front()
    }

    async fn wait_pop(self: &Arc<Self>) -> RunnerCoreMessage {
        loop {
            if let Some(message) = self.pop_next() {
                return message;
            }
            self.notify.notified().await;
        }
    }
}

struct ScriptedTransport {
    runner: Arc<ScriptedRunner>,
}

impl RunnerTransport for ScriptedTransport {
    fn identity(&self) -> Result<ProcessIdentity> {
        // The real current-process identity: the supervisor's image binding and
        // account checks run for real against this test process.
        Ok(self.runner.identity.clone())
    }

    fn connect<'a>(&'a mut self) -> TransportFuture<'a, ()> {
        Box::pin(async move {
            loop {
                if self.runner.state.lock().unwrap().connect_open {
                    return Ok(());
                }
                self.runner.notify.notified().await;
            }
        })
    }

    fn authenticate_client(&self) -> Result<()> {
        Ok(())
    }

    fn read_ready<'a>(&'a mut self) -> TransportFuture<'a, RunnerCoreMessage> {
        Box::pin(async move {
            let message = self.runner.wait_pop().await;
            if matches!(message, RunnerCoreMessage::Ready)
                && let Some(hook) = self.runner.state.lock().unwrap().on_ready.clone()
            {
                hook();
            }
            Ok(message)
        })
    }

    fn split_for_execute(self: Box<Self>) -> (Box<dyn RunnerReader>, Box<dyn RunnerWriter>) {
        (
            Box::new(ScriptedReader { runner: self.runner.clone() }),
            Box::new(ScriptedWriter { runner: self.runner }),
        )
    }
}

struct ScriptedReader {
    runner: Arc<ScriptedRunner>,
}

impl RunnerReader for ScriptedReader {
    fn read<'a>(&'a mut self) -> TransportFuture<'a, RunnerCoreMessage> {
        Box::pin(async move { Ok(self.runner.wait_pop().await) })
    }
}

struct ScriptedWriter {
    runner: Arc<ScriptedRunner>,
}

impl RunnerWriter for ScriptedWriter {
    fn write<'a>(&'a mut self, message: &'a CoreRunnerMessage) -> TransportFuture<'a, ()> {
        Box::pin(async move {
            let hook = {
                let mut state = self.runner.state.lock().unwrap();
                state.inbound.push(message.clone());
                match message {
                    CoreRunnerMessage::Execute { .. } => state.on_execute.clone(),
                    CoreRunnerMessage::Cancel { .. } => None,
                }
            };
            if let Some(hook) = hook {
                hook();
            }
            Ok(())
        })
    }
}

/// The real runtime resource owner plus a shifted wall clock; everything else
/// at the boundary answers with test doubles.
struct ScriptedBoundary {
    clock_offset_ms: AtomicI64,
    advance_on_allocate_ms: AtomicI64,
    resources: crate::runtime::RuntimeResources,
    runner: Arc<ScriptedRunner>,
}

impl LaunchBoundary for ScriptedBoundary {
    fn now(&self) -> UtcTimestamp {
        UtcTimestamp::from_value(
            UtcTimestamp::now().value()
                + time::Duration::milliseconds(self.clock_offset_ms.load(Ordering::Acquire)),
        )
    }

    fn verify_installation(
        &self,
        _: &LaunchFacts,
        _: bool,
        _: &mut LaunchWorkV1,
    ) -> Result<SecretBytes> {
        Ok(SecretBytes::new(b"synthetic-test-secret".to_vec()))
    }

    fn runtime_reused(&self) -> bool {
        self.resources.is_open()
    }

    fn take_recovery_work(&self) -> Option<crate::work::RecoveryWork> {
        self.resources.take_recovery_work()
    }

    fn resource_counts(&self) -> crate::runtime::ResourceCounts {
        self.resources.resource_counts()
    }

    fn allocate_resources(
        &self,
        facts: &LaunchFacts,
        nonce: &str,
        account_sid: &str,
        read_roots: &[ExecutionFileBinding],
        prepare: &mut dyn FnMut(&mut FileLease, &mut Option<ExecutionFileBinding>) -> Result<()>,
    ) -> Result<Arc<RootJob>> {
        let job = self.resources.allocate(facts, nonce, account_sid, read_roots, prepare)?;
        // Slow-preparation simulation: authority time advances while the real
        // grant work runs, so no test waits through a real forty seconds.
        let advance = self.advance_on_allocate_ms.swap(0, Ordering::AcqRel);
        if advance > 0 {
            self.clock_offset_ms.fetch_add(advance, Ordering::AcqRel);
        }
        Ok(job)
    }

    fn prepare_workspace(
        &self,
        nonce: &str,
        prepare: &mut dyn FnMut(&mut FileLease) -> Result<()>,
    ) -> Result<()> {
        self.resources.prepare_workspace(nonce, prepare)
    }

    fn stop_resources(&self, nonce: &str) -> Result<bool> {
        self.resources.stopped(nonce)
    }

    fn runner_metadata_access(
        &self,
        identity: &ProcessIdentity,
        account_sid: &str,
    ) -> Result<ExecutionMetadataAccess> {
        // The real Authz context against this test process's token.
        Ok(ExecutionMetadataAccess::for_runner(identity, account_sid)?)
    }

    fn launch_runner(&self, _: &RunnerLaunch<'_>) -> Result<Box<dyn RunnerTransport>> {
        Ok(Box::new(ScriptedTransport { runner: self.runner.clone() }))
    }
}

// ---------------------------------------------------------------------------
// The rig: a synthetic installation layout around the real runtime.
// ---------------------------------------------------------------------------

pub(crate) struct Rig {
    _temp: tempfile::TempDir,
    workspace: std::path::PathBuf,
    install_root: std::path::PathBuf,
    facts: LaunchFacts,
    prepared: WindowsUserPreparation,
    pub(crate) request: DispatchRequest,
    boundary: Arc<ScriptedBoundary>,
    pub(crate) runner: Arc<ScriptedRunner>,
    pub(crate) port: Arc<RecordingPort>,
    gate: Arc<Semaphore>,
    pub(crate) cancelled: watch::Sender<bool>,
}

fn protect_private_dir(path: &std::path::Path, owner_sid: &str) {
    let object = aura_windows_platform::ExecutionFileSecurity::open(path, true).unwrap();
    let before = object.capture().unwrap();
    let protected = format!("D:PAI(A;OICI;FA;;;{owner_sid})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)");
    object.apply_exact(&before, &protected).unwrap();
    aura_windows_platform::verify_execution_installation_acl(path, owner_sid, true).unwrap();
}

impl Rig {
    pub(crate) fn new(files: usize) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        for n in 0..files {
            std::fs::write(workspace.join(format!("{n:04}.txt")), b"content").unwrap();
        }
        let install_root = temp.path().join("install");
        let leases = install_root.join("private/execution-leases");
        std::fs::create_dir_all(&leases).unwrap();
        std::fs::create_dir(install_root.join("execution-scratch")).unwrap();
        std::fs::create_dir(install_root.join("runtime")).unwrap();
        std::fs::write(install_root.join("private/execution-setup.lock"), []).unwrap();
        std::fs::write(install_root.join("private/execution.lock"), []).unwrap();
        let (owner_sid, _) = aura_windows_platform::current_user_token_identity().unwrap();
        protect_private_dir(&install_root.join("private"), &owner_sid);
        protect_private_dir(&leases, &owner_sid);

        let (workspace_binding, _) = crate::file_binding::capture(&workspace, false).unwrap();
        let (runner_image, _) =
            crate::file_binding::capture(&std::env::current_exe().unwrap(), true).unwrap();
        let facts = LaunchFacts {
            root: install_root.clone(),
            installation_id: format!(
                "b3f00000-0000-4000-8000-{:012x}",
                std::process::id() as u64 ^ 0x5a5a_0000_0000u64
            ),
            owner_sid: owner_sid.clone(),
            account: ExecutionAccount {
                name: "aura-chain-test".into(),
                sid: owner_sid.clone(),
                credential_sha256: aura_domain_common::ContentHash::sha256(b"synthetic-credential"),
            },
            runner_image,
        };
        let prepared = WindowsUserPreparation {
            schema_version: ToolWireVersionV1,
            assurance_version: ASSURANCE_VERSION.into(),
            shell_environment_version: SHELL_ENVIRONMENT_VERSION.into(),
            installation_fingerprint: CanonicalSha256HashV1::sha256(b"chain-test-installation"),
            mode: PermissionMode::WorkspaceReadWrite,
            permission_revision: 1,
            valid_until: UtcTimestamp::from_value(
                UtcTimestamp::now().value() + time::Duration::seconds(3600),
            ),
            account_sid: owner_sid.clone(),
            shell_image: facts.runner_image.clone(),
            working_directory: workspace_binding.clone(),
            read_roots: vec![workspace_binding.clone()],
            write_roots: vec![workspace_binding.clone()],
        };
        let request = build_request(&workspace, &prepared);
        let scope_facts = ScopeFacts {
            mode: prepared.mode,
            revision: prepared.permission_revision,
            read_roots: prepared.read_roots.clone(),
            write_roots: prepared.write_roots.clone(),
        };
        let runner = Arc::new(ScriptedRunner {
            state: Mutex::new(RunnerState {
                outbound: VecDeque::new(),
                inbound: Vec::new(),
                connect_open: true,
                on_ready: None,
                on_execute: None,
            }),
            notify: Notify::new(),
            identity: aura_windows_platform::snapshot_process(std::process::id()).unwrap(),
        });
        let (cancelled, _) = watch::channel(false);
        Self {
            _temp: temp,
            workspace,
            install_root,
            facts,
            prepared,
            request,
            boundary: Arc::new(ScriptedBoundary {
                clock_offset_ms: AtomicI64::new(0),
                advance_on_allocate_ms: AtomicI64::new(0),
                resources: crate::runtime::RuntimeResources::default(),
                runner: runner.clone(),
            }),
            runner,
            port: Arc::new(RecordingPort::new(scope_facts)),
            gate: Arc::new(Semaphore::new(1)),
            cancelled,
        }
    }

    pub(crate) fn spawn_run(
        &self,
        expiry_in_seconds: i64,
    ) -> tokio::task::JoinHandle<Result<TerminationEvidence>> {
        let expiry = UtcTimestamp::from_value(
            UtcTimestamp::now().value() + time::Duration::seconds(expiry_in_seconds),
        );
        tokio::spawn(run(
            self.request.clone(),
            self.prepared.clone(),
            self.facts.clone(),
            expiry,
            self.cancelled.subscribe(),
            self.gate.clone(),
            self.port.clone(),
            self.boundary.clone(),
            crate::observation::ExecutionObservation::capture(self.port.as_ref(), &self.request),
        ))
    }

    fn journal_text(&self) -> String {
        let entries = std::fs::read_dir(self.install_root.join("private/execution-leases"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 1, "one lease journal per launch");
        std::fs::read_to_string(&entries[0]).unwrap()
    }

    pub(crate) fn snapshot_acls(&self) -> BTreeMap<String, String> {
        let mut snapshot = BTreeMap::new();
        let root_acl = aura_windows_platform::ExecutionFileSecurity::open(&self.workspace, false)
            .unwrap()
            .capture()
            .unwrap();
        snapshot.insert("<root>".into(), root_acl);
        for entry in std::fs::read_dir(&self.workspace).unwrap() {
            let path = entry.unwrap().path();
            let acl = aura_windows_platform::ExecutionFileSecurity::open(&path, false)
                .unwrap()
                .capture()
                .unwrap();
            snapshot.insert(path.file_name().unwrap().to_string_lossy().into_owned(), acl);
        }
        snapshot
    }

    pub(crate) fn assert_acls_restored(&self, before: &BTreeMap<String, String>) {
        let after = self.snapshot_acls();
        assert_eq!(
            after.keys().collect::<Vec<_>>(),
            before.keys().collect::<Vec<_>>(),
            "the same objects exist after the launch"
        );
        for (name, acl) in &after {
            assert!(
                aura_windows_platform::ExecutionFileSecurity::sddl_equivalent(acl, &before[name]),
                "{name}: every grant the launch wrote must be restored, still {acl}"
            );
        }
    }

    /// The paths this lease journaled as intents must return to their exact
    /// captured before-state. Intents under the attempt's own scratch tree are
    /// exempt: that tree is attempt output, so a settling lease removes it
    /// instead of restoring it.
    fn assert_captured_acls_restored(&self) {
        let journal = self.journal_text();
        let scratch = self.install_root.join("execution-scratch");
        let mut checked = 0;
        for line in journal.lines() {
            let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if record["entry"] != serde_json::json!("intent") {
                continue;
            }
            let text = record["path"].as_str().expect("intent path");
            // Journaled paths are final paths, which carry the verbatim prefix.
            let path = Path::new(text.strip_prefix(r"\\?\").unwrap_or(text));
            if path.starts_with(&scratch) {
                continue;
            }
            let acl = aura_windows_platform::ExecutionFileSecurity::open(path, false)
                .unwrap()
                .capture()
                .unwrap();
            let captured = record["before"].as_str().expect("captured before ACL");
            assert!(
                aura_windows_platform::ExecutionFileSecurity::sddl_equivalent(&acl, captured),
                "{}: the journaled object must return to its captured ACL, now {acl}",
                path.display()
            );
            checked += 1;
        }
        assert!(checked > 0, "the lease journaled at least one intent");
    }

    /// The per-attempt write SID is the artifact only this launch could leave
    /// behind; no object may retain it after restore.
    fn assert_no_write_grant_remains(&self) {
        let journal = self.journal_text();
        let write_sid = journal
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .find_map(|record| {
                (record["entry"] == serde_json::json!("scope"))
                    .then(|| record["write_sid"].as_str().map(str::to_owned))
                    .flatten()
            })
            .expect("a scope records the write SID");
        for entry in std::fs::read_dir(&self.workspace).unwrap() {
            let path = entry.unwrap().path();
            let acl = aura_windows_platform::ExecutionFileSecurity::open(&path, false)
                .unwrap()
                .capture()
                .unwrap();
            assert!(!acl.contains(&write_sid), "{}: write grant remains", path.display());
        }
    }

    fn advance_clock_on_allocate(&self, milliseconds: i64) {
        self.boundary.advance_on_allocate_ms.store(milliseconds, Ordering::Release);
    }

    /// Shift the scripted clock; deadline races advance it while a boundary wait is pending.
    pub(crate) fn advance_clock_ms(&self, milliseconds: i64) {
        self.boundary.clock_offset_ms.fetch_add(milliseconds, Ordering::AcqRel);
    }

    fn set_on_ready(&self, hook: Arc<dyn Fn() + Send + Sync>) {
        self.runner.state.lock().unwrap().on_ready = Some(hook);
    }

    fn set_on_execute(&self, hook: Arc<dyn Fn() + Send + Sync>) {
        self.runner.state.lock().unwrap().on_execute = Some(hook);
    }
}

fn build_request(workspace: &Path, prepared: &WindowsUserPreparation) -> DispatchRequest {
    let fixture = aura_contract_schema::v2_kernel_fixture().unwrap();
    let mut content = fixture.dispatch.operation.content;
    content.network = aura_execution_contract::OperationNetwork::None;
    content.credentials.clear();
    content.capabilities = vec![
        aura_security_contract::ResolvedCapability::ProcessExecute,
        aura_security_contract::ResolvedCapability::WorkspaceFileRead,
        aura_security_contract::ResolvedCapability::WorkspaceFileWrite,
    ];
    content.action_kind = "shell.powershell".into();
    content.action = serde_json::to_value(ManagedShellCommand {
        shell: ManagedShell::PowerShell,
        script: "Set-Content -Path marker.txt -Value 'value'".into(),
        working_directory: workspace.to_string_lossy().into_owned(),
        stdin: String::new(),
        timeout_ms: 60_000,
        max_output_bytes: 64 * 1024,
        secret_environment: Vec::new(),
    })
    .unwrap();
    let operation = SecuredOperation::new(aura_domain_common::SecurityOperationId::new(), content)
        .expect("operation");
    let plan = ExecutionPlan::new(PreparedExecution {
        operation_fingerprint: operation.fingerprint.clone(),
        backend: fixture.dispatch.plan.prepared.backend,
        preparation: serde_json::to_value(prepared).unwrap(),
    })
    .expect("plan");
    let operation_id = operation.operation_id;
    let operation_fingerprint = operation.fingerprint.clone();
    let plan_fingerprint = plan.fingerprint.clone();
    DispatchRequest {
        binding: DispatchBinding {
            attempt: AttemptRef { operation_id, ordinal: NonZeroU32::new(1).unwrap() },
            operation_fingerprint,
            authorization_fingerprint: CanonicalSha256HashV1::sha256(b"chain-test-authority"),
            plan_fingerprint,
            cancellation_revision: aura_domain_common::Revision::INITIAL,
        },
        operation,
        plan,
    }
}

pub(crate) async fn wait_until(what: &str, condition: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !condition() {
        assert!(std::time::Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

pub(crate) fn finished_observation(rig: &Rig, process_id: u32) -> RunnerObservation {
    let started_at = UtcTimestamp::now();
    RunnerObservation {
        attempt: rig.request.binding.attempt.clone(),
        binding_fingerprint: aura_domain_common::jcs::canonical_sha256(&rig.request.binding)
            .unwrap(),
        account_sid: rig.prepared.account_sid.clone(),
        process_id,
        shell_image: rig.prepared.shell_image.clone(),
        started_at,
        finished_at: UtcTimestamp::from_value(started_at.value() + time::Duration::milliseconds(5)),
        exit_code: Some(0),
        stop_reason: RunnerStopReason::Exited,
        process_tree_stopped: true,
        stdout: b"value\n".to_vec(),
        stderr: Vec::new(),
        submitted_script: String::new(),
        output_truncated: false,
    }
}

/// Drive one honest launch to completion: Ready -> Started -> Completed, with
/// the Runner side played by the test.
pub(crate) async fn drive_to_execute(rig: &Rig) {
    wait_until("pipe connect", || rig.port.stages().contains(&LaunchStageKind::PipeConnected))
        .await;
    rig.runner.send(RunnerCoreMessage::Ready);
    wait_until("execute frame", || rig.runner.execute_count() == 1).await;
}

// ---------------------------------------------------------------------------
// The matrix.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slow_preparation_still_executes_exactly_once() {
    let rig = Rig::new(8);
    rig.advance_clock_on_allocate(40_000);
    let task = rig.spawn_run(300);
    drive_to_execute(&rig).await;
    let process_id = std::process::id();
    rig.runner.send(RunnerCoreMessage::Started {
        attempt: rig.request.binding.attempt.clone(),
        process_id,
    });
    rig.runner.send(RunnerCoreMessage::Completed {
        observation: Box::new(finished_observation(&rig, process_id)),
    });
    let evidence = task.await.unwrap().expect("launch completes");
    assert!(matches!(evidence, TerminationEvidence::Confirmed { .. }));
    assert_eq!(rig.runner.execute_count(), 1, "exactly one Execute frame");
    assert_eq!(rig.port.state.lock().unwrap().started, vec![process_id]);
    assert_eq!(rig.port.begin_launch_calls(), 1);
    let completes = rig.port.completes();
    assert_eq!(completes.len(), 1);
    assert_eq!(completes[0].not_started, None);
    assert!(completes[0].tree_stopped && completes[0].cleanup);
    assert!(completes[0].observation.is_some());
    // The window is whole after a forty-second preparation: it is decided from
    // the clock at dispatch, bounded by the authority's own deadline.
    let work = &rig.port.work_records()[0];
    assert_eq!(work.launch_window_ms, 30_000, "the window is fresh, not spent by preparation");
    assert_eq!(work.roots_granted, 2, "scratch and workspace");
    assert!(work.objects_scanned >= 8);
    assert!(work.preparation_ms > 0);
    assert_eq!(rig.port.work_records().len(), 1, "exactly one workload record");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_while_queued_reports_work_and_never_executes() {
    let rig = Rig::new(4);
    let _held = rig.gate.acquire().await.unwrap();
    let task = rig.spawn_run(300);
    tokio::time::sleep(Duration::from_millis(30)).await;
    rig.cancelled.send(true).unwrap();
    task.await.unwrap().expect("queued cancel settles");
    let completes = rig.port.completes();
    assert_eq!(completes.len(), 1);
    assert_eq!(completes[0].not_started, Some(NotStarted::Cancelled));
    assert_eq!(rig.runner.execute_count(), 0);
    assert!(rig.port.stages().is_empty(), "no resource work started");
    let work = &rig.port.work_records()[0];
    assert!(work.queue_ms > 0, "the queue wait is this launch's work");
    assert_eq!(work.objects_scanned, 0);
    assert_eq!(rig.port.work_records().len(), 1, "exactly one workload record");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plan_expired_before_resource_work_reports_work_and_never_executes() {
    let mut rig = Rig::new(4);
    rig.prepared.valid_until =
        UtcTimestamp::from_value(UtcTimestamp::now().value() - time::Duration::seconds(1));
    let task = rig.spawn_run(300);
    task.await.unwrap().expect("expired plan settles");
    let completes = rig.port.completes();
    assert_eq!(completes.len(), 1);
    assert_eq!(completes[0].not_started, Some(NotStarted::Expired));
    assert_eq!(rig.runner.execute_count(), 0);
    assert!(rig.port.stages().is_empty(), "no resource work started");
    assert_eq!(rig.port.work_records().len(), 1, "exactly one workload record");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_during_enumeration_stops_walk_and_restores_real_acls() {
    let rig = Rig::new(64);
    let task = rig.spawn_run(300);
    // Cancel once four intents are durable — the same observation the
    // independent probe used against the production lease.
    let journal_dir = rig.install_root.join("private/execution-leases");
    let cancelled = rig.cancelled.clone();
    let watcher = std::thread::spawn(move || {
        let until = std::time::Instant::now() + Duration::from_secs(30);
        while std::time::Instant::now() < until {
            if let Ok(read) = std::fs::read_dir(&journal_dir) {
                for path in read.flatten().map(|entry| entry.path()) {
                    if let Ok(text) = std::fs::read_to_string(&path)
                        && text.matches("\"entry\":\"intent\"").count() >= 4
                    {
                        let _ = cancelled.send(true);
                        return;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("four intents were never journaled");
    });
    watcher.join().unwrap();
    task.await.unwrap().expect("cancelled launch settles");
    let completes = rig.port.completes();
    assert_eq!(completes.len(), 1);
    assert_eq!(completes[0].not_started, Some(NotStarted::Cancelled));
    assert_eq!(rig.runner.execute_count(), 0);
    let work = &rig.port.work_records()[0];
    assert!(
        work.objects_scanned > 0 && work.objects_scanned < 66,
        "partial walk: {}",
        work.objects_scanned
    );
    assert_eq!(
        work.acl_rewrites, 1,
        "only the scratch root finished its grant before the workspace walk stopped"
    );
    assert_eq!(rig.port.work_records().len(), 1, "exactly one workload record");
    // Captured objects return to their journaled before-state, and no object —
    // captured or not — keeps the per-attempt write grant.
    rig.assert_captured_acls_restored();
    rig.assert_no_write_grant_remains();
    // The workspace scope journaled its halt, which proves no Execute followed
    // it: the intents are restored, the attempt's own scratch tree is retired
    // and the lease settles with a clean receipt.
    let journal = rig.journal_text();
    assert!(journal.contains("\"entry\":\"intent\""));
    assert!(journal.contains("\"entry\":\"objects_restored\""));
    assert!(journal.contains("\"entry\":\"released\""), "a halted preparation settles");
    assert!(completes[0].cleanup, "a halted preparation cleans up after itself");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_during_acl_apply_stops_between_rewrites_and_restores() {
    let rig = Rig::new(256);
    let before = rig.snapshot_acls();
    let task = rig.spawn_run(300);
    // The workspace root is the first object the apply phase rewrites; the
    // grant turns the user's inherited entry into an explicit protected
    // inheritable one, and that exact form is the signal that apply started.
    let cancelled = rig.cancelled.clone();
    let account = rig.facts.account.sid.clone();
    let stop = cancelled.clone();
    let watched_root = rig.workspace.clone();
    let grant_form = format!("(A;OICI;FA;;;{account})");
    let watcher = std::thread::spawn(move || {
        let until = std::time::Instant::now() + Duration::from_secs(30);
        while std::time::Instant::now() < until {
            let acl = aura_windows_platform::ExecutionFileSecurity::open(&watched_root, false)
                .unwrap()
                .capture()
                .unwrap();
            if acl.contains(&grant_form) {
                let _ = stop.send(true);
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("the apply phase never rewrote the workspace root");
    });
    watcher.join().unwrap();
    task.await.unwrap().expect("cancelled launch settles");
    let completes = rig.port.completes();
    assert_eq!(completes.len(), 1);
    assert_eq!(completes[0].not_started, Some(NotStarted::Cancelled));
    assert_eq!(rig.runner.execute_count(), 0);
    let work = &rig.port.work_records()[0];
    assert_eq!(work.objects_scanned, 258, "scratch + workspace root + 256 files captured");
    assert!(
        work.acl_rewrites >= 1 && work.acl_rewrites < 258,
        "each rewrite before the stop is counted: {}",
        work.acl_rewrites
    );
    assert_eq!(rig.port.work_records().len(), 1, "exactly one workload record");
    rig.assert_acls_restored(&before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_at_ready_boundary_never_executes() {
    let rig = Rig::new(4);
    let before = rig.snapshot_acls();
    let task = rig.spawn_run(300);
    wait_until("runner awaits Ready", || {
        rig.port.stages().contains(&LaunchStageKind::PipeConnected)
    })
    .await;
    // Cancel while the supervisor waits at the Ready boundary, then let Ready
    // through: the ancestor grant sees the live stop and the launch ends before
    // begin_launch, before Execute.
    rig.cancelled.send(true).unwrap();
    rig.runner.send(RunnerCoreMessage::Ready);
    task.await.unwrap().expect("cancelled launch settles");
    let completes = rig.port.completes();
    assert_eq!(completes.len(), 1);
    assert_eq!(completes[0].not_started, Some(NotStarted::Cancelled));
    assert_eq!(rig.runner.execute_count(), 0);
    assert_eq!(rig.port.begin_launch_calls(), 0, "the authority was never re-read for an Execute");
    assert_eq!(rig.port.work_records().len(), 1, "exactly one workload record");
    rig.assert_acls_restored(&before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authority_revoked_at_begin_launch_never_executes() {
    let rig = Rig::new(4);
    let before = rig.snapshot_acls();
    // The authority re-read answers revoked immediately, the way the Core port
    // fails when live mode or revision no longer matches the preparation.
    rig.port.configure_begin_launch(None, Some("command.mode_changed_before_start".into()));
    let task = rig.spawn_run(300);
    drive_to_execute_until_ready(&rig).await;
    // The authority is re-read at begin_launch and now answers revoked; the
    // launch settles without Execute.
    task.await.unwrap().expect("revoked launch settles");
    let completes = rig.port.completes();
    assert_eq!(completes.len(), 1);
    assert_eq!(completes[0].not_started, Some(NotStarted::Rejected));
    // The bounded reason survives the whole chain: persisted diagnostic, and
    // the code handed to the receipt instead of an unexplained rejection.
    assert_eq!(completes[0].failure_code.as_deref(), Some("command.mode_changed_before_start"));
    assert_eq!(
        rig.port.state.lock().unwrap().diagnostics,
        vec!["command.mode_changed_before_start".to_owned()]
    );
    assert_eq!(rig.runner.execute_count(), 0);
    assert_eq!(rig.port.begin_launch_calls(), 1);
    assert_eq!(rig.port.work_records().len(), 1, "exactly one workload record");
    rig.assert_acls_restored(&before);
}

pub(crate) async fn drive_to_execute_until_ready(rig: &Rig) {
    wait_until("pipe connect", || rig.port.stages().contains(&LaunchStageKind::PipeConnected))
        .await;
    rig.runner.send(RunnerCoreMessage::Ready);
    wait_until("begin_launch attempted", || rig.port.begin_launch_calls() == 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expired_window_before_execute_never_reaches_begin_launch() {
    let rig = Rig::new(4);
    let before = rig.snapshot_acls();
    // A five-second authority: by the time Ready is read, ten more seconds of
    // authority time have passed.
    let task = rig.spawn_run(5);
    let boundary = rig.boundary.clone();
    rig.set_on_ready(Arc::new(move || {
        boundary.clock_offset_ms.fetch_add(10_000, Ordering::AcqRel);
    }));
    wait_until("pipe connect", || rig.port.stages().contains(&LaunchStageKind::PipeConnected))
        .await;
    rig.runner.send(RunnerCoreMessage::Ready);
    task.await.unwrap().expect("expired launch settles");
    let completes = rig.port.completes();
    assert_eq!(completes.len(), 1);
    assert_eq!(completes[0].not_started, Some(NotStarted::Expired));
    assert_eq!(rig.runner.execute_count(), 0);
    assert_eq!(rig.port.begin_launch_calls(), 0, "no Execute path after the window closed");
    assert_eq!(rig.port.work_records().len(), 1, "exactly one workload record");
    rig.assert_acls_restored(&before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replaced_runner_identity_is_rejected_before_execute() {
    let mut rig = Rig::new(4);
    let before = rig.snapshot_acls();
    // Someone swapped the Runner image after the manifest was captured: the
    // launched process no longer matches the bound image.
    let (other_image, _) =
        crate::file_binding::capture(Path::new(r"C:/Windows/System32/cmd.exe"), true).unwrap();
    rig.facts.runner_image = other_image;
    let task = rig.spawn_run(300);
    task.await.unwrap().expect("rejected launch settles");
    let completes = rig.port.completes();
    assert_eq!(completes.len(), 1);
    assert_eq!(completes[0].not_started, Some(NotStarted::Rejected));
    assert_eq!(rig.runner.execute_count(), 0);
    assert!(
        !rig.port.stages().contains(&LaunchStageKind::PipeConnected),
        "the identity check fires before the handshake continues"
    );
    assert_eq!(rig.port.work_records().len(), 1, "exactly one workload record");
    rig.assert_acls_restored(&before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cleanup_failure_never_reports_cleaned() {
    let rig = Rig::new(4);
    let before = rig.snapshot_acls();
    let scratch_parent = rig.install_root.join("execution-scratch");
    rig.set_on_execute(Arc::new(move || {
        // External fault: the scratch parent vanishes before settle time, so
        // the runtime cannot retire the scratch root.
        let _ = std::fs::remove_dir_all(&scratch_parent);
    }));
    let task = rig.spawn_run(300);
    drive_to_execute(&rig).await;
    let process_id = std::process::id();
    rig.runner.send(RunnerCoreMessage::Started {
        attempt: rig.request.binding.attempt.clone(),
        process_id,
    });
    rig.runner.send(RunnerCoreMessage::Completed {
        observation: Box::new(finished_observation(&rig, process_id)),
    });
    task.await.unwrap().expect("launch settles even when cleanup fails");
    let completes = rig.port.completes();
    assert_eq!(completes.len(), 1);
    assert!(!completes[0].cleanup, "a failed cleanup is a false fact when reported as clean");
    assert!(completes[0].observation.is_some());
    assert_eq!(rig.port.work_records().len(), 1, "exactly one workload record");
    rig.assert_acls_restored(&before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_during_connect_never_executes() {
    let rig = Rig::new(4);
    let before = rig.snapshot_acls();
    // The Runner never connects: the handshake wait is the controllable gate.
    rig.runner.state.lock().unwrap().connect_open = false;
    let task = rig.spawn_run(300);
    wait_until("process created", || rig.port.stages().contains(&LaunchStageKind::ProcessCreated))
        .await;
    // The identity capture hashes this test executable before connect starts;
    // give it a wide margin, then cancel while the connect wait is pending.
    tokio::time::sleep(Duration::from_secs(1)).await;
    rig.cancelled.send(true).unwrap();
    task.await.unwrap().expect("cancelled launch settles");
    let completes = rig.port.completes();
    assert_eq!(completes.len(), 1);
    assert_eq!(completes[0].not_started, Some(NotStarted::Cancelled));
    assert_eq!(rig.runner.execute_count(), 0);
    assert!(
        !rig.port.stages().contains(&LaunchStageKind::PipeConnected),
        "no connected pipe exists for this launch"
    );
    assert_eq!(rig.port.work_records().len(), 1, "exactly one workload record");
    rig.assert_acls_restored(&before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_racing_begin_launch_never_sends_execute() {
    let rig = Rig::new(4);
    let before = rig.snapshot_acls();
    // The authority re-read is held open and cancel races it. The launch
    // re-reads the live stop signal and the launch window when the wait ends,
    // before any Execute byte exists, and settles as a clean NotStarted.
    let gate = Arc::new(Notify::new());
    rig.port.configure_begin_launch(Some(gate.clone()), None);
    let task = rig.spawn_run(300);
    drive_to_execute_until_ready(&rig).await;
    rig.cancelled.send(true).unwrap();
    rig.port.allow_begin_launch();
    gate.notify_waiters();
    task.await.unwrap().expect("cancelled launch settles");
    let completes = rig.port.completes();
    assert_eq!(completes.len(), 1);
    assert_eq!(completes[0].not_started, Some(NotStarted::Cancelled));
    assert_eq!(rig.runner.execute_count(), 0, "no Execute frame after the stop signal");
    assert_eq!(rig.runner.cancel_count(), 0, "no Cancel frame without an Execute");
    assert_eq!(rig.port.begin_launch_calls(), 1);
    assert_eq!(rig.port.work_records().len(), 1, "exactly one workload record");
    rig.assert_acls_restored(&before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn launch_window_expiring_during_begin_launch_never_executes() {
    let rig = Rig::new(4);
    let before = rig.snapshot_acls();
    // The authority re-read is held open and the boundary clock jumps past the
    // launch window while it waits; no cancel is sent. The re-read after the
    // wait ends settles the launch as a clean NotStarted::Expired.
    let gate = Arc::new(Notify::new());
    rig.port.configure_begin_launch(Some(gate.clone()), None);
    let task = rig.spawn_run(60);
    drive_to_execute_until_ready(&rig).await;
    rig.boundary.clock_offset_ms.fetch_add(120_000, Ordering::AcqRel);
    rig.port.allow_begin_launch();
    gate.notify_waiters();
    task.await.unwrap().expect("expired launch settles");
    let completes = rig.port.completes();
    assert_eq!(completes.len(), 1);
    assert_eq!(completes[0].not_started, Some(NotStarted::Expired));
    assert_eq!(rig.runner.execute_count(), 0);
    assert_eq!(rig.runner.cancel_count(), 0);
    assert_eq!(rig.port.begin_launch_calls(), 1);
    assert_eq!(rig.port.work_records().len(), 1, "exactly one workload record");
    rig.assert_acls_restored(&before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_after_execute_sends_one_cancel_and_settles_once() {
    let rig = Rig::new(4);
    let before = rig.snapshot_acls();
    // The Execute frame is on the wire before the stop arrives, so the launch
    // keeps the conservative semantics: exactly one Cancel follows it, no
    // replay, and the launch settles once through the direct-observation path.
    let task = rig.spawn_run(300);
    drive_to_execute(&rig).await;
    rig.cancelled.send(true).unwrap();
    wait_until("cancel frame", || rig.runner.cancel_count() == 1).await;
    let process_id = std::process::id();
    rig.runner.send(RunnerCoreMessage::Started {
        attempt: rig.request.binding.attempt.clone(),
        process_id,
    });
    rig.runner.send(RunnerCoreMessage::Completed {
        observation: Box::new(finished_observation(&rig, process_id)),
    });
    let evidence = task.await.unwrap().expect("launch settles once");
    assert!(matches!(evidence, TerminationEvidence::Confirmed { .. }));
    assert_eq!(rig.runner.execute_count(), 1, "exactly one Execute, never retried");
    assert_eq!(rig.runner.cancel_count(), 1, "exactly one Cancel follows it");
    assert_eq!(rig.port.begin_launch_calls(), 1);
    let completes = rig.port.completes();
    assert_eq!(completes.len(), 1);
    assert_eq!(completes[0].not_started, None);
    assert!(completes[0].observation.is_some(), "the direct result is published once");
    assert_eq!(rig.port.work_records().len(), 1, "exactly one workload record");
    rig.assert_acls_restored(&before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runner_rejection_at_ready_settles_as_rejected() {
    let rig = Rig::new(4);
    let before = rig.snapshot_acls();
    let task = rig.spawn_run(300);
    wait_until("pipe connect", || rig.port.stages().contains(&LaunchStageKind::PipeConnected))
        .await;
    rig.runner.send(RunnerCoreMessage::Rejected { reason: "runner.install_version".into() });
    task.await.unwrap().expect("rejected launch settles");
    let completes = rig.port.completes();
    assert_eq!(completes.len(), 1);
    assert_eq!(completes[0].not_started, Some(NotStarted::Rejected));
    // The receipt carries the Runner's own bounded reason, not the generic
    // wrapper the launch bailed with.
    assert_eq!(completes[0].failure_code.as_deref(), Some("runner.install_version"));
    assert_eq!(rig.runner.execute_count(), 0);
    assert_eq!(rig.port.begin_launch_calls(), 0);
    assert_eq!(
        rig.port.state.lock().unwrap().diagnostics,
        vec!["runner.install_version".to_string()]
    );
    assert_eq!(rig.port.work_records().len(), 1, "exactly one workload record");
    rig.assert_acls_restored(&before);
}
