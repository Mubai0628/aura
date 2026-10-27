#![forbid(unsafe_code)]
mod service_composition;
#[cfg(test)]
use service_composition::classify_startup;
use service_composition::{
    AgentLiveOwners, CoreConfigTransactionRecovery, P03Dependencies, RuntimeDependentRebuilder,
    SidecarFacts, attach_dependent_rebuilder, claim_refusal, initialize_runtime_services,
    initialize_services_at, open_dependent_services,
};

mod runtime_lifecycle;
use runtime_lifecycle::{spawn_p05_maintenance, spawn_p07_maintenance};

/// The default-off S02-E acceptance boundary. Compiled only with `--features acceptance-barrier`;
/// no product build enables it, and with the feature off this module does not exist at all.
#[cfg(feature = "acceptance-barrier")]
pub mod acceptance_barrier;
mod activity;
/// The projections a real Agent executor publishes into the run-information
/// index, and the query surface over that index.
mod agent_activity;
mod agent_application;
mod agent_execution;
mod agent_store;
mod agent_supervision;
mod blocking_pool;
mod broker;
mod busy;
mod chrome_control;
mod command_claims;
pub(crate) mod git;
pub mod package_http;
pub mod package_lifecycle;
pub mod package_program;
pub(crate) mod package_services;
mod secret_commands;
mod terminal_secret;
use secret_commands::{
    expire_pending_secret_commands, handle_secret_request, prepare_pending_secret_command,
};
mod command_execution;
mod command_settlement;
/// What a configuration fault costs Core, and what it does not.
pub(crate) mod config_degradation;
/// The reachable user entry for recovering a damaged configuration.
mod config_recovery;
mod controlled_web;
mod projection_publish;
use config_degradation::ConfigDependentCapability;
mod core_health;
mod credential_io;
#[cfg(test)]
mod ipc_lifecycle_tests;
mod ipc_session;
#[cfg(test)]
mod r03_tests;
mod readiness;
mod runtime_pulse;
#[cfg(test)]
mod test_watchdog;
pub(crate) use core_health::current_core_health_projection;
pub(crate) use runtime_pulse::spawn_heartbeat;
mod core_log;
#[cfg(test)]
pub(crate) use core_log::{CoreLogRoute, write_core_log_line};
pub(crate) use core_log::{append_core_log, append_startup_stage};
mod routes;
/// The recovery lifecycle — Models and combined faults, the first
/// recovery a process never ran, and the health/availability distinction.
#[cfg(test)]
#[path = "service_composition/tests/recovery_lifecycle/mod.rs"]
mod s05r3_recovery_lifecycle_tests;
#[cfg(test)]
mod s05r_config_isolation_tests;
/// The recovery entry is reachable, not just implemented.
#[cfg(test)]
#[path = "service_composition/tests/recovery_entry.rs"]
mod s05r_recovery_entry_tests;
/// The secret domain boundary through the real authority chains.
#[cfg(test)]
mod s05r_secret_authority_tests;
mod secret_boundary;
/// Supplying a missing capability's service on a live runtime,
/// with the startup-only work of a process deliberately kept out of reach of it.
mod service_rebuild;
mod tool_credential_service;

use command_claims::{
    ClaimVerdict, CommandClaim, CommandFlights, CommandOutcomeLedger, CommandReservation,
    CommandSettlement, claim_verdict, command_invocation_refusal, missing_claim_error,
    needs_command_claim, reserve_command_invocation, settle_command_claim,
};

mod external_grants;
mod hooks;
mod host_preparation;
mod mcp;
mod memory_services;
pub mod model_channel;
#[cfg(test)]
mod model_channel_tests;
mod native_session_grants;
mod p03;
mod p04;
mod p04_broker;
mod p05;
pub mod p05_credential;
#[allow(
    dead_code,
    reason = "the W7 authorization-bound adapter is committed before its gated fixture-runtime wiring"
)]
mod p07;
mod p07_builtin;
mod p07_commands;
#[cfg(any(test, feature = "p08-contract-driver"))]
mod p07_continuation;
mod p07_control;
#[allow(
    dead_code,
    reason = "the W7 admission coordinator is connected before the gated Runtime dispatch route"
)]
mod p07_execution;
#[allow(
    dead_code,
    reason = "the W7 HTTP assembly is connected before the gated production batch route"
)]
mod p07_http;
#[allow(
    dead_code,
    reason = "the W7 dynamic output importer is consumed by the production AuraStdio route"
)]
mod p07_output;
#[allow(
    dead_code,
    reason = "the exact Registry control service is ready before the P08 Core Tool route"
)]
mod p07_registry;
#[allow(
    dead_code,
    reason = "the W7 result reconciler is connected before the final production route"
)]
mod p07_result;
#[cfg(any(test, feature = "p08-contract-driver"))]
mod p07_test_support;
#[cfg(feature = "p08-contract-driver")]
mod p08_contract_driver;
mod plugins;
#[cfg(test)]
mod r04_tests;
#[cfg(test)]
mod r05_tests;
/// **The real Named Pipe / two-connection resource gate for batch A.**
///
/// It is test-only and it owns no production behaviour: the pipe helpers it uses are the same
/// production ones `run_core` uses, and the lane state machine it drives is the real `IpcSession`.
#[cfg(test)]
mod s045b_pipe_gate_tests;
pub mod security_control;
mod session_branch;
mod session_lifecycle;
mod sidecars;
mod skills;
mod slash_commands;
mod sync_boundary;
#[cfg(test)]
mod sync_boundary_tests;
mod workspace_policy;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Read as _,
    path::{Component, Path, PathBuf},
    process,
    str::FromStr,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use aura_config::{
    AuraPaths, ConfigEventKind, ConfigModule, ConfigTransactionJournal, ConfigTransactionRecovery,
    ConfigTransactionTarget, ConfigWatchHandle, ModuleConfigValidator, SettingsFacade,
};
use aura_domain_common::{
    CommandInvocationId, ContentHash, ContentHashAlgorithm, ContractVersion, CorrelationId,
    ErrorCategory, ErrorCode, IpcChannelId, Revision, RuntimeInstanceId, SecretNonce, UtcTimestamp,
};
use aura_ipc::{
    AsyncPriorityQueue, AuthenticatedFrameReader, AuthenticatedWireFrame, CancellationSettlement,
    FrameRead, JSON_RPC_VERSION, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, MAX_CONTROL_PAYLOAD_BYTES, OutboundFrame, PendingSecretInput,
    PendingSecretInputRegistry, QueuePriority, RequestLedger, WireFrame, method,
    read_authenticated_frame, write_authenticated_frame,
};
use aura_model_config::ModelConfigValidator;
use aura_network_control::AuthenticatedConfigurationChannelIdentity;
use aura_permission_domain::{SecurityProjectionChange, SecurityProjectionPayload};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_projection_contract::{
    ProjectionChange, ProjectionKind, ProjectionScope, ProjectionSnapshotRequest,
    ProjectionSubscription, ProjectionUnsubscribeRequest,
};
use aura_runtime_contract::{
    AuthenticatedPeerIdentity, BuildComponentKind, BuildComponentManifest, BundleNamespace,
    CancellationRequest, CommandOutcome, CommandOutcomeQuery, CoreHealthProjection,
    CoreLifecycleState, CoreStartupPhase, CoreStartupProgress, IpcErrorEnvelope, IpcFrameHeader,
    IpcRequestTerminal, ReleaseDescriptor, ReleaseIdentity, RuntimeHandshake,
    RuntimeHandshakeComplete, RuntimeHandshakeFinish, RuntimeHandshakeKind,
    RuntimeHandshakeRequest, RuntimeHandshakeResponse, RuntimeHandshakeTranscript,
    RuntimeHeartbeat, RuntimeProcessKind, runtime_handshake_transcript_hash,
    verify_runtime_bootstrap_proof,
};
use aura_windows_platform::{
    ProcessIdentity, RetainedProcess, SecretBytes, create_secure_byte_pipe_server,
    named_pipe_client_identity, retain_process,
};
use aura_workspace_domain::{WorkspaceCreateResult, WorkspaceSummary};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{net::windows::named_pipe::NamedPipeServer, task::JoinHandle, time};
use zeroize::{Zeroize, Zeroizing};

use crate::p03::{P03Services, P03ToolsConfigValidator};
use crate::p04::{P04SecurityConfigValidator, P04Services};
use crate::p04_broker::ElevatedBrokerOptions;
use crate::p05::P05Services;

/// Narrow P05 boundary used by the Session owner while atomically creating a
/// Session, its initial model binding and the N-A channel authorization.
pub trait InitialSessionModelBindingResolver {
    fn resolve_initial_session_model_binding(
        &self,
    ) -> Result<aura_model_domain::InitialSessionModelBinding>;
}

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
/// Names the Release Bundle directory a development run assembled.
///
/// Only read when `--allow-dev-component-mismatch` is passed, so a packaged Core
/// cannot be pointed at another directory by an environment variable. It selects a
/// location; every component's content hash and protocol are still verified.
const DEV_COMPONENT_ROOT_ENVIRONMENT: &str = "AURA_DEV_COMPONENT_ROOT";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CoreTestMode {
    #[default]
    Normal,
    Incompatible,
    CrashAfterHandshake,
    DisconnectAfterReady,
    ProjectionRevisionGap,
    StartupSequenceGap,
    StallAfterHandshake,
    MigrationRecoveryRequired,
}

#[derive(Clone, Debug)]
pub struct CoreOptions {
    pub pipe_name: String,
    pub bootstrap_stdin: bool,
    pub parent_process_id: u32,
    pub runtime_instance_id: RuntimeInstanceId,
    pub component_manifest_path: PathBuf,
    pub allow_dev_component_mismatch: bool,
    pub safe_mode: bool,
    pub test_mode: CoreTestMode,
    #[cfg(feature = "p08-contract-driver")]
    pub(crate) p08_contract_driver: Option<p08_contract_driver::P08ContractDriverOptionsV1>,
}

impl CoreOptions {
    pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut pipe_name = None;
        let mut bootstrap_stdin = false;
        let mut parent_process_id = None;
        let mut runtime_instance_id = None;
        let mut component_manifest_path = None;
        let mut allow_dev_component_mismatch = false;
        let mut safe_mode = false;
        let mut test_mode = CoreTestMode::Normal;
        #[cfg(feature = "p08-contract-driver")]
        let mut p08_contract_driver =
            p08_contract_driver::P08ContractDriverOptionsBuilderV1::default();

        for argument in arguments.into_iter().skip(1) {
            #[cfg(feature = "p08-contract-driver")]
            if p08_contract_driver.try_parse(&argument)? {
                continue;
            }
            if let Some(value) = argument.strip_prefix("--pipe-name=") {
                pipe_name = Some(value.to_owned());
            } else if argument == "--bootstrap-stdin" {
                bootstrap_stdin = true;
            } else if let Some(value) = argument.strip_prefix("--parent-pid=") {
                parent_process_id = Some(value.parse::<u32>().context("invalid --parent-pid")?);
            } else if let Some(value) = argument.strip_prefix("--runtime-instance-id=") {
                runtime_instance_id = Some(
                    RuntimeInstanceId::from_str(value).context("invalid --runtime-instance-id")?,
                );
            } else if let Some(value) = argument.strip_prefix("--component-manifest=") {
                component_manifest_path = Some(PathBuf::from(value));
            } else if argument == "--allow-dev-component-mismatch" {
                allow_dev_component_mismatch = true;
            } else if argument == "--safe-mode" {
                safe_mode = true;
            } else if let Some(value) = argument.strip_prefix("--test-mode=") {
                test_mode = match value {
                    "incompatible" => CoreTestMode::Incompatible,
                    "crash-after-handshake" => CoreTestMode::CrashAfterHandshake,
                    "disconnect-after-ready" => CoreTestMode::DisconnectAfterReady,
                    "projection-revision-gap" => CoreTestMode::ProjectionRevisionGap,
                    "startup-sequence-gap" => CoreTestMode::StartupSequenceGap,
                    "stall-after-handshake" => CoreTestMode::StallAfterHandshake,
                    "migration-recovery-required" => CoreTestMode::MigrationRecoveryRequired,
                    _ => bail!("unknown Core test mode: {value}"),
                };
            } else {
                bail!("unsupported aura-core argument: {argument}");
            }
        }

        let options = Self {
            pipe_name: pipe_name.context("missing --pipe-name")?,
            bootstrap_stdin,
            parent_process_id: parent_process_id.context("missing --parent-pid")?,
            runtime_instance_id: runtime_instance_id.context("missing --runtime-instance-id")?,
            component_manifest_path: component_manifest_path
                .context("missing --component-manifest")?,
            allow_dev_component_mismatch,
            safe_mode,
            test_mode,
            #[cfg(feature = "p08-contract-driver")]
            p08_contract_driver: p08_contract_driver.finish()?,
        };
        options.validate()?;
        Ok(options)
    }

    fn validate(&self) -> Result<()> {
        if self.parent_process_id == 0 {
            bail!("--parent-pid must not be zero");
        }
        if !self.bootstrap_stdin {
            bail!("Aura Core requires the inherited bootstrap input channel");
        }
        let scope = self
            .pipe_name
            .strip_prefix(r"\\.\pipe\Aura\")
            .context("pipe name is outside the Aura namespace")?;
        let parts = scope.split('\\').collect::<Vec<_>>();
        if parts.len() != 3
            || !is_lower_hex_128(parts[0])
            || parts[1] != self.runtime_instance_id.to_string()
            || !is_lower_hex_128(parts[2])
        {
            bail!("pipe name must contain namespace hash, runtime identity, and 128-bit nonce");
        }
        Ok(())
    }

    fn validate_release_namespace(&self, namespace: &BundleNamespace) -> Result<()> {
        let expected = format!(
            r"\\.\pipe\Aura\{}\{}\",
            namespace.object_name_hash(),
            self.runtime_instance_id
        );
        self.pipe_name
            .strip_prefix(&expected)
            .context("pipe name does not match the active Release namespace")?;
        Ok(())
    }
}

fn is_lower_hex_128(value: &str) -> bool {
    value.len() == 32
        && value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

#[derive(Clone)]
struct CoreReleaseContext {
    release_descriptor: ReleaseDescriptor,
    component_version: String,
    build_commit: String,
    manifest_path: PathBuf,
    elevated_helper_executable: PathBuf,
    elevated_helper_hash: ContentHash,
    sandbox_host: SidecarReleaseIdentity,
    egress_proxy: SidecarReleaseIdentity,
}

#[derive(Clone)]
struct SidecarReleaseIdentity {
    process_kind: RuntimeProcessKind,
    component_version: String,
    executable: PathBuf,
    content_hash: ContentHash,
    protocol_version: Option<ContractVersion>,
}

struct AuthenticatedChannelState {
    bootstrap_secret: Option<Zeroizing<[u8; 32]>>,
    peer_identity: Option<AuthenticatedPeerIdentity>,
    peer_handshake: Option<RuntimeHandshake>,
    active_channel_id: Option<IpcChannelId>,
    connection_epoch: u64,
    transcript_hash: Option<ContentHash>,
}

/// How long disconnect cleanup waits for real scans and handlers to reclaim
/// before it aborts their tasks. Aborting is not reclamation, so a search that
/// outlives this budget keeps its cancel handle and is reported as pending.
const SEARCH_RECLAIM_TIMEOUT: Duration = Duration::from_secs(2);

/// How long disconnect cleanup waits for ordinary handler *tasks* to stop after
/// they have been aborted.
///
/// `abort()` only takes effect at the task's next await point, and a handler parked
/// in synchronous work — a host API, a persistence write — may not reach one for a
/// long time. This budget is what keeps one such handler from holding the whole
/// teardown; the handler itself is handed to [`detached_handlers`] rather than
/// cancelled out of existence, because the work it started is still real.
const HANDLER_RECLAIM_TIMEOUT: Duration = Duration::from_millis(500);

/// Handlers whose connection ended before they stopped.
///
/// A task that could not be joined inside its budget keeps running — the synchronous
/// work inside it cannot be interrupted by `abort()` — so something has to keep
/// owning it. This is that owner, and it is deliberately process-scoped and
/// self-cleaning: one watcher per detached handler, released when the handler really
/// ends. Nothing here re-replies, re-dispatches or reports a result: the connection
/// that asked is gone, and its reply channel is closed, so a late result can only be
/// dropped by the dead channel and never delivered into a later connection.
mod detached_handlers {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::ipc_session::HandlerTerminal;

    static LIVE: AtomicUsize = AtomicUsize::new(0);
    static TOTAL: AtomicUsize = AtomicUsize::new(0);

    /// Takes ownership of handlers that did not stop inside the budget.
    ///
    /// Each one gets exactly one watcher task that awaits its real completion, so the
    /// count below is the number of connection-scoped handlers still running rather
    /// than the number that were once abandoned.
    pub(super) fn detach(handles: Vec<tokio::task::JoinHandle<HandlerTerminal>>) -> usize {
        let detached = handles.len();
        if detached == 0 {
            return 0;
        }
        TOTAL.fetch_add(detached, Ordering::Relaxed);
        LIVE.fetch_add(detached, Ordering::Relaxed);
        for handle in handles {
            tokio::spawn(async move {
                let _ = handle.await;
                LIVE.fetch_sub(1, Ordering::Relaxed);
            });
        }
        detached
    }

    /// Handlers still running after their connection ended.
    pub(super) fn live() -> usize {
        LIVE.load(Ordering::Relaxed)
    }

    /// Handlers ever handed over, for a bounded diagnostic.
    pub(super) fn total() -> usize {
        TOTAL.load(Ordering::Relaxed)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("authenticated Main/Core pairing changed: {0}")]
struct PairingInvalidated(String);

struct RuntimeState {
    options: CoreOptions,
    release: CoreReleaseContext,
    expected_parent: RetainedProcess,
    authenticated_channel: Mutex<AuthenticatedChannelState>,
    command_outcomes: Arc<Mutex<CommandOutcomeLedger>>,
    terminal_openings: command_execution::terminal_opening::OpeningRegistry,
    startup_sequence: AtomicU64,
    heartbeat_sequence: AtomicU64,
    core_health_revision: AtomicU64,
    /// Which configuration-derived capabilities this process really lost.
    ///
    /// Owned by the runtime state because it is a fact about *this* process's
    /// startup, and every consumer that needs it already holds the state. It owns
    /// no configuration and no permission, so it cannot become a second source of
    /// truth.
    config_degradation: config_degradation::SharedConfigDegradation,
    workspace_session_revision: AtomicU64,
    workspace_files_revision: AtomicU64,
    ready: AtomicBool,
    startup_complete: AtomicBool,
    startup_mode: StartupMode,
    /// The bounded synchronous boundary every blocking Core step runs on. Owned here
    /// because every entry point that needs it already holds the runtime state, and shared
    /// by cloning: one pool, one record, for the whole process.
    sync: crate::sync_boundary::SyncBoundary,
    /// Who owns the result of a synchronous business operation whose caller stopped waiting.
    /// One owner for the process, bound to this runtime instance, so a late result can only
    /// settle the invocation it belongs to and only inside the Core that produced it.
    settlements: crate::command_settlement::CommandSettlements,
    /// Who owns the **post-execution** tail of a broker dispatch (the evidence step and the durable
    /// ledger completion) once the caller that started it stops waiting. One record for the
    /// process, and every tail it admits is bounded by the settlement owner above.
    post_execution: crate::broker::completion::PostExecutionOwners,
    disconnect_injected: AtomicBool,
    reconnect_delay_injected: AtomicBool,
    rejected_connection_log_at_ms: AtomicU64,
    #[cfg(feature = "p08-contract-driver")]
    p08_contract_driver_consumed: AtomicBool,
}

#[derive(Default)]
struct ProjectionSubscriptions {
    workspace_session: AtomicBool,
    workspace_files: AtomicBool,
    routes: Mutex<BTreeSet<(ProjectionKind, ProjectionScope)>>,
}

impl ProjectionSubscriptions {
    fn subscribe(&self, kind: ProjectionKind, scope: ProjectionScope) -> Result<()> {
        self.routes
            .lock()
            .map_err(|_| anyhow::anyhow!("projection subscription registry was poisoned"))?
            .insert((kind, scope));
        match kind {
            ProjectionKind::CoreHealth => {}
            ProjectionKind::WorkspaceSession => {
                self.workspace_session.store(true, Ordering::Release);
            }
            ProjectionKind::WorkspaceFiles => {
                self.workspace_files.store(true, Ordering::Release);
            }
            ProjectionKind::ModelSettings
            | ProjectionKind::ProviderCatalog
            | ProjectionKind::EndpointDetail
            | ProjectionKind::ModelProfile
            | ProjectionKind::SessionModelSelector
            | ProjectionKind::ModelAvailabilityTest
            | ProjectionKind::ModelDiscovery
            | ProjectionKind::ContextWindowCorrection
            | ProjectionKind::PromptInspector
            | ProjectionKind::ToolSettings
            | ProjectionKind::ToolPackageList
            | ProjectionKind::ToolDraftList
            | ProjectionKind::WorkspaceToolPins
            | ProjectionKind::SessionToolPins
            | ProjectionKind::WorkspaceToolRegistrySummary
            | ProjectionKind::SessionEffectiveToolRegistry
            | ProjectionKind::SessionLoadedTools
            | ProjectionKind::SessionToolActivity
            | ProjectionKind::ToolPackage
            | ProjectionKind::ToolDefinition
            | ProjectionKind::ToolDraft
            | ProjectionKind::ToolConformanceRun
            | ProjectionKind::ToolLongOperation
            | ProjectionKind::ToolBatch
            | ProjectionKind::ToolCall
            | ProjectionKind::ToolExecutionAttempt => {}
        }
        Ok(())
    }

    fn unsubscribe(&self, kind: ProjectionKind, scope: ProjectionScope) -> Result<()> {
        let mut routes = self
            .routes
            .lock()
            .map_err(|_| anyhow::anyhow!("projection subscription registry was poisoned"))?;
        routes.remove(&(kind, scope));
        if kind == ProjectionKind::WorkspaceSession {
            self.workspace_session.store(
                routes
                    .iter()
                    .any(|(route_kind, _)| *route_kind == ProjectionKind::WorkspaceSession),
                Ordering::Release,
            );
        }
        if kind == ProjectionKind::WorkspaceFiles {
            self.workspace_files.store(
                routes.iter().any(|(route_kind, _)| *route_kind == ProjectionKind::WorkspaceFiles),
                Ordering::Release,
            );
        }
        Ok(())
    }

    fn contains(&self, change: &ProjectionChange) -> bool {
        self.routes
            .lock()
            .is_ok_and(|routes| routes.contains(&(change.kind(), change.projection_scope())))
    }
}

struct RuntimeServices {
    broker: Arc<broker::BrokerJournal>,
    /// The one tool-package service. `None` when its owner could not start (for
    /// example another Core holds the package namespace); only package routes refuse.
    packages: Option<Arc<package_services::PackageServices>>,
    /// The one run-information index of this process. Created beside the other
    /// process-wide owners and shared by cloning, so the Agent publisher, the Shell backend,
    /// Main's filesystem entry and the context fragment all read the same table.
    activity_index: aura_activity_runtime::ActivityIndex,
    /// The drop counters every publisher reports into, so "the index lost a fact" is one
    /// process-wide observable rather than a per-owner private tally.
    activity_observer: aura_activity_runtime::ActivityObserverHandle,
    /// The runtime instance this process really is. It is the same value every other owner
    /// receives, so an activity's runtime generation is the process's own identity.
    activity_runtime_instance_id: RuntimeInstanceId,
    /// The bounded synchronous boundary of this process. It is created with the services so the
    /// Agent Store and every route share one pool and one admission capacity.
    sync: crate::sync_boundary::SyncBoundary,
    /// The services that genuinely depend on the workspace runtime and the
    /// tool/model configuration. `None` means that capability was refused because
    /// its configuration domain is damaged — it is **not** a Core failure, and
    /// every field below that is not `Option` keeps working regardless.
    ///
    /// Each slot is behind its own `Mutex` so a **real**
    /// configuration recovery can supply the missing service on the live runtime. A
    /// missing capability is not permanent — the configuration it depends on can be
    /// repaired in place — and the slot has to be fillable for the recovery to be able to
    /// report anything more than a cleared record.
    ///
    /// A per-slot lock, deliberately **not** one lock over the whole service graph: a
    /// rebuild prepares outside the lock and publishes inside it for one slot, so unrelated
    /// requests are never blocked behind an unrelated capability's construction.
    commands: Arc<DependentSlot<command_execution::CommandExecution>>,
    broker_http: Arc<aura_network_control::CoreHttpClientGateway>,
    /// The Agent execution layer, retained so a **live** Agent supply can reuse the process's
    /// existing one instead of constructing a second.
    ///
    /// It is a slot, because a damaged `tools.json` means the
    /// startup path refused it and a live recovery must be able to create the one the process
    /// never got. Every consumer reads this slot, so a supplied layer is the layer requests use.
    agent_execution: Arc<DependentSlot<agent_execution::AgentExecution>>,
    agent: Arc<DependentSlot<agent_application::AgentApplication>>,
    /// P04's security authority, as a slot so a live recovery can supply it and so a
    /// request reads the **same** object the recovery published.
    ///
    /// This used to be a plain `Option` read by
    /// [`RuntimeServices::p04`], while the rebuilder inspected a *separate* slot — two
    /// sources for one fact. A recovery could fill the slot and the ordinary request
    /// would keep reading the old `Option`, so `still_missing` was reported forever and
    /// no request ever saw the service. There is now one slot per capability and every
    /// consumer reads it.
    p04_slot: Arc<DependentSlot<P04Services>>,
    /// The model authority, likewise a slot.
    ///
    /// Its construction is safe against a live runtime because opening `P05Services`
    /// borrows the existing persistence, settings and boundary and creates only its own
    /// per-service owners. The pending-journal finishing it used to do inline is now the
    /// startup half, so a live supply does not re-run it.
    p05_slot: Arc<DependentSlot<P05Services>>,
    /// The workspace-files capability, likewise a slot.
    p03_slot: Arc<DependentSlot<P03Services>>,
    /// The process's existing Agent Store owner, retained so a
    /// rebuild can **reuse** it instead of constructing a second admission capacity.
    ///
    /// It is the same `Arc` the startup path already created and handed to the execution
    /// layer — a reference, not a new owner.
    agent_store: Arc<agent_store::AgentStore>,
    _config_watcher: ConfigWatchHandle,
    _p05_maintenance: Option<RuntimeBackgroundTask>,
    _p07_maintenance: Option<RuntimeBackgroundTask>,
    _settings: Arc<SettingsFacade>,
    /// The MCP servers of this process (discovery and one connection per session).
    mcp: Arc<mcp::McpRuntime>,
    /// The user's Skills, for Settings → Skill and the model's `skills.*` tools.
    skills: Arc<skills::SkillRuntime>,
    /// The installed plugins, for Settings → 插件.
    plugins: Arc<plugins::PluginRuntime>,
    /// Chrome control: the relay's pipe, pairing and the connected extension.
    chrome: Arc<chrome_control::ChromeControl>,
    /// The process's configuration-derived capability availability, passed
    /// on to the runtime state so the health projection and the tool-policy path
    /// read the same record rather than each re-deriving it.
    config_degradation: config_degradation::SharedConfigDegradation,
    /// The reachable user entry for recovering a damaged configuration.
    ///
    /// Built from the same settings facade and the same degradation registry the
    /// rest of the process uses, so a recovery updates the very record the health
    /// projection and the capability checks read.
    config_recovery: config_recovery::ConfigRecoveryEntry,
    /// The process's real record of which one-time startup steps have finished.
    ///
    /// The same ledger the rebuilder consults, held here so the
    /// runtime's own diagnostics can answer "is the first recovery owed?" without asking a service.
    #[cfg_attr(not(test), allow(dead_code))]
    startup_ledger: Arc<service_rebuild::StartupLedger>,
    persistence: Arc<PersistenceCoordinator>,
    /// The owners a **live** supply needs and must reuse. They are the process's existing
    /// `Arc`s, cloned into the supply path rather than recreated there.
    p03_dependencies: Arc<P03Dependencies>,
    /// The release facts a supply needs to construct a service the same way startup did.
    release: Arc<CoreReleaseContext>,
    /// The process's activity publisher set, retained so a live
    /// supply of the execution layer reuses the one the Shell, Main and buffer paths publish
    /// into. A second set would be a second owner of the same facts.
    activities: Arc<activity::AgentActivities>,
    /// `None` when the sidecar set could not be built. The readiness
    /// projection already reports each sidecar's own state, so an absent sidecar
    /// set is reported as unavailable rather than as ready.
    ///
    /// Behind a shared `Mutex` so a live supply can install the
    /// set and every consumer sees it. It is the process's sidecar set either way — a supply
    /// does not create a second one, it creates the one the process never got.
    security_sidecars: Arc<std::sync::Mutex<Option<Arc<sidecars::SecuritySidecars>>>>,
}

struct RuntimeBackgroundTask(JoinHandle<()>);

/// One capability's service, present or absent, fillable on a live runtime.
///
/// A plain `Option` could only be set at construction, so a
/// capability lost to a damaged configuration domain stayed lost until the process
/// restarted — even after the configuration was repaired.
///
/// The lock guards **only this slot**, and it is held for the publication of an already
/// prepared service rather than across its construction. That keeps two properties:
///
/// * a request that reads a present service is never blocked behind another capability's
///   construction work;
/// * the value a reader gets is a complete service, never a half-built one.
struct DependentSlot<T>(std::sync::Mutex<Option<Arc<T>>>);

impl<T> DependentSlot<T> {
    fn empty() -> Self {
        Self(std::sync::Mutex::new(None))
    }

    fn filled(service: Arc<T>) -> Self {
        Self(std::sync::Mutex::new(Some(service)))
    }

    /// The service, when it is present.
    fn get(&self) -> Option<Arc<T>> {
        self.0.lock().ok().and_then(|slot| slot.clone())
    }

    fn is_present(&self) -> bool {
        self.0.lock().is_ok_and(|slot| slot.is_some())
    }

    /// Publishes a prepared service. An occupant is **kept**, never replaced.
    ///
    /// Refusing to replace is what preserves an in-flight task: the object already serving
    /// holds the task's owner, run identity and cancel path, and swapping it would orphan
    /// them. A rebuild supplies only what is missing.
    fn publish_if_vacant(&self, service: Arc<T>) -> bool {
        let Ok(mut slot) = self.0.lock() else {
            return false;
        };
        if slot.is_some() {
            return false;
        }
        *slot = Some(service);
        true
    }
}
// R1: `RuntimeServices::activity_subject()` used to live here. It selected
// `projection.sessions.first()` - the *first* session in the process projection, not the
// session that made the request - and every non-Agent publisher was handed that guess as its
// owner. It is removed rather than repaired, because there is nothing in `RuntimeServices` that
// knows which request is being served: the real owner facts only exist at the request's own
// entry. Each publisher now receives them from that entry (`MainFsActivitySubject` for Main
// files, `BufferCommitFact` for buffer commits, `P04Services` lifecycle for publications), so a
// request belonging to a second session can no longer be attributed to the first.

/// The capability a dependent field carries, for an honest refusal.
///
/// A dependent service is `None` only because its configuration domain is
/// damaged. Every accessor below turns that into a typed error naming the
/// capability and its stable code, so a caller refuses the *dependent* request
/// while unrelated control keeps working. Nothing here panics: a panic in a
/// configuration-degraded process would take away the very control paths the
/// user's confirmed rule requires to survive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MissingCapability {
    P03WorkspaceFiles,
    P04SecurityAuthority,
    P05ModelAuthority,
    AgentExecution,
    CommandExecution,
    SecuritySidecars,
}

impl MissingCapability {
    pub(crate) const fn capability(self) -> ConfigDependentCapability {
        match self {
            Self::P03WorkspaceFiles | Self::AgentExecution | Self::CommandExecution => {
                ConfigDependentCapability::ToolPolicy
            }
            Self::P04SecurityAuthority => ConfigDependentCapability::SecurityAuthority,
            Self::P05ModelAuthority => ConfigDependentCapability::ModelAuthority,
            // The sidecar readiness set is part of the tool-policy chain: it is
            // built after P03 and needs the same workspace runtime.
            Self::SecuritySidecars => ConfigDependentCapability::ToolPolicy,
        }
    }

    /// The stable code a caller may report, and the reason text.
    pub(crate) fn refusal(self) -> anyhow::Error {
        let capability = self.capability();
        anyhow::anyhow!(
            "{}: 这项能力的配置读不出来，暂不可用；其他工作、状态查看与修复不受影响。",
            capability.unavailable_code()
        )
    }

    /// The refusal as an IPC terminal envelope.
    ///
    /// A route whose capability is gone must answer with a stable,
    /// renderer-safe code and `Unavailable` — not with a panic and not with a
    /// fabricated success. The message names the capability only; no path, no
    /// document body and nothing the caller supplied enters it.
    pub(crate) fn envelope(self, correlation_id: CorrelationId) -> IpcErrorEnvelope {
        let code = self.capability().unavailable_code();
        IpcErrorEnvelope {
            code: ErrorCode::new(code).expect("static capability error code is valid"),
            category: ErrorCategory::Unavailable,
            message: format!(
                "这项能力的配置读不出来，暂不可用（{code}）；其他工作、状态查看与修复不受影响，\
                 可到设置中查看恢复选项。"
            ),
            retryable: false,
            correlation_id,
            terminal: IpcRequestTerminal::Failed,
            details: BTreeMap::new(),
        }
    }
}

/// Whether a capability is served by a **service slot** at all.
///
/// `ConfigTransactionRecovery` is not: it is served by the startup judgement, which has
/// already run by the time a recovery can be attempted. Reporting it is a separate statement
/// from reporting a service, and this is where that distinction is made.
const fn is_slot_backed(capability: ConfigDependentCapability) -> bool {
    matches!(
        capability,
        ConfigDependentCapability::ToolPolicy
            | ConfigDependentCapability::SecurityAuthority
            | ConfigDependentCapability::ModelAuthority
    )
}

enum StartupMode {
    Ready(Box<RuntimeServices>),
    RecoveryRequired { reason: String },
}

fn new_runtime_state(
    options: CoreOptions,
    release: CoreReleaseContext,
    expected_parent: RetainedProcess,
    bootstrap_secret: Zeroizing<[u8; 32]>,
    startup_mode: StartupMode,
) -> RuntimeState {
    // Read before `options` is moved into the state: the settlement owner is bound to this
    // runtime instance, which is what makes a late result unable to confirm another process.
    let runtime_instance_id = options.runtime_instance_id;
    let command_outcomes = Arc::new(Mutex::new(CommandOutcomeLedger::default()));
    // The one boundary of the process: the services created theirs, and the state shares it
    // instead of opening a second pool that would be a second admission capacity.
    let sync = match &startup_mode {
        StartupMode::Ready(services) => services.sync.clone(),
        StartupMode::RecoveryRequired { .. } => crate::sync_boundary::SyncBoundary::open(),
    };
    // The one degradation record for this process. A runtime that started
    // ready carries the registry `initialize_services_at` filled in; a runtime in
    // `RecoveryRequired` never got that far, so it has an empty one and its
    // `RecoveryRequired` lifecycle already reports the honest state.
    let config_degradation = match &startup_mode {
        StartupMode::Ready(services) => Arc::clone(&services.config_degradation),
        StartupMode::RecoveryRequired { .. } => {
            config_degradation::SharedConfigDegradation::default()
        }
    };
    RuntimeState {
        options,
        release,
        expected_parent,
        authenticated_channel: Mutex::new(AuthenticatedChannelState {
            bootstrap_secret: Some(bootstrap_secret),
            peer_identity: None,
            peer_handshake: None,
            active_channel_id: None,
            connection_epoch: 0,
            transcript_hash: None,
        }),
        command_outcomes: Arc::clone(&command_outcomes),
        terminal_openings: Default::default(),
        startup_sequence: AtomicU64::new(0),
        heartbeat_sequence: AtomicU64::new(0),
        core_health_revision: AtomicU64::new(1),
        config_degradation,
        workspace_session_revision: AtomicU64::new(1),
        workspace_files_revision: AtomicU64::new(1),
        ready: AtomicBool::new(false),
        startup_complete: AtomicBool::new(false),
        startup_mode,
        sync,
        settlements: crate::command_settlement::CommandSettlements::new(
            runtime_instance_id,
            command_outcomes,
        ),
        post_execution: crate::broker::completion::PostExecutionOwners::new(runtime_instance_id),
        disconnect_injected: AtomicBool::new(false),
        reconnect_delay_injected: AtomicBool::new(false),
        rejected_connection_log_at_ms: AtomicU64::new(0),
        #[cfg(feature = "p08-contract-driver")]
        p08_contract_driver_consumed: AtomicBool::new(false),
    }
}

pub async fn run_core(options: CoreOptions) -> Result<()> {
    core_log::start();
    let bootstrap_secret = read_core_bootstrap_secret()
        .context("failed to consume inherited Main/Core bootstrap secret")?;
    append_startup_stage("bootstrap_secret_read");
    let release = load_release_identity(&options)?;
    options.validate_release_namespace(&release.release_descriptor.bundle_namespace)?;
    append_startup_stage("release_identity_loaded");
    let expected_parent = retain_process(options.parent_process_id)
        .context("failed to retain the expected Electron Main process")?;
    let startup_mode = initialize_runtime_services(&options, &release).await;
    append_startup_stage("runtime_services_initialized");
    let state = Arc::new(new_runtime_state(
        options,
        release,
        expected_parent,
        bootstrap_secret,
        startup_mode,
    ));
    append_core_log(
        "core_started",
        json!({
            "runtime_instance_id": state.options.runtime_instance_id,
            "bundle_namespace": state.release.release_descriptor.bundle_namespace,
            "release_bundle_id": state.release.release_descriptor.identity.release_bundle_id,
            "safe_mode": state.options.safe_mode,
        }),
    );
    if let StartupMode::RecoveryRequired { reason } = &state.startup_mode {
        append_core_log(
            "core_recovery_required",
            json!({ "reason": reason, "agent_runtime_enabled": false }),
        );
    }

    loop {
        if !state
            .expected_parent
            .is_running()
            .context("failed to wait on the authenticated Electron Main process")?
        {
            return Err(PairingInvalidated("Electron Main process exited".to_owned()).into());
        }
        if state.options.test_mode == CoreTestMode::DisconnectAfterReady
            && state.disconnect_injected.load(Ordering::Acquire)
            && !state.reconnect_delay_injected.swap(true, Ordering::AcqRel)
        {
            time::sleep(Duration::from_millis(500)).await;
        }
        let server = create_secure_byte_pipe_server(&state.options.pipe_name, true)
            .context("failed to create secure Aura Named Pipe")?;
        append_core_log("pipe_listening", json!({ "pipe_scope": "local_logon" }));
        append_startup_stage("pipe_listening");
        server.connect().await.context("failed to accept Electron Main pipe connection")?;
        let actual_parent = named_pipe_client_identity(&server)
            .context("failed to identify the connected Named Pipe client")?;
        let expected_parent = state.expected_parent.identity();
        if actual_parent.process_id != expected_parent.process_id {
            if should_log_rejected_connection(&state) {
                append_core_log(
                    "pipe_client_rejected",
                    json!({
                        "reason": "unexpected_client_process",
                        "actual_process_id": actual_parent.process_id,
                    }),
                );
            }
            drop(server);
            continue;
        }
        if !same_process_identity(&actual_parent, expected_parent) {
            return Err(PairingInvalidated(
                "Electron Main PID was reused or its frozen process identity changed".to_owned(),
            )
            .into());
        }
        append_core_log(
            "pipe_client_accepted",
            json!({ "process_id": state.options.parent_process_id }),
        );
        let served = serve_connection(Arc::clone(&state), server, actual_parent).await;
        if let Err(ref error) = served {
            append_core_log("pipe_connection_closed", json!({ "reason": error.to_string() }));
            if error.downcast_ref::<PairingInvalidated>().is_some() {
                return served;
            }
        }
    }
}

fn same_process_identity(left: &ProcessIdentity, right: &ProcessIdentity) -> bool {
    left.process_id == right.process_id
        && left.creation_time_100ns == right.creation_time_100ns
        && left.windows_session_id == right.windows_session_id
        && left
            .executable_path
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.executable_path.to_string_lossy())
}

fn should_log_rejected_connection(state: &RuntimeState) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64);
    let previous = state.rejected_connection_log_at_ms.load(Ordering::Acquire);
    now.saturating_sub(previous) >= 5_000
        && state
            .rejected_connection_log_at_ms
            .compare_exchange(previous, now, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
}

fn read_core_bootstrap_secret() -> Result<Zeroizing<[u8; 32]>> {
    let mut secret = Zeroizing::new([0_u8; 32]);
    let mut input = std::io::stdin().lock();
    input.read_exact(secret.as_mut())?;
    let mut trailing = [0_u8; 1];
    if input.read(&mut trailing)? != 0 {
        secret.zeroize();
        bail!("bootstrap input contained trailing bytes");
    }
    Ok(secret)
}

fn load_release_identity(options: &CoreOptions) -> Result<CoreReleaseContext> {
    let manifest_path = fs::canonicalize(&options.component_manifest_path).with_context(|| {
        format!("failed to resolve {}", options.component_manifest_path.display())
    })?;
    let manifest_bytes = fs::read(&manifest_path)
        .with_context(|| format!("failed to read {}", options.component_manifest_path.display()))?;
    let manifest: BuildComponentManifest = serde_json::from_slice(&manifest_bytes)
        .context("component manifest is not a valid Aura contract")?;
    if manifest.bundle_namespace.trust_domain().is_none() {
        bail!("unsupported Aura release bundle namespace");
    }
    let core = manifest
        .components
        .iter()
        .find(|component| component.kind == BuildComponentKind::Core)
        .context("component manifest has no Core component")?;
    let elevated_helper = manifest
        .components
        .iter()
        .find(|component| component.kind == BuildComponentKind::ElevatedHelper)
        .context("component manifest has no Elevated Helper component")?;
    let sandbox_host = manifest
        .components
        .iter()
        .find(|component| component.kind == BuildComponentKind::SandboxHost)
        .context("component manifest has no Sandbox Host component")?;
    let egress_proxy = manifest
        .components
        .iter()
        .find(|component| component.kind == BuildComponentKind::EgressProxy)
        .context("component manifest has no Egress Proxy component")?;
    if let Some(protocol) = core.protocol_version
        && protocol.major != ContractVersion::CURRENT.major
    {
        bail!("Core manifest protocol major is incompatible");
    }
    if let Some(protocol) = elevated_helper.protocol_version
        && protocol.major != ContractVersion::CURRENT.major
    {
        bail!("Elevated Helper manifest protocol major is incompatible");
    }
    let executable = env::current_exe().context("failed to resolve aura-core.exe")?;
    let executable_bytes = fs::read(&executable).context("failed to read aura-core.exe")?;
    let actual = match core.content_hash.algorithm {
        ContentHashAlgorithm::Sha256 => format!("{:x}", Sha256::digest(&executable_bytes)),
        ContentHashAlgorithm::Blake3 => blake3::hash(&executable_bytes).to_hex().to_string(),
    };
    if actual != core.content_hash.hex && !options.allow_dev_component_mismatch {
        bail!("aura-core.exe does not match its Release Bundle manifest");
    }
    if actual != core.content_hash.hex {
        append_core_log(
            "dev_component_mismatch_allowed",
            json!({ "component": "aura-core", "severity": "warning" }),
        );
    }
    let manifest_directory =
        manifest_path.parent().context("component manifest path has no parent")?.to_path_buf();
    let executable_directory = executable.parent().context("aura-core.exe path has no parent")?;
    // Where the Development Release Bundle's own components live.
    //
    // A published Core runs from the execution installation root, and that
    // installation deliberately publishes only the execution objects (Core, the
    // command runner, the helper and the maintainer). The Sandbox Host, the Egress
    // Proxy and the Elevated Helper are *desktop* components: they ship beside the
    // manifest in the Release Bundle and are never copied into the installation.
    // Resolving them as siblings of `aura-core.exe` therefore worked only while the
    // development Core was also the Release Bundle Core. Once the development entry
    // selects a published Core, the siblings are simply not there.
    //
    // The anchor is explicit rather than inferred: `AURA_DEV_COMPONENT_ROOT` names
    // the bundle directory the development entry assembled. This chooses a location,
    // never a content — the launch path still verifies each component's hash against
    // the manifest, and the manifest's protocol check is unchanged, so a wrong or
    // stale directory fails exactly like a wrong file would. When the variable is
    // absent the previous sibling behaviour is kept, which is what the packaged
    // product and every existing test rely on.
    let development_component_root = if !options.allow_dev_component_mismatch {
        None
    } else {
        match std::env::var_os(DEV_COMPONENT_ROOT_ENVIRONMENT) {
            Some(value) if !value.is_empty() => {
                let root = PathBuf::from(value);
                if !root.is_absolute() {
                    bail!("{DEV_COMPONENT_ROOT_ENVIRONMENT} must be an absolute path");
                }
                if !root.is_dir() {
                    bail!("{DEV_COMPONENT_ROOT_ENVIRONMENT} must name an existing directory");
                }
                Some(root)
            }
            _ => Some(executable_directory.to_path_buf()),
        }
    };
    let elevated_helper_executable = match &development_component_root {
        Some(root) => root.join("aura-elevated-helper.exe"),
        None => safe_release_component_path(&manifest_directory, &elevated_helper.relative_path)?,
    };
    let resolve_sidecar = |component: &aura_runtime_contract::BuildComponent,
                           development_name: &str,
                           process_kind: RuntimeProcessKind|
     -> Result<SidecarReleaseIdentity> {
        let component_executable = match &development_component_root {
            Some(root) => root.join(development_name),
            None => safe_release_component_path(&manifest_directory, &component.relative_path)?,
        };
        Ok(SidecarReleaseIdentity {
            process_kind,
            component_version: component.version.clone(),
            executable: component_executable,
            content_hash: component.content_hash.clone(),
            protocol_version: component.protocol_version,
        })
    };
    let release_descriptor = ReleaseDescriptor {
        bundle_namespace: manifest.bundle_namespace.clone(),
        identity: ReleaseIdentity {
            release_bundle_id: manifest.release_bundle_id,
            component_manifest_hash: ContentHash::new(
                ContentHashAlgorithm::Sha256,
                format!("{:x}", Sha256::digest(&manifest_bytes)),
            )?,
        },
    };
    Ok(CoreReleaseContext {
        release_descriptor,
        component_version: core.version.clone(),
        build_commit: manifest.build_commit.clone(),
        manifest_path,
        elevated_helper_executable,
        elevated_helper_hash: elevated_helper.content_hash.clone(),
        sandbox_host: resolve_sidecar(
            sandbox_host,
            "aura-sandbox-host.exe",
            RuntimeProcessKind::SandboxHost,
        )?,
        egress_proxy: resolve_sidecar(
            egress_proxy,
            "aura-egress-proxy.exe",
            RuntimeProcessKind::EgressProxy,
        )?,
    })
}

fn safe_release_component_path(root: &Path, relative: &str) -> Result<PathBuf> {
    let candidate = Path::new(relative);
    if candidate.is_absolute()
        || !candidate.components().all(|component| matches!(component, Component::Normal(_)))
    {
        bail!("component manifest contains an unsafe relative path: {relative}");
    }
    Ok(root.join(candidate))
}

async fn serve_connection(
    state: Arc<RuntimeState>,
    mut server: NamedPipeServer,
    actual_parent: ProcessIdentity,
) -> Result<()> {
    let channel = authenticate_connection(&state, &mut server, &actual_parent).await?;
    if state.options.test_mode == CoreTestMode::CrashAfterHandshake {
        process::exit(91);
    }

    let (reader, writer) = tokio::io::split(server);
    let queue = AsyncPriorityQueue::new();
    let subscriptions = Arc::new(ProjectionSubscriptions::default());
    let writer_task = spawn_writer(writer, queue.clone(), channel);
    let heartbeat_task = spawn_heartbeat(Arc::clone(&state), queue.clone());
    let startup_task = spawn_startup(Arc::clone(&state), queue.clone());
    let projection_task =
        spawn_projection_updates(Arc::clone(&state), queue.clone(), Arc::clone(&subscriptions))?;
    let ledger = Arc::new(std::sync::Mutex::new(RequestLedger::new()));
    let mut pending_secret_registry = PendingSecretInputRegistry::new();
    let mut pending_secret_commands = BTreeMap::<CommandInvocationId, JsonRpcRequest>::new();
    install_search_flight_reporter();
    let mut frame_reader = AuthenticatedFrameReader::new(reader);
    let mut next_inbound_sequence = 2_u64;
    // Bounded execution lanes. Handlers never run inside this loop, and a burst
    // is answered with an overload error instead of queueing without limit.
    let mut session = ipc_session::IpcSession::new();
    // Searches this connection started, so disconnect cleanup cancels only this
    // connection's flights and leaves other connections' work alone.
    let mut search_ids = BTreeSet::<CorrelationId>::new();
    // Side-effecting commands this connection is executing, so the atomic
    // CommandInvocationId claim is released exactly when they settle.
    let mut command_flights = CommandFlights::new();
    // The wait channel between this connection's handlers and its lanes. A handler that enters the
    // real common admission and finds its resource held reports it here, and the loop below frees
    // that request's execution slot while it waits — the separation that keeps related waiters from
    // exhausting the connection.
    let (wait_handlers, mut wait_signals) = ipc_session::ResourceWaitSignals::channel();

    let read_result: Result<()> = tokio::select! {
        result = async {
            loop {
                // Reap finished handlers: the reply was already queued by the
                // handler itself, so this closes the request terminal and
                // releases this connection's registrations for it.
                let undeliverable = finish_handled_requests(
                    &state,
                    &queue,
                    &ledger,
                    &mut search_ids,
                    &mut command_flights,
                    session.poll_completions(),
                )?;
                // Apply every wait report a handler has made since the last turn, so a request that
                // is really waiting stops holding an execution slot in the same turn it said so.
                session.apply_wait_signals(wait_signals.drain());
                if !undeliverable.is_empty() {
                    // A panicked handler's reply is undefined, and a failed
                    // channel cannot deliver one: retiring the session is
                    // honest, waiting out Main's timeout is not.
                    bail!(
                        "{} request handler(s) could not answer; retiring the session",
                        undeliverable.len()
                    );
                }
                // Start whatever the lanes have room for now, including work that
                // was queued while the connection was busy.
                spawn_ready_handlers_reporting(
                    &state,
                    &queue,
                    &subscriptions,
                    &ledger,
                    &command_flights,
                    &mut session,
                    &wait_handlers,
                )?;
                // The outbound writer is the only path that can deliver a
                // reply. Once it stops, this session can never answer again, so
                // stop reading and let the channel be torn down instead of
                // accepting requests that can never be answered.
                if writer_task.is_finished() {
                    append_core_log(
                        "session_writer_stopped",
                        json!({ "reason": "outbound_writer_finished" }),
                    );
                    bail!("outbound writer stopped; the channel can no longer deliver replies");
                }
                let wait_for_search = session.is_busy();
                // The incremental reader keeps a partially read frame in its own
                // state, so a timeout that fires mid-frame can be retried
                // without desynchronising the stream.
                let authenticated = if pending_secret_registry.is_empty() && !wait_for_search {
                    match frame_reader.next_frame().await {
                        FrameRead::Frame(frame) => *frame,
                        FrameRead::Pending => continue,
                        FrameRead::Eof => bail!("IPC reader reached end of stream"),
                        FrameRead::Invalid(error) => return Err(error.into()),
                        FrameRead::Failed(error) => return Err(error.into()),
                    }
                } else {
                    match time::timeout(
                        Duration::from_millis(if wait_for_search { 50 } else { 250 }),
                        frame_reader.next_frame(),
                    )
                    .await
                    {
                        Ok(FrameRead::Frame(frame)) => *frame,
                        Ok(FrameRead::Pending) | Err(_) => {
                            if !wait_for_search {
                                expire_pending_secret_commands(
                                    &state,
                                    &queue,
                                    &ledger,
                                    &mut pending_secret_registry,
                                    &mut pending_secret_commands,
                                )
                                .await?;
                            }
                            continue;
                        }
                        Ok(FrameRead::Eof) => bail!("IPC reader reached end of stream"),
                        Ok(FrameRead::Invalid(error)) => return Err(error.into()),
                        Ok(FrameRead::Failed(error)) => return Err(error.into()),
                    }
                };
                if authenticated.header.connection_epoch < channel.connection_epoch {
                    continue;
                }
                if authenticated.header.channel_id != channel.channel_id
                    || authenticated.header.connection_epoch != channel.connection_epoch
                {
                    bail!("IPC frame did not belong to the active authenticated channel");
                }
                if authenticated.header.sequence != next_inbound_sequence {
                    bail!("IPC frame sequence was duplicate, reversed, or missing");
                }
                let inbound_sequence = authenticated.header.sequence;
                next_inbound_sequence = next_inbound_sequence
                    .checked_add(1)
                    .context("IPC inbound sequence overflow")?;
                match authenticated.frame {
                    WireFrame::Control(payload) => {
                        let message: JsonRpcMessage = serde_json::from_slice(&payload)
                            .context("invalid JSON-RPC control frame")?;
                        match message {
                            JsonRpcMessage::Request(request) => {
                                if request.jsonrpc != JSON_RPC_VERSION {
                                    bail!("unsupported JSON-RPC version");
                                }
                                aura_ipc::trace(
                                    "core.recv",
                                    &format!(
                                        "id={} method={} epoch={} seq={}",
                                        request.id,
                                        request.method,
                                        channel.connection_epoch,
                                        inbound_sequence
                                    ),
                                );
                                ledger
                                    .lock()
                                    .map_err(|_| anyhow::anyhow!("request ledger was poisoned"))?
                                    .begin(request.id)
                                    .context("duplicate request id")?;
                                if P05Services::handles_secret_main_command(&request.method) || terminal_secret::binding(&request.method).is_some() {
                                    let pending = prepare_pending_secret_command(
                                        &state,
                                        &queue,
                                        &request,
                                        channel,
                                    )
                                    .await?;
                                    match pending {
                                        Some(pending) => {
                                            let command_invocation_id =
                                                pending.command_invocation_id;
                                            pending_secret_registry
                                                .register(pending)
                                                .context("SecretInput command was already pending")?;
                                            if pending_secret_commands
                                                .insert(command_invocation_id, request)
                                                .is_some()
                                            {
                                                bail!("SecretInput command registration was duplicated");
                                            }
                                        }
                                        None => {
                                            ledger
                                                .lock()
                                                .map_err(|_| {
                                                    anyhow::anyhow!("request ledger was poisoned")
                                                })?
                                                .finish(request.id, IpcRequestTerminal::Failed)
                                                .context("request terminal was not unique")?;
                                        }
                                    }
                                    continue;
                                }
                                // Claim the CommandInvocationId before dispatch.
                                // The completed-outcome check inside the handler
                                // only protects ids that already finished, so a
                                // replay arriving while the first attempt runs
                                // could execute the same side effect twice.
                                let reserved_command = match reserve_command_invocation(
                                    &state,
                                    &queue,
                                    &request,
                                )
                                .await?
                                {
                                    CommandReservation::Granted(id) => id,
                                    CommandReservation::Refused => {
                                        ledger
                                            .lock()
                                            .map_err(|_| {
                                                anyhow::anyhow!("request ledger was poisoned")
                                            })?
                                            .finish(request.id, IpcRequestTerminal::Failed)
                                            .context("request terminal was not unique")?;
                                        continue;
                                    }
                                };
                                // Bound the connection's work *before* anything
                                // assumes this request will run. The answer for a
                                // refused request is written without waiting for
                                // queue space, so the reader keeps serving
                                // cancellation and connection control.
                                if let ipc_session::Admission::Overloaded { lane } =
                                    session.admit(request.clone())
                                {
                                    aura_ipc::trace(
                                        "core.handler.overloaded",
                                        &format!(
                                            "id={} method={} lane={lane:?}",
                                            request.id, request.method
                                        ),
                                    );
                                    let error =
                                        ipc_session::Admission::overloaded_error(request.id, lane);
                                    send_control_answer(
                                        &queue,
                                        &JsonRpcResponse::failure(request.id, error),
                                    )?;
                                    if let Some(claim) = reserved_command
                                        && let Ok(mut outcomes) = state.command_outcomes.lock()
                                    {
                                        // Never started: nothing could have happened.
                                        outcomes.abandon(&claim);
                                    }
                                    ledger
                                        .lock()
                                        .map_err(|_| {
                                            anyhow::anyhow!("request ledger was poisoned")
                                        })?
                                        .finish(request.id, IpcRequestTerminal::Failed)
                                        .context("request terminal was not unique")?;
                                    continue;
                                }
                                if request.method == method::FS_SEARCH_TEXT {
                                    // The route cancels through the request
                                    // ledger, so its handle must exist before the
                                    // handler starts.
                                    aura_fs_tools::register_search(
                                        request.id,
                                        aura_fs_tools::SearchCancel::new(),
                                    );
                                    search_ids.insert(request.id);
                                }
                                if let Some(claim) = reserved_command {
                                    command_flights.insert(request.id, claim);
                                }
                                spawn_ready_handlers_reporting(
                                    &state,
                                    &queue,
                                    &subscriptions,
                                    &ledger,
                                    &command_flights,
                                    &mut session,
                                    &wait_handlers,
                                )?;
                            }
                            JsonRpcMessage::Notification(_) => {
                                bail!("Electron Main sent an unsupported control notification");
                            }
                            JsonRpcMessage::Response(_) => {
                                bail!("Electron Main sent an unsolicited JSON-RPC response");
                            }
                        }
                    }
                    WireFrame::SecretInput(frame) => {
                        let command_invocation_id = frame.command_invocation_id;
                        let secret = pending_secret_registry
                            .consume(
                                channel.channel_id,
                                channel.connection_epoch,
                                frame,
                                Instant::now(),
                            )
                            .context("SecretInput did not match its pending command")?;
                        let request = pending_secret_commands
                            .remove(&command_invocation_id)
                            .context("SecretInput command metadata was missing")?;
                        let claim = match reserve_command_invocation(&state,&queue,&request).await? {
                            CommandReservation::Granted(claim) => claim,
                            CommandReservation::Refused => {
                                ledger.lock().map_err(|_| anyhow::anyhow!("request ledger was poisoned"))?.finish(request.id,IpcRequestTerminal::Failed)?;
                                continue;
                            }
                        };
                        if let ipc_session::Admission::Overloaded {lane} = session.admit_secret(request.clone(),secret) {
                            send_control_answer(&queue,&JsonRpcResponse::failure(request.id,ipc_session::Admission::overloaded_error(request.id,lane)))?;
                            if let Some(claim) = claim && let Ok(mut outcomes) = state.command_outcomes.lock() {outcomes.abandon(&claim);}
                            ledger.lock().map_err(|_| anyhow::anyhow!("request ledger was poisoned"))?.finish(request.id,IpcRequestTerminal::Failed)?;
                        } else if let Some(claim) = claim {command_flights.insert(request.id,claim);}
                    }
                    WireFrame::BinaryChunk { .. } => {
                        // Retired Tool Draft snapshot uploads, the only binary stream.
                        anyhow::bail!("Core has no binary stream receiver");
                    }
                    WireFrame::ToolRuntimeSecret(_) => {
                        bail!("Tool runtime secret frame arrived on the Renderer channel");
                    }
                    WireFrame::ToolConformanceRuntimeSecret(_) => {
                        bail!(
                            "Tool Conformance runtime secret frame arrived on the Renderer channel"
                        );
                    }
                }
            }
        } => result,
        result = async {
            if state.options.test_mode == CoreTestMode::DisconnectAfterReady
                && !state.disconnect_injected.swap(true, Ordering::AcqRel)
            {
                while !state.ready.load(Ordering::Acquire) {
                    time::sleep(Duration::from_millis(10)).await;
                }
                time::sleep(Duration::from_millis(250)).await;
                bail!("injected Named Pipe disconnect after Core Ready");
            }
            std::future::pending::<Result<()>>().await
        } => result,
    };

    // Disconnect cleanup. Three different facts must stay separate:
    //
    // * the connection is gone (this function);
    // * the coordinator returned a result or an error (the handler completing);
    // * every piece of admitted scan work really ended (the search's own
    //   lifecycle).
    //
    // Only the third releases a search registration.
    let ConnectionReclaim { started, .. } = reclaim_connection_work(
        &state,
        &queue,
        &ledger,
        &mut session,
        &mut search_ids,
        &mut command_flights,
    )
    .await;
    // Settle the CommandInvocationId claims this connection held. A command that
    // ran without recording an outcome becomes an explicit `Unknown` — its
    // business result is undetermined, and a replay must conflict rather than
    // execute a second time. One that never started is simply released.
    for (request_id, claim) in std::mem::take(&mut command_flights) {
        let settlement = if started.contains(&request_id) {
            CommandSettlement::Ran
        } else {
            CommandSettlement::NeverRan
        };
        settle_command_claim(&state, &claim, settlement);
    }
    if let Ok(mut ledger) = ledger.lock() {
        // The *request* is transport-lost; that is not a business outcome, and
        // the command ledger above stays the place that says what happened to the
        // command itself.
        let _ = ledger.finish_all_running(IpcRequestTerminal::TransportLost);
    }
    queue.close();
    heartbeat_task.abort();
    startup_task.abort();
    projection_task.abort();
    let _ = writer_task.await;
    read_result
}

/// What one connection's teardown established.
pub(crate) struct ConnectionReclaim {
    /// Searches whose admitted work was still running when the bounded wait
    /// expired. Their registrations are now owned by the registry's sweeper.
    ///
    /// Read by the disconnect integration test, which asserts on what the wait
    /// proved rather than on the log line alone.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) still_running: Vec<CorrelationId>,
    /// Requests that really took a lane, i.e. whose handler may have had an
    /// effect. Everything outside this set never left the queue.
    pub(crate) started: BTreeSet<CorrelationId>,
}

/// Everything a connection's teardown must do about its searches, in order:
///
/// 1. cancel this connection's searches;
/// 2. release the ones whose request never started — nothing was admitted, so
///    there is no work to wait for;
/// 3. wait, bounded, for the running ones to really finish; a bounded wait that
///    expires ends *this* wait, not the work;
/// 4. abort the handler tasks (their replies can no longer be delivered);
/// 5. hand what is still running to the registry's own sweeper, which keeps the
///    entry, watches for the real completion and reports work that never ends
///    instead of dropping it.
///
/// This is the real disconnect path, not a test stand-in: `serve_connection`
/// calls it, and so does the integration test that drives a real search worker
/// through a real connection teardown.
pub(crate) async fn reclaim_connection_work(
    state: &Arc<RuntimeState>,
    queue: &AsyncPriorityQueue,
    ledger: &Arc<std::sync::Mutex<RequestLedger>>,
    session: &mut ipc_session::IpcSession,
    search_ids: &mut BTreeSet<CorrelationId>,
    command_flights: &mut CommandFlights,
) -> ConnectionReclaim {
    if state.terminal_openings.reclaim(ledger).is_err() {
        append_core_log(
            "terminal_opening_reclaim_unavailable",
            json!({"reason":"opening_registry_unavailable"}),
        );
    }
    for request_id in search_ids.iter() {
        aura_fs_tools::cancel_search(*request_id);
    }
    // Requests still queued never started, so nothing of theirs can be running:
    // their registrations go now instead of waiting on a scan that never began.
    // `release_unstarted_search` refuses anything with admitted work or a
    // coordinator run, so this cannot drop real work.
    let started: BTreeSet<CorrelationId> = session.running_request_ids().into_iter().collect();
    // Owned Broker work may outlive the handler. Stop admission-only waits before retiring
    // this connection; an effect already in flight retains its settlement responsibility.
    for request_id in &started {
        aura_fs_tools::cancel_resource_wait(*request_id);
    }
    let mut kept = BTreeSet::new();
    for request_id in std::mem::take(search_ids) {
        if started.contains(&request_id)
            || !aura_fs_tools::release_unstarted_search(request_id, aura_fs_tools::NeverStarted)
        {
            // Either the request really took a lane, or the registry refused to
            // treat it as never-started — a handler can return while its worker is
            // still inside synchronous I/O, and then the session no longer lists
            // the request. Both mean this connection still owns the wait; dropping
            // the id here would abandon a live registration with nobody watching
            // it.
            kept.insert(request_id);
        }
    }
    *search_ids = kept;
    let reclaim_deadline = time::Instant::now() + SEARCH_RECLAIM_TIMEOUT;
    let mut still_running: Vec<CorrelationId> = Vec::new();
    while !search_ids.is_empty() || session.is_busy() {
        // Handlers that finish while the scans unwind still close their request
        // terminal here.
        let _ = finish_handled_requests(
            state,
            queue,
            ledger,
            search_ids,
            command_flights,
            session.poll_completions(),
        );
        // Release only what really finished; everything else stays registered.
        search_ids.retain(|request_id| !aura_fs_tools::release_search_flight(*request_id));
        if search_ids.is_empty() && !session.is_busy() {
            break;
        }
        if time::Instant::now() >= reclaim_deadline {
            still_running = search_ids.iter().copied().collect();
            append_core_log(
                "session_reclaim_timeout",
                json!({
                    "reason": "scan work did not finish inside the connection cleanup budget",
                    "searches": still_running.len(),
                    "running": session.running_count(),
                    "queued": session.queued_count(),
                    "owner_after_timeout": "search_flight_registry",
                }),
            );
            break;
        }
        time::sleep(Duration::from_millis(10)).await;
    }
    // Abort handler tasks whose reply can no longer be delivered; the ledger
    // records their terminal as transport lost. The wait is bounded, because
    // `abort()` cannot interrupt a handler parked in synchronous work — a host API
    // or a persistence write — and awaiting one without a budget is exactly how a
    // teardown outlived the reclaim budget while the read loop waited for it.
    let detached =
        detached_handlers::detach(session.shutdown_within(HANDLER_RECLAIM_TIMEOUT).await);
    if detached > 0 {
        // The work is still real, so an explicit owner keeps it: one watcher per
        // handler, released when the handler really ends. No reply is re-sent, no
        // command is released, and no result is reported as no-effect, so a late
        // completion cannot leak into a later connection.
        append_core_log(
            "session_handler_handover",
            json!({
                "reason": "handler tasks did not stop inside the connection cleanup budget",
                "handed_over": detached,
                "live": detached_handlers::live(),
                "total": detached_handlers::total(),
                "owner_after_timeout": "detached_handler_watcher",
            }),
        );
    }
    if !still_running.is_empty() {
        // The work is still real, so the registry keeps it: its sweeper watches
        // for the actual completion and releases the entry then. No thread is
        // started per connection — the registry has exactly one sweeper, and it
        // only ever observes searches it owns.
        aura_fs_tools::orphan_search(still_running.iter().copied());
        append_core_log(
            "search_flight_handover",
            json!({
                "searches": still_running.len(),
                "owner": "search_flight_registry_sweeper",
            }),
        );
    }
    ConnectionReclaim { still_running, started }
}

/// Start the process-wide sink for search flights that never report finished.
///
/// The registry owns flight lifetime; the host process owns the log. Wiring them
/// once here keeps the platform module from knowing about Core logging.
fn install_search_flight_reporter() {
    aura_fs_tools::set_orphan_reporter(|request_id, waited| {
        append_core_log(
            "search_flight_orphaned",
            json!({
                "request_id": request_id.to_string(),
                "reason": "scan work has not reported finished; the registration is kept",
                "waited_seconds": waited.as_secs(),
                "owner": "search_flight_registry_sweeper",
            }),
        );
    });
}

/// Start every accepted request that has a free lane.
///
/// Called from the read loop and right after a completion, so work that was
/// queued while the connection was busy starts as soon as a lane frees up. The
/// read loop never runs a handler body itself.
///
/// `command_flights` carries the claims the read loop already took for this
/// connection's requests. A handler has to know which claim is *its own*: the
/// duplicate check inside the handler would otherwise read the claim that bought
/// its request the right to run as proof that someone else is running it.
///
/// Fails when the ledger cannot record that a handler started; the connection then
/// retires rather than running work whose cancellation state it cannot describe.
/// The test form of [`spawn_ready_handlers_reporting`], for a component that starts real handlers but
/// does not observe their resource waits.
///
/// It still gives every handler a real channel rather than a missing one, so a handler's own
/// `entered`/`left` calls always have somewhere to go; only the observation is absent.
#[cfg(test)]
fn spawn_ready_handlers(
    state: &Arc<RuntimeState>,
    queue: &AsyncPriorityQueue,
    subscriptions: &Arc<ProjectionSubscriptions>,
    ledger: &Arc<std::sync::Mutex<RequestLedger>>,
    command_flights: &CommandFlights,
    session: &mut ipc_session::IpcSession,
) -> Result<()> {
    let (wait_handlers, _wait_signals) = ipc_session::ResourceWaitSignals::channel();
    spawn_ready_handlers_reporting(
        state,
        queue,
        subscriptions,
        ledger,
        command_flights,
        session,
        &wait_handlers,
    )
}

/// The production form of [`spawn_ready_handlers`], reporting each handler's resource waits through
/// the connection's own channel.
///
/// The channel is what makes the wait-ticket/execution-slot separation real: the handler says it is
/// waiting, and the read loop frees its slot. A caller that does not need to observe waits uses the
/// form above, which still gives every handler a real (unobserved) channel rather than a missing one
/// — so a handler's own `entered`/`left` calls always have somewhere to go.
fn spawn_ready_handlers_reporting(
    state: &Arc<RuntimeState>,
    queue: &AsyncPriorityQueue,
    subscriptions: &Arc<ProjectionSubscriptions>,
    ledger: &Arc<std::sync::Mutex<RequestLedger>>,
    command_flights: &CommandFlights,
    session: &mut ipc_session::IpcSession,
    wait_signals: &ipc_session::ResourceWaitHandler,
) -> Result<()> {
    for admitted in session.take_ready() {
        let lane = admitted.lane();
        let (ticket, request) = admitted.into_parts();
        aura_ipc::trace(
            "core.handler.start",
            &format!("id={} method={} lane={lane:?}", request.id, request.method),
        );
        // The request starts *here*: a cancellation that arrives after this line
        // names a running handler, one that arrives before it names queued work.
        // Recording the transition is what lets both be told apart, because the
        // wire answer cannot carry that difference.
        ledger
            .lock()
            .map_err(|_| anyhow::anyhow!("request ledger was poisoned"))?
            .mark_started(request.id)
            .context("a handler started for a request that was not waiting to run")?;
        let owned_claim = command_flights.get(&request.id).cloned();
        let secret = session.take_secret(request.id);
        let handler_state = Arc::clone(state);
        let handler_queue = queue.clone();
        let handler_subscriptions = Arc::clone(subscriptions);
        let handler_ledger = Arc::clone(ledger);
        // The **wait ticket** this handler reports through. It is deliberately separate from the
        // execution slot: when the handler enters the real common admission and finds its resource
        // held, it says so, the read loop frees this request's slot, and unrelated work on the same
        // connection proceeds. The handler keeps running — it is parked inside its own wait, which is
        // where its deadline and cancellation live.
        let handler_waits = wait_signals.for_ticket(ticket);
        let handle = tokio::spawn(async move {
            if let Some(secret) = secret {
                return match handle_secret_request(
                    &handler_state,
                    &handler_queue,
                    &request,
                    &secret,
                )
                .await
                {
                    Ok(terminal) => ipc_session::HandlerTerminal::Answered(terminal),
                    Err(error) if is_channel_failure(&error) => {
                        ipc_session::HandlerTerminal::TransportFailed
                    }
                    Err(_) => ipc_session::HandlerTerminal::Unreplied,
                };
            }
            run_handler(
                &handler_state,
                &handler_queue,
                &handler_subscriptions,
                &handler_ledger,
                &request,
                owned_claim,
                &handler_waits,
            )
            .await
        });
        session.attach(ticket, handle);
    }
    Ok(())
}

/// Start every accepted request with a caller-supplied body.
///
/// The production [`spawn_ready_handlers`] always runs the real handler, which is what
/// an ordinary test wants. A lifecycle regression needs the opposite: a handler that is
/// deliberately parked in synchronous work, so it can prove what a teardown does while
/// the work cannot stop. Only the body differs — the lanes, the tickets and the
/// teardown are the production ones.
#[cfg(test)]
fn spawn_ready_handlers_with<B, F>(session: &mut ipc_session::IpcSession, body: B)
where
    B: Fn() -> F,
    F: std::future::Future<Output = ipc_session::HandlerTerminal> + Send + 'static,
{
    for admitted in session.take_ready() {
        let (ticket, _request) = admitted.into_parts();
        session.attach(ticket, tokio::spawn(body()));
    }
}

/// Run one request handler to its terminal. The reply is queued here; the
/// request terminal is closed by the read loop, so exactly one place owns it.
async fn run_handler(
    state: &Arc<RuntimeState>,
    queue: &AsyncPriorityQueue,
    subscriptions: &Arc<ProjectionSubscriptions>,
    ledger: &Arc<std::sync::Mutex<RequestLedger>>,
    request: &JsonRpcRequest,
    owned_claim: Option<CommandClaim>,
    waits: &ipc_session::ResourceWaitHandler,
) -> ipc_session::HandlerTerminal {
    // The handler's own wait channel, reachable from the synchronous P03 dispatcher without
    // threading a parameter through every route. Scoped to this task, so one request's handler can
    // never report a wait for another request's ticket.
    let handled = ipc_session::RESOURCE_WAIT_HANDLER
        .scope(waits.clone(), async {
            if request.method == method::FS_SEARCH_TEXT {
                routes::workspace(state, queue, request).await
            } else {
                handle_request(state, queue, subscriptions, ledger, request, owned_claim.as_ref())
                    .await
            }
        })
        .await;
    let terminal = match handled {
        Ok(terminal) => ipc_session::HandlerTerminal::Answered(terminal),
        Err(error) => {
            aura_ipc::trace(
                "core.handler.error",
                &format!("id={} method={} error={error}", request.id, request.method),
            );
            if is_channel_failure(&error) {
                ipc_session::HandlerTerminal::TransportFailed
            } else {
                ipc_session::HandlerTerminal::Unreplied
            }
        }
    };
    aura_ipc::trace(
        "core.handled",
        &format!("id={} method={} terminal={terminal:?}", request.id, request.method),
    );
    terminal
}

/// Whether a handler failure came from the channel rather than from the request.
///
/// This decides who owns the reply: a broken channel is retired, while an
/// application failure still owes the peer one typed error.
fn is_channel_failure(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause.downcast_ref::<aura_ipc::QueuePushError>().is_some()
            || cause.downcast_ref::<aura_ipc::QueueClosed>().is_some()
    })
}

/// The typed error for a request whose handler failed before answering.
fn handler_failed_error(request_id: CorrelationId) -> Result<IpcErrorEnvelope> {
    Ok(IpcErrorEnvelope {
        code: ErrorCode::new("internal.unexpected")?,
        category: ErrorCategory::Internal,
        message: "Aura Core 处理这次请求时出错，没有产生结果；请重试。".to_owned(),
        retryable: false,
        correlation_id: request_id,
        terminal: IpcRequestTerminal::Failed,
        details: BTreeMap::new(),
    })
}
/// Close the book on handlers that finished.
///
/// One request terminal per request, and only this connection's registrations
/// released: the search registry and the command ledger are process-global, so
/// cleanup must never reach another connection's work.
///
/// Reply ownership is decided here, in one place: a handler that answered needs
/// nothing, one that failed before answering gets exactly one typed error, and a
/// handler whose channel failed is reported so the caller retires the session
/// instead of waiting out a timeout.
fn finish_handled_requests(
    state: &RuntimeState,
    queue: &AsyncPriorityQueue,
    ledger: &Arc<std::sync::Mutex<RequestLedger>>,
    search_ids: &mut BTreeSet<CorrelationId>,
    command_flights: &mut CommandFlights,
    completions: Vec<(CorrelationId, ipc_session::HandlerTerminal)>,
) -> Result<Vec<CorrelationId>> {
    let mut unreplied = Vec::new();
    for (request_id, terminal) in completions {
        aura_fs_tools::clear_resource_wait_cancel(request_id);
        if search_ids.contains(&request_id) && aura_fs_tools::release_search_flight(request_id) {
            // The handler returned *and* the scan's admitted work ended. A
            // handler that returned while a worker is still inside synchronous
            // I/O leaves the registration alone, so nothing here reports that
            // scan as reclaimed.
            search_ids.remove(&request_id);
        }
        if let Some(claim) = command_flights.remove(&request_id) {
            // A completion means the command *ran*. If its handler recorded a
            // confirmed outcome, the claim is already gone. If it did not — the
            // handler failed, panicked or answered with a transport error — the
            // business effect is unknown, so the claim stays and becomes an
            // explicit `Unknown` record: releasing it here would let a replay
            // execute the same command twice.
            settle_command_claim(state, &claim, CommandSettlement::Ran);
        }
        match terminal {
            ipc_session::HandlerTerminal::Answered(terminal) => {
                let cancellation = close_request_terminal(ledger, request_id, terminal)?;
                record_cancellation_settlement(request_id, cancellation);
            }
            ipc_session::HandlerTerminal::Unreplied => {
                try_send_json(
                    queue,
                    QueuePriority::P0,
                    &JsonRpcResponse::failure(request_id, handler_failed_error(request_id)?),
                )?;
                let cancellation =
                    close_request_terminal(ledger, request_id, IpcRequestTerminal::Failed)?;
                record_cancellation_settlement(request_id, cancellation);
            }
            ipc_session::HandlerTerminal::TransportFailed => unreplied.push(request_id),
            ipc_session::HandlerTerminal::Panicked => {
                // The reply is undefined and nothing was sent, so the caller
                // retires the session rather than pretending it answered.
                unreplied.push(request_id);
            }
        }
        state.terminal_openings.finish(request_id)?;
    }
    Ok(unreplied)
}

/// Close one request's single terminal in the ledger.
fn close_request_terminal(
    ledger: &Arc<std::sync::Mutex<RequestLedger>>,
    request_id: CorrelationId,
    terminal: IpcRequestTerminal,
) -> Result<CancellationSettlement> {
    ledger
        .lock()
        .map_err(|_| anyhow::anyhow!("request ledger was poisoned"))?
        .finish(request_id, terminal)
        .context("request terminal was not unique")
}

/// Record what a cancellation intent received for this request achieved.
///
/// The request terminal and the cancellation are two facts. A request whose
/// cancellation was accepted can still end in its own terminal — the handler ran
/// to completion and never looked at the intent — and that is recorded as
/// `NotHonoured` instead of being reported as a stop that never happened.
fn record_cancellation_settlement(request_id: CorrelationId, settlement: CancellationSettlement) {
    if settlement == CancellationSettlement::NotHonoured {
        aura_ipc::trace(
            "core.cancel.not_honoured",
            &format!("id={request_id} reason=the handler reached its own terminal anyway"),
        );
    }
}

/// Answer a request from the read loop itself.
///
/// The reader must never park, so a queue that cannot take this frame is treated
/// as a dead connection: the peer would otherwise wait for a reply that can no
/// longer be delivered.
fn send_control_answer(queue: &AsyncPriorityQueue, response: &JsonRpcResponse) -> Result<()> {
    try_send_json(queue, QueuePriority::P0, response)
}

/// Queue a frame without waiting for space.
///
/// Used by the branches the read loop answers directly (refusals, expirations,
/// secret-input replies). Parking the reader for queue space would starve
/// cancellation and connection control, so a full partition is an error the
/// caller turns into a connection failure.
fn try_send_json<T: Serialize>(
    queue: &AsyncPriorityQueue,
    priority: QueuePriority,
    value: &T,
) -> Result<()> {
    let payload = serde_json::to_vec(value)?;
    let bytes = payload.len();
    let id = trace_request_id(&payload);
    let kind = if id == "none" { "notification" } else { "response" };
    aura_ipc::trace(
        "core.enqueue",
        &format!("kind={kind} priority={priority:?} bytes={bytes} id={id}"),
    );
    queue
        .try_send(OutboundFrame { priority, frame: WireFrame::Control(payload) })
        .map_err(|error| anyhow::anyhow!("the connection could not be answered: {error}"))?;
    aura_ipc::trace("core.enqueued", &format!("kind={kind} id={id}"));
    Ok(())
}

/// Reader-side [send_recorded_response]: records the outcome the same way, but
/// never waits for queue space.
async fn try_send_recorded_response(
    state: &RuntimeState,
    queue: &AsyncPriorityQueue,
    request: &JsonRpcRequest,
    response: JsonRpcResponse,
) -> Result<()> {
    let response = response_with_size_fallback(response)?;
    record_command_outcome(state, request, &response)?;
    let priority = if response.result.is_some() { QueuePriority::P1 } else { QueuePriority::P0 };
    try_send_json(queue, priority, &response)
}

fn spawn_projection_updates(
    state: Arc<RuntimeState>,
    queue: AsyncPriorityQueue,
    subscriptions: Arc<ProjectionSubscriptions>,
) -> Result<JoinHandle<()>> {
    let StartupMode::Ready(services) = &state.startup_mode else {
        return Ok(tokio::spawn(async move {
            std::future::pending::<()>().await;
        }));
    };
    let workspace_session_updates = services
        .persistence
        .subscribe_workspace_sessions()
        .context("failed to subscribe to Workspace/Session projection updates")?;
    // The Workspace Files projection lives in the workspace runtime, which
    // is part of the tool-policy chain. When that capability is gone there is no
    // such projection to subscribe to; the publisher simply has nothing to send,
    // and the unrelated projections below keep working. This is **not** a Core
    // failure and must not stop the other subscriptions.
    let workspace_file_updates = match services.p03() {
        Ok(p03) => Some(
            p03.workspace
                .subscribe_projection_changes()
                .context("failed to subscribe to Workspace Files projection updates")?,
        ),
        Err(_) => None,
    };
    let security_updates = services
        .persistence
        .subscribe_security()
        .context("failed to subscribe to Security projection updates")?;
    let model_updates = services
        .persistence
        .subscribe_model_projections()
        .context("failed to subscribe to Model projection updates")?;
    let tool_updates = services
        .persistence
        .subscribe_tool_projections()
        .context("failed to subscribe to Tool projection updates")?;

    Ok(tokio::spawn(async move {
        if let StartupMode::Ready(services) = &state.startup_mode
            && let Ok(payload) = services.p04().and_then(|p04| p04.security_runtime_projection())
        {
            let change = SecurityProjectionChange::SecurityRuntime {
                runtime_instance_id: state.options.runtime_instance_id,
                revision: Revision::INITIAL,
                payload,
            };
            let _ = send_notification(
                &queue,
                QueuePriority::P2,
                method::SECURITY_PROJECTION_CHANGE,
                &change,
            )
            .await;
        }
        let mut interval = time::interval(Duration::from_millis(25));
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            let mut latest_workspace_session = None;
            while let Ok(committed) = workspace_session_updates.try_recv() {
                latest_workspace_session = Some(committed.payload);
            }
            if let Some(payload) = latest_workspace_session
                && projection_publish::workspace_session(&state, &queue, &subscriptions, payload)
                    .await
                    .is_err()
            {
                break;
            }

            let mut workspace_files_changed = false;
            // With no Workspace Files capability there is no subscription,
            // so no change can arrive and no rebuild is attempted.
            if let Some(updates) = &workspace_file_updates {
                while updates.try_recv().is_ok() {
                    workspace_files_changed = true;
                }
            }
            if workspace_files_changed {
                let revision = Revision::new(
                    state.workspace_files_revision.fetch_add(1, Ordering::AcqRel) + 1,
                );
                if subscriptions.workspace_files.load(Ordering::Acquire) {
                    // Building this projection reads every online workspace's catalogue,
                    // buffers and deferred mutations. It is synchronous work and it runs on
                    // the bounded boundary, so a slow catalogue cannot park this reactor
                    // thread - and therefore cannot stop the heartbeat, the control lane or
                    // the session notifications that share it.
                    let payload = match &state.startup_mode {
                        StartupMode::Ready(services) => {
                            // No Workspace Files capability means no such
                            // projection exists. It is reported as unavailable
                            // rather than rebuilt from a fabricated workspace.
                            let Ok(p03) = services.p03() else {
                                continue;
                            };
                            let workspace = Arc::clone(&p03.workspace);
                            match state
                                .sync
                                .run(
                                    crate::sync_boundary::SyncSection::Projection,
                                    crate::sync_boundary::SyncSubject::new()
                                        .method("projection.workspace_files"),
                                    move || workspace.workspace_files_projection(),
                                )
                                .await
                            {
                                crate::blocking_pool::BlockingAdmission::Completed(Ok(payload)) => {
                                    Ok(payload)
                                }
                                crate::blocking_pool::BlockingAdmission::Completed(Err(error)) => {
                                    Err(error.to_string())
                                }
                                crate::blocking_pool::BlockingAdmission::Refused { waited_ms } => {
                                    Err(format!(
                                        "the projection did not run: no synchronous worker was free after {waited_ms} ms"
                                    ))
                                }
                                crate::blocking_pool::BlockingAdmission::Unknown { waited_ms } => {
                                    Err(format!(
                                        "the projection did not answer inside its budget after {waited_ms} ms"
                                    ))
                                }
                            }
                        }
                        StartupMode::RecoveryRequired { .. } => continue,
                    };
                    let payload = match payload {
                        Ok(payload) => payload,
                        Err(reason) => {
                            append_core_log(
                                "workspace_files_projection_failed",
                                json!({ "reason": reason }),
                            );
                            continue;
                        }
                    };
                    let change = ProjectionChange::WorkspaceFiles {
                        runtime_instance_id: state.options.runtime_instance_id,
                        projection_scope: ProjectionScope::Global,
                        revision,
                        payload,
                    };
                    if send_notification(
                        &queue,
                        QueuePriority::P2,
                        method::PROJECTION_CHANGE,
                        &change,
                    )
                    .await
                    .is_err()
                    {
                        break;
                    }
                }
            }

            while let Ok(committed) = security_updates.try_recv() {
                let revision = Revision::new(committed.source_revision);
                let change = match committed.payload {
                    SecurityProjectionPayload::SessionPermission(payload) => {
                        SecurityProjectionChange::SessionPermission {
                            runtime_instance_id: state.options.runtime_instance_id,
                            revision,
                            payload,
                        }
                    }
                    SecurityProjectionPayload::PendingApproval(payload) => {
                        SecurityProjectionChange::PendingApproval {
                            runtime_instance_id: state.options.runtime_instance_id,
                            revision,
                            payload,
                        }
                    }
                    SecurityProjectionPayload::SecurityRuntime(payload) => {
                        SecurityProjectionChange::SecurityRuntime {
                            runtime_instance_id: state.options.runtime_instance_id,
                            revision,
                            payload,
                        }
                    }
                };
                if send_notification(
                    &queue,
                    QueuePriority::P2,
                    method::SECURITY_PROJECTION_CHANGE,
                    &change,
                )
                .await
                .is_err()
                {
                    return;
                }
            }

            while let Ok(change) = model_updates.try_recv() {
                if subscriptions.contains(&change)
                    && send_notification(
                        &queue,
                        projection_change_priority(&change),
                        method::PROJECTION_CHANGE,
                        &change,
                    )
                    .await
                    .is_err()
                {
                    return;
                }
            }

            while let Ok(change) = tool_updates.try_recv() {
                if subscriptions.contains(&change)
                    && send_notification(
                        &queue,
                        projection_change_priority(&change),
                        method::PROJECTION_CHANGE,
                        &change,
                    )
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    }))
}

async fn authenticate_connection(
    state: &RuntimeState,
    server: &mut NamedPipeServer,
    actual_parent: &ProcessIdentity,
) -> Result<IpcFrameHeader> {
    let begin_frame = time::timeout(HANDSHAKE_TIMEOUT, read_authenticated_frame(server))
        .await
        .context("runtime handshake begin timed out")??;
    if begin_frame.header.sequence != 0 {
        bail!("runtime handshake begin must use frame sequence zero");
    }
    let begin_rpc = parse_handshake_frame(
        begin_frame.frame,
        method::RUNTIME_HANDSHAKE_BEGIN,
        "runtime.handshake.begin",
    )?;
    let begin: RuntimeHandshakeRequest = serde_json::from_value(begin_rpc.params.clone())
        .context("runtime.handshake.begin params are invalid")?;
    if begin_rpc.command_invocation_id.is_some()
        || begin.channel_id != begin_frame.header.channel_id
        || begin.connection_epoch != begin_frame.header.connection_epoch
    {
        bail!("runtime handshake begin did not match its authenticated frame header");
    }
    validate_handshake_begin(state, actual_parent, &begin)?;

    let negotiated = ContractVersion::new(
        ContractVersion::CURRENT.major,
        negotiate_minor(ContractVersion::CURRENT.minor, begin.handshake.protocol_version.minor),
    );
    let advertised_protocol = if state.options.test_mode == CoreTestMode::Incompatible {
        ContractVersion::new(99, 0)
    } else {
        ContractVersion::CURRENT
    };
    let server_handshake = RuntimeHandshake {
        protocol_version: advertised_protocol,
        process_kind: RuntimeProcessKind::Core,
        component_version: state.release.component_version.clone(),
        build_commit: state.release.build_commit.clone(),
        release_descriptor: state.release.release_descriptor.clone(),
        runtime_instance_id: state.options.runtime_instance_id,
        parent_runtime_instance_id: Some(begin.handshake.runtime_instance_id),
        supported_features: vec![
            "aura.ipc.v1".to_owned(),
            "aura.ipc.authenticated-channel.v1".to_owned(),
            "aura.ipc.command-outcome.v1".to_owned(),
            "aura.ipc.secret-input.v1".to_owned(),
            "aura.ipc.tool-secret-input.v1".to_owned(),
            "aura.config.recovery.v1".to_owned(),
            "aura.projection.core-health.v1".to_owned(),
            "aura.projection.workspace-session.v1".to_owned(),
            "aura.projection.workspace-files.v1".to_owned(),
            "aura.security-control-plane.v1".to_owned(),
            "aura.model-control-plane.v1".to_owned(),
            "aura.tool-control-plane.v1".to_owned(),
            "aura.binary-stream.v1".to_owned(),
        ],
    };
    let mut nonce_bytes = [0_u8; 32];
    getrandom::fill(&mut nonce_bytes).context("failed to create runtime server nonce")?;
    let server_nonce = SecretNonce::from_bytes(nonce_bytes);
    nonce_bytes.zeroize();
    let transcript = RuntimeHandshakeTranscript {
        handshake_kind: begin.handshake_kind,
        channel_id: begin.channel_id,
        connection_epoch: begin.connection_epoch,
        client_nonce: begin.client_nonce.clone(),
        server_nonce: server_nonce.clone(),
        client_handshake: begin.handshake.clone(),
        server_handshake: server_handshake.clone(),
        negotiated_protocol_version: negotiated,
    };
    let transcript_hash = runtime_handshake_transcript_hash(&transcript)?;
    let response = RuntimeHandshakeResponse {
        handshake_kind: begin.handshake_kind,
        handshake: server_handshake,
        channel_id: begin.channel_id,
        connection_epoch: begin.connection_epoch,
        client_nonce: begin.client_nonce.clone(),
        server_nonce,
        negotiated_protocol_version: negotiated,
        transcript_hash: transcript_hash.clone(),
    };
    write_json_direct_authenticated(
        server,
        IpcFrameHeader {
            channel_id: begin.channel_id,
            connection_epoch: begin.connection_epoch,
            sequence: 0,
        },
        &JsonRpcResponse::success(begin_rpc.id, serde_json::to_value(response)?),
    )
    .await?;

    let finish_frame = time::timeout(HANDSHAKE_TIMEOUT, read_authenticated_frame(server))
        .await
        .context("runtime handshake finish timed out")??;
    if finish_frame.header.channel_id != begin.channel_id
        || finish_frame.header.connection_epoch != begin.connection_epoch
        || finish_frame.header.sequence != 1
    {
        bail!("runtime handshake finish did not match the pending channel");
    }
    let finish_rpc = parse_handshake_frame(
        finish_frame.frame,
        method::RUNTIME_HANDSHAKE_FINISH,
        "runtime.handshake.finish",
    )?;
    if finish_rpc.command_invocation_id.is_some() {
        bail!("runtime handshake finish cannot carry a CommandInvocationId");
    }
    let finish: RuntimeHandshakeFinish = serde_json::from_value(finish_rpc.params)
        .context("runtime.handshake.finish params are invalid")?;
    if finish.handshake_kind != begin.handshake_kind
        || finish.channel_id != begin.channel_id
        || finish.connection_epoch != begin.connection_epoch
        || finish.transcript_hash != transcript_hash
    {
        bail!("runtime handshake finish did not match the frozen transcript");
    }
    activate_authenticated_channel(state, actual_parent, &begin, &finish, &transcript_hash)?;
    let complete = RuntimeHandshakeComplete {
        handshake_kind: begin.handshake_kind,
        channel_id: begin.channel_id,
        connection_epoch: begin.connection_epoch,
        transcript_hash: transcript_hash.clone(),
    };
    write_json_direct_authenticated(
        server,
        IpcFrameHeader {
            channel_id: begin.channel_id,
            connection_epoch: begin.connection_epoch,
            sequence: 1,
        },
        &JsonRpcResponse::success(finish_rpc.id, serde_json::to_value(complete)?),
    )
    .await?;
    append_core_log(
        "authenticated_channel_activated",
        json!({
            "handshake_kind": begin.handshake_kind,
            "connection_epoch": begin.connection_epoch,
        }),
    );
    Ok(IpcFrameHeader {
        channel_id: begin.channel_id,
        connection_epoch: begin.connection_epoch,
        sequence: 0,
    })
}

fn parse_handshake_frame(
    frame: WireFrame,
    expected_method: &str,
    label: &str,
) -> Result<JsonRpcRequest> {
    let WireFrame::Control(payload) = frame else {
        bail!("{label} must be a JSON control frame");
    };
    let message: JsonRpcMessage =
        serde_json::from_slice(&payload).context("invalid runtime handshake JSON-RPC frame")?;
    let JsonRpcMessage::Request(request) = message else {
        bail!("{label} must be a JSON-RPC request");
    };
    if request.jsonrpc != JSON_RPC_VERSION || request.method != expected_method {
        bail!("{label} used the wrong JSON-RPC method or version");
    }
    Ok(request)
}

fn validate_handshake_begin(
    state: &RuntimeState,
    actual_parent: &ProcessIdentity,
    request: &RuntimeHandshakeRequest,
) -> Result<()> {
    if request.handshake.process_kind != RuntimeProcessKind::ElectronMain {
        return Err(PairingInvalidated("peer process kind changed".to_owned()).into());
    }
    if request.handshake.protocol_version.major != ContractVersion::CURRENT.major {
        return Err(PairingInvalidated("Electron Main protocol major changed".to_owned()).into());
    }
    if request.handshake.release_descriptor != state.release.release_descriptor {
        return Err(PairingInvalidated("Electron Main ReleaseDescriptor changed".to_owned()).into());
    }
    let channel = state
        .authenticated_channel
        .lock()
        .map_err(|_| anyhow::anyhow!("authenticated channel state was poisoned"))?;
    match request.handshake_kind {
        RuntimeHandshakeKind::Initial => {
            if channel.peer_identity.is_some()
                || channel.bootstrap_secret.is_none()
                || request.connection_epoch != 1
            {
                bail!("initial handshake is unavailable after bootstrap consumption");
            }
        }
        RuntimeHandshakeKind::Reconnect => {
            let peer = channel.peer_identity.as_ref().context(
                "reconnect handshake is unavailable before a successful initial handshake",
            )?;
            let frozen_handshake = channel
                .peer_handshake
                .as_ref()
                .context("authenticated peer handshake is unavailable")?;
            let expected_epoch =
                channel.connection_epoch.checked_add(1).context("connection epoch overflow")?;
            if request.connection_epoch != expected_epoch {
                bail!("reconnect connection_epoch was not the next monotonic value");
            }
            if channel.active_channel_id == Some(request.channel_id) {
                bail!("reconnect must create a fresh channel_id");
            }
            if peer.process_id != actual_parent.process_id
                || peer.process_creation_time_100ns != actual_parent.creation_time_100ns
                || peer.windows_session_id != actual_parent.windows_session_id
            {
                return Err(PairingInvalidated(
                    "Electron Main process identity changed during reconnect".to_owned(),
                )
                .into());
            }
            if peer.runtime_instance_id != request.handshake.runtime_instance_id
                || peer.release_descriptor != state.release.release_descriptor
                || frozen_handshake != &request.handshake
            {
                return Err(PairingInvalidated(
                    "Electron Main RuntimeInstanceId or ReleaseDescriptor changed".to_owned(),
                )
                .into());
            }
        }
    }
    Ok(())
}

fn activate_authenticated_channel(
    state: &RuntimeState,
    actual_parent: &ProcessIdentity,
    begin: &RuntimeHandshakeRequest,
    finish: &RuntimeHandshakeFinish,
    transcript_hash: &ContentHash,
) -> Result<()> {
    let mut channel = state
        .authenticated_channel
        .lock()
        .map_err(|_| anyhow::anyhow!("authenticated channel state was poisoned"))?;
    let expected_epoch =
        channel.connection_epoch.checked_add(1).context("connection epoch overflow")?;
    if begin.connection_epoch != expected_epoch {
        bail!("connection_epoch changed while the handshake was pending");
    }
    match begin.handshake_kind {
        RuntimeHandshakeKind::Initial => {
            let proof = finish
                .bootstrap_proof
                .as_ref()
                .context("initial handshake did not include a bootstrap proof")?;
            let secret = channel
                .bootstrap_secret
                .as_ref()
                .context("bootstrap secret was already consumed")?;
            if !verify_runtime_bootstrap_proof(secret, transcript_hash, proof)? {
                bail!("initial handshake bootstrap proof was rejected");
            }
            let mut consumed =
                channel.bootstrap_secret.take().context("bootstrap secret was already consumed")?;
            consumed.zeroize();
            channel.peer_identity = Some(AuthenticatedPeerIdentity {
                process_id: actual_parent.process_id,
                process_creation_time_100ns: actual_parent.creation_time_100ns,
                runtime_instance_id: begin.handshake.runtime_instance_id,
                release_descriptor: state.release.release_descriptor.clone(),
                windows_session_id: actual_parent.windows_session_id,
            });
            channel.peer_handshake = Some(begin.handshake.clone());
        }
        RuntimeHandshakeKind::Reconnect => {
            if finish.bootstrap_proof.is_some() {
                bail!("reconnect handshake must not reuse a bootstrap proof");
            }
        }
    }
    channel.active_channel_id = Some(begin.channel_id);
    channel.connection_epoch = begin.connection_epoch;
    channel.transcript_hash = Some(transcript_hash.clone());
    Ok(())
}

const fn negotiate_minor(local: u16, peer: u16) -> u16 {
    if local < peer { local } else { peer }
}

fn spawn_writer<W>(
    mut writer: W,
    queue: AsyncPriorityQueue,
    channel: IpcFrameHeader,
) -> JoinHandle<Result<()>>
where
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut sequence = 2_u64;
        while let Ok(outbound) = queue.recv().await {
            let frame = AuthenticatedWireFrame {
                header: IpcFrameHeader {
                    channel_id: channel.channel_id,
                    connection_epoch: channel.connection_epoch,
                    sequence,
                },
                frame: outbound.frame,
            };
            aura_ipc::trace(
                "core.write.start",
                &format!("epoch={} seq={sequence}", channel.connection_epoch),
            );
            write_authenticated_frame(&mut writer, &frame).await?;
            aura_ipc::trace(
                "core.write.done",
                &format!("epoch={} seq={sequence}", channel.connection_epoch),
            );
            sequence = sequence.checked_add(1).context("IPC outbound sequence overflow")?;
        }
        aura_ipc::trace("core.write.closed", "queue closed");
        Ok(())
    })
}

fn spawn_startup(state: Arc<RuntimeState>, queue: AsyncPriorityQueue) -> JoinHandle<()> {
    tokio::spawn(async move {
        if state.options.test_mode == CoreTestMode::StallAfterHandshake {
            return;
        }
        let terminal_phase = match &state.startup_mode {
            StartupMode::Ready(_) => CoreStartupPhase::Ready,
            StartupMode::RecoveryRequired { .. } => CoreStartupPhase::RecoveryRequired,
        };
        let phases = if state.startup_complete.load(Ordering::Acquire) {
            vec![terminal_phase]
        } else {
            let mut phases =
                vec![CoreStartupPhase::TransportReady, CoreStartupPhase::RecoveringRuntime];
            if terminal_phase == CoreStartupPhase::Ready {
                phases.push(CoreStartupPhase::RebuildingProjection);
            }
            phases.push(terminal_phase);
            phases
        };
        for (index, phase) in phases.into_iter().enumerate() {
            if index > 0 {
                time::sleep(Duration::from_millis(25)).await;
            }
            let mut sequence = state.startup_sequence.fetch_add(1, Ordering::AcqRel);
            if state.options.test_mode == CoreTestMode::StartupSequenceGap && index == 1 {
                sequence = state.startup_sequence.fetch_add(1, Ordering::AcqRel) + 1;
            }
            let progress = CoreStartupProgress {
                runtime_instance_id: state.options.runtime_instance_id,
                sequence,
                phase,
                message: startup_message(phase).to_owned(),
                occurred_at: UtcTimestamp::now(),
            };
            if send_notification(
                &queue,
                QueuePriority::P0,
                method::RUNTIME_STARTUP_PROGRESS,
                &progress,
            )
            .await
            .is_err()
            {
                return;
            }
        }
        state.ready.store(terminal_phase == CoreStartupPhase::Ready, Ordering::Release);
        if terminal_phase == CoreStartupPhase::Ready {
            git::runtime::check_after_startup();
        }
        if !state.startup_complete.swap(true, Ordering::AcqRel) {
            state.core_health_revision.fetch_add(1, Ordering::AcqRel);
        }
        let mut projection = current_core_health_projection(&state);
        if state.options.test_mode == CoreTestMode::ProjectionRevisionGap
            && let ProjectionChange::CoreHealth { revision, .. } = &mut projection
        {
            *revision = Revision::new(revision.value() + 2);
        }
        let _ =
            send_notification(&queue, QueuePriority::P2, method::PROJECTION_CHANGE, &projection)
                .await;
    })
}

/// Handle one request.
///
/// `owned_claim` is the credential this request was dispatched with, when the
/// read loop took its claim before dispatch. The handler verifies that the
/// credential names *this* request and is still the live claim for its invocation
/// id; a borrowed, stale, invented or absent one cannot authorise an execution,
/// and there is no path that runs a claimed command without a live claim.
async fn handle_request(
    state: &RuntimeState,
    queue: &AsyncPriorityQueue,
    subscriptions: &ProjectionSubscriptions,
    ledger: &Arc<std::sync::Mutex<RequestLedger>>,
    request: &JsonRpcRequest,
    owned_claim: Option<&CommandClaim>,
) -> Result<IpcRequestTerminal> {
    if request.method == method::RUNTIME_COMMAND_OUTCOME {
        let query: CommandOutcomeQuery = serde_json::from_value(request.params.clone())
            .context("invalid runtime.command_outcome params")?;
        let outcome = state
            .command_outcomes
            .lock()
            .map_err(|_| anyhow::anyhow!("command outcome ledger was poisoned"))?
            .query(query.command_invocation_id);
        send_success(queue, request.id, serde_json::to_value(outcome)?).await?;
        return Ok(IpcRequestTerminal::Succeeded);
    }
    if is_side_effecting_main_method(&request.method) {
        let Some(command_invocation_id) = request.command_invocation_id else {
            let error = IpcErrorEnvelope {
                code: ErrorCode::new("ipc.command_invocation_id_required")?,
                category: ErrorCategory::Validation,
                message: "这次请求缺少操作编号，Aura Core 没有执行它；请重试。".to_owned(),
                retryable: false,
                correlation_id: request.id,
                terminal: IpcRequestTerminal::Failed,
                details: BTreeMap::new(),
            };
            send_json(queue, QueuePriority::P0, &JsonRpcResponse::failure(request.id, error))
                .await?;
            return Ok(IpcRequestTerminal::Failed);
        };
        if needs_command_claim(&request.method) {
            // One verdict, decided by the ledger: this request owns the live claim
            // (it may run), or the id belongs to another attempt (the frozen
            // refusal), or no claim was presented at all (an internal invariant —
            // every execution path takes its claim through the same entry point).
            let verdict = {
                let outcomes = state
                    .command_outcomes
                    .lock()
                    .map_err(|_| anyhow::anyhow!("command outcome ledger was poisoned"))?;
                claim_verdict(&outcomes, request.id, command_invocation_id, owned_claim)
            };
            let error = match verdict {
                ClaimVerdict::Owned => None,
                ClaimVerdict::Refused(refusal) => {
                    Some(command_invocation_refusal(request.id, command_invocation_id, refusal)?)
                }
                ClaimVerdict::Missing => Some(missing_claim_error(request.id)?),
            };
            if let Some(error) = error {
                send_json(queue, QueuePriority::P0, &JsonRpcResponse::failure(request.id, error))
                    .await?;
                return Ok(IpcRequestTerminal::Failed);
            }
        }
    }
    match request.method.as_str() {
        // Main is quitting. The Store records a normal exit, so the next
        // start checks only its structure first (see `BrokerJournal::prepare_exit`).
        "runtime.prepare_exit" => {
            if request.params != json!({}) {
                bail!("invalid runtime.prepare_exit params");
            }
            let marked = match &state.startup_mode {
                StartupMode::Ready(services) => services.broker.prepare_exit().unwrap_or(false),
                _ => false,
            };
            send_success(queue, request.id, json!({ "marked_clean": marked })).await?;
            Ok(IpcRequestTerminal::Succeeded)
        }
        "runtime.readiness" => {
            if request.params != json!({}) {
                bail!("invalid runtime.readiness params");
            }
            send_success(
                queue,
                request.id,
                serde_json::to_value(readiness::current_nonblocking(state).await)?,
            )
            .await?;
            Ok(IpcRequestTerminal::Succeeded)
        }
        // The user's configuration recovery entry.
        //
        // These three methods are the reachable flow the requirement asks for. They
        // are served from the runtime's own recovery entry, which performs the real
        // `aura-config` recovery and then updates the degradation registry, so a
        // repaired capability stops being reported as unavailable.
        "config.recovery.notice" => {
            if request.params != json!({}) {
                bail!("invalid config.recovery.notice params");
            }
            let StartupMode::Ready(services) = &state.startup_mode else {
                // A runtime in `RecoveryRequired` has no settings facade to repair
                // from. That is reported as its own state rather than as an empty
                // notice, which would read as "nothing is wrong".
                let notice = config_recovery::RecoveryNotice {
                    affected_domains: Vec::new(),
                    unavailable_capabilities: Vec::new(),
                    fault_codes: vec!["core.recovery_required".to_owned()],
                    suspended_calls: vec!["Agent Runtime".to_owned()],
                    available_actions: vec!["retry".to_owned()],
                    scope_known: false,
                    recovery_in_progress: false,
                    next_step: "Aura data requires recovery; configuration recovery is not the \
                               remedy for this state."
                        .to_owned(),
                };
                send_success(queue, request.id, serde_json::to_value(notice)?).await?;
                return Ok(IpcRequestTerminal::Succeeded);
            };
            let mut notice = services.config_recovery().notice();
            notice.recovery_in_progress =
                state.settlements.has_pending_method("config.recovery.apply");
            // The notice's facts are cross-checked against the registry through the
            // entry's own accessors, so "the notice says X is unavailable" and "the
            // capability check says X is unavailable" cannot drift apart.
            debug_assert!(
                notice.unavailable_capabilities.iter().all(|key| {
                    config_degradation::ConfigDependentCapability::ALL
                        .into_iter()
                        .find(|capability| capability.key() == key)
                        .is_none_or(|capability| {
                            !services.config_recovery().capability_available(capability)
                        })
                }),
                "the notice and the capability check must agree about what is unavailable"
            );
            // Every fault the notice reports must be one the recovery entry can act
            // on, or the notice would send the user to a repair that cannot help.
            // The check is the entry's own judgement, so the two cannot drift.
            let recoverable = notice
                .fault_codes
                .iter()
                .filter(|code| config_recovery::code_is_recoverable(code))
                .count();
            debug_assert_eq!(
                recoverable,
                notice.fault_codes.len(),
                "the notice reports a fault the recovery entry cannot act on: {:?}",
                notice.fault_codes
            );
            send_success(queue, request.id, serde_json::to_value(notice)?).await?;
            Ok(IpcRequestTerminal::Succeeded)
        }
        "config.recovery.plan" => {
            let request_value: config_recovery::ConfigRecoveryPlanRequest =
                serde_json::from_value(request.params.clone())
                    .map_err(|_| anyhow::anyhow!("invalid config.recovery.plan params"))?;
            let StartupMode::Ready(services) = &state.startup_mode else {
                return send_projection_unavailable(queue, request.id).await;
            };
            let entry = services.config_recovery();
            // An empty domain list means "every domain the runtime currently
            // reports as degraded", resolved from the real state rather than
            // guessed. An empty *degraded set* with an unknown recovery scope means
            // every domain, because the runtime cannot narrow it.
            let domains = if request_value.domains.is_empty() {
                // The domains the runtime really reports as affected, taken from the
                // same registry the notice and the capability checks read.
                let notice = entry.notice();
                if notice.scope_known {
                    services
                        .config_degradation
                        .all()
                        .into_iter()
                        .filter_map(|degraded| degraded.domain)
                        .collect::<Vec<_>>()
                } else {
                    aura_config::ConfigModule::ALL.to_vec()
                }
            } else {
                request_value.domains
            };
            if domains.is_empty() {
                bail!("config.recovery.plan has no affected configuration domain to plan for");
            }
            // Planning really reads configuration files (it opens
            // domains, reads history and validates recorded versions), so it runs on
            // the bounded synchronous boundary rather than on the request executor's
            // own thread. Before, a slow or blocked configuration file stalled the
            // whole dispatch loop — including the status and stop calls the user keeps
            // while configuration is broken.
            let planned = services
                .sync
                .run(
                    crate::sync_boundary::SyncSection::ConfigRecovery,
                    crate::sync_boundary::SyncSubject::new().method("config.recovery.plan"),
                    move || entry.plan_bundle(&domains),
                )
                .await;
            let plan = match planned {
                crate::blocking_pool::BlockingAdmission::Completed(Ok(plan)) => plan,
                crate::blocking_pool::BlockingAdmission::Completed(Err(error)) => {
                    // A plan that cannot be built is reported as a recovery refusal
                    // with the real reason, so the user learns why the entry cannot
                    // help instead of seeing a generic dispatch failure.
                    return send_recovery_refusal(
                        queue,
                        request.id,
                        "config.recovery.plan_unavailable",
                        &error.to_string(),
                    )
                    .await;
                }
                // A boundary refusal did **not** run the work, and an unknown outcome
                // started but did not answer. They are reported as themselves rather
                // than collapsed into "the plan failed", because the user's next step
                // differs: one is safe to retry, the other may already have read.
                crate::blocking_pool::BlockingAdmission::Refused { waited_ms } => {
                    return send_recovery_refusal(
                        queue,
                        request.id,
                        "config.recovery.plan_boundary_refused",
                        &format!("no configuration worker was free within {waited_ms}ms"),
                    )
                    .await;
                }
                crate::blocking_pool::BlockingAdmission::Unknown { waited_ms } => {
                    return send_recovery_refusal(
                        queue,
                        request.id,
                        "config.recovery.plan_boundary_unknown",
                        &format!("the configuration read did not answer within {waited_ms}ms"),
                    )
                    .await;
                }
            };
            send_success(queue, request.id, serde_json::to_value(plan)?).await?;
            Ok(IpcRequestTerminal::Succeeded)
        }
        "config.recovery.apply" => {
            let apply_request: config_recovery::ConfigRecoveryApplyRequest =
                serde_json::from_value(request.params.clone())
                    .map_err(|_| anyhow::anyhow!("invalid config.recovery.apply params"))?;
            let StartupMode::Ready(services) = &state.startup_mode else {
                return send_projection_unavailable(queue, request.id).await;
            };
            let entry = services.config_recovery();
            let domains = match (&apply_request.domain, apply_request.domains.is_empty()) {
                (Some(module), true) => vec![*module],
                (None, false) => apply_request.domains.clone(),
                (Some(_), false) => {
                    // A request that names both is ambiguous: it is refused as a
                    // response the caller can act on, not as a dispatch failure.
                    return send_recovery_refusal(
                        queue,
                        request.id,
                        "config.recovery.ambiguous_scope",
                        "config.recovery.apply must name either one domain or several, not both",
                    )
                    .await;
                }
                (None, true) => {
                    // No scope named. The runtime must not guess which configuration
                    // to reset, so it refuses and says so.
                    return send_recovery_refusal(
                        queue,
                        request.id,
                        "config.recovery.no_scope",
                        "config.recovery.apply must name the domains to recover; the runtime \
                         will not choose a scope for the user",
                    )
                    .await;
                }
            };
            let apply_guard = match entry.admit_apply(&domains).await {
                Ok(guard) => guard,
                Err(code) => return send_recovery_refusal(queue, request.id, code,
                    "another recovery still owns the affected scope; refresh its status before retrying").await,
            };
            // A single named domain is planned and applied through the single-domain
            // entry, so the narrower flow is the one really exercised and its scope
            // is exactly the one domain the caller named.
            //
            // This branch previously required the caller to use the
            // `domain` field. A caller that named exactly one domain in `domains`
            // instead fell through to the **bundle** path, where a one-member set has
            // no joint record and the whole-set restore is correctly refused — so the
            // same single-domain recovery succeeded or failed depending only on which
            // field the request happened to use. Routing by the real scope ("exactly
            // one domain") removes that difference.
            let single = match (&apply_request.domain, apply_request.domains.as_slice()) {
                (Some(module), []) => Some(*module),
                (None, [module]) => Some(*module),
                _ => None,
            };
            if let Some(module) = single.as_ref() {
                // The **whole** single-domain chain runs on the
                // bounded synchronous boundary — the confirmation read, the plan comparison and the
                // action check, as well as the write.
                //
                // The previous form put only `apply_domain` inside the section and called
                // `plan_domain_confirmed` directly on the request worker. That call is not a trivial
                // comparison: it plans the recovery, which **reads configuration files and their
                // history** and validates recorded versions. So a slow or blocked configuration file
                // parked the request worker even though the write itself was bounded — the same
                // stall the boundary exists to prevent, on the very path whose whole purpose is to
                // work while configuration is broken.
                //
                // It is one section rather than two on purpose: the confirmation and the write it
                // authorises are one decision, and splitting them would let the state change between
                // the comparison and the apply — which is exactly the stale-confirmation failure the
                // binding exists to refuse.
                //
                // The preview identity is **required** and is checked
                // before any write. A missing, stale, or action-mismatched confirmation is refused
                // rather than applied, so omitting the field is no longer a bypass.
                let apply_entry = entry.clone();
                let preview_id = apply_request.preview_id.clone();
                let action = apply_request.action;
                let module = *module;
                let applied = services
                    .sync
                    .run(
                        crate::sync_boundary::SyncSection::ConfigRecovery,
                        crate::sync_boundary::SyncSubject::new().method("config.recovery.apply"),
                        move || {
                            let _apply_guard = apply_guard;
                            let plan = apply_entry.plan_domain_confirmed(module, &preview_id)?;
                            // The action must be one the preview the user saw really offered. An
                            // action that has since become unavailable is refused, and an action
                            // the preview never listed is not something the user confirmed.
                            if !config_recovery::ConfigRecoveryEntry::domain_permits(&plan, action)
                            {
                                return Err(
                                    aura_config::recovery::ConfigRecoveryError::Unavailable {
                                        module,
                                        reason_code:
                                            aura_config::recovery::codes::ACTION_NOT_OFFERED,
                                    },
                                );
                            }
                            apply_entry.apply_domain(&plan, action)
                        },
                    )
                    .await;
                let report = match applied {
                    crate::blocking_pool::BlockingAdmission::Completed(Ok(report)) => report,
                    crate::blocking_pool::BlockingAdmission::Completed(Err(error)) => {
                        // The refusal carries its own stable code, so a stale confirmation and a
                        // failed write are reported as the different facts they are rather than
                        // collapsed into one generic failure.
                        let code = match &error {
                            aura_config::recovery::ConfigRecoveryError::Unavailable {
                                reason_code,
                                ..
                            } if *reason_code == aura_config::recovery::codes::PREVIEW_STALE => {
                                "config.recovery.preview_stale"
                            }
                            aura_config::recovery::ConfigRecoveryError::Unavailable {
                                reason_code,
                                ..
                            } if *reason_code
                                == aura_config::recovery::codes::ACTION_NOT_OFFERED =>
                            {
                                "config.recovery.action_not_previewed"
                            }
                            _ => "config.recovery.apply_failed",
                        };
                        return send_recovery_refusal(queue, request.id, code, &error.to_string())
                            .await;
                    }
                    crate::blocking_pool::BlockingAdmission::Refused { waited_ms } => {
                        // Provably not started, so this is safe for the user to retry.
                        return send_recovery_refusal(
                            queue,
                            request.id,
                            "config.recovery.apply_boundary_refused",
                            &format!("no configuration worker was free within {waited_ms}ms"),
                        )
                        .await;
                    }
                    crate::blocking_pool::BlockingAdmission::Unknown { waited_ms } => {
                        // The write may have run. This is reported as unknown rather
                        // than as a failure, because telling the user it did not happen
                        // when it may have is the dangerous direction.
                        return send_recovery_refusal(
                            queue,
                            request.id,
                            "config.recovery.apply_boundary_unknown",
                            &format!("the configuration write did not answer within {waited_ms}ms"),
                        )
                        .await;
                    }
                };
                append_core_log(
                    "config_recovery_applied",
                    json!({
                        "action": report.action.key(),
                        "domains": report
                            .outcomes
                            .iter()
                            .map(|outcome| outcome.module.key())
                            .collect::<Vec<_>>(),
                        "restored_capabilities": report.restored_capabilities,
                        "scope": entry.scoped_reset_domains(&[module])
                            .iter()
                            .map(|module| module.key())
                            .collect::<Vec<_>>(),
                    }),
                );
                send_success(queue, request.id, serde_json::to_value(report)?).await?;
                return Ok(IpcRequestTerminal::Succeeded);
            }
            // The cross-domain path used to rebuild the plan and
            // apply it **without ever consuming the request's `preview_id`**, so a bundle
            // confirmation was not binding. It now goes through the same confirmation check as
            // the single-domain path: the plan is rebuilt, compared against the preview the
            // user saw, and only then applied.
            //
            // The plan is rebuilt rather than trusted from the wire so the action is checked
            // against the plan the runtime would show **now** — an option that has since become
            // unavailable is refused rather than applied on a stale preview.
            // The cross-domain path reads and writes real
            // configuration files too, so it runs on the same bounded synchronous
            // boundary as the single-domain path. Both directions are carried, because
            // the bundle path plans and applies in one request.
            let bundle_entry = entry.clone();
            let plan_domains = domains.clone();
            let preview_id = apply_request.preview_id.clone();
            let planned = services
                .sync
                .run(
                    crate::sync_boundary::SyncSection::ConfigRecovery,
                    crate::sync_boundary::SyncSubject::new().method("config.recovery.plan"),
                    move || bundle_entry.plan_bundle_confirmed(&plan_domains, &preview_id),
                )
                .await;
            let plan = match planned {
                crate::blocking_pool::BlockingAdmission::Completed(Ok(plan)) => plan,
                crate::blocking_pool::BlockingAdmission::Completed(Err(error)) => {
                    return send_recovery_refusal(
                        queue,
                        request.id,
                        "config.recovery.preview_stale",
                        &error.to_string(),
                    )
                    .await;
                }
                crate::blocking_pool::BlockingAdmission::Refused { waited_ms } => {
                    return send_recovery_refusal(
                        queue,
                        request.id,
                        "config.recovery.plan_boundary_refused",
                        &format!("no configuration worker was free within {waited_ms}ms"),
                    )
                    .await;
                }
                crate::blocking_pool::BlockingAdmission::Unknown { waited_ms } => {
                    return send_recovery_refusal(
                        queue,
                        request.id,
                        "config.recovery.plan_boundary_unknown",
                        &format!("the configuration read did not answer within {waited_ms}ms"),
                    )
                    .await;
                }
            };
            if !config_recovery::ConfigRecoveryEntry::bundle_permits(&plan, apply_request.action) {
                return send_recovery_refusal(
                    queue,
                    request.id,
                    "config.recovery.action_not_previewed",
                    "the chosen action is not among the options that preview offered",
                )
                .await;
            }
            let bundle_apply_entry = entry.clone();
            let action = apply_request.action;
            let applied = services
                .sync
                .run(
                    crate::sync_boundary::SyncSection::ConfigRecovery,
                    crate::sync_boundary::SyncSubject::new().method("config.recovery.apply"),
                    move || {
                        let _apply_guard = apply_guard;
                        bundle_apply_entry.apply_bundle(&plan, action)
                    },
                )
                .await;
            let report = match applied {
                crate::blocking_pool::BlockingAdmission::Completed(Ok(report)) => report,
                crate::blocking_pool::BlockingAdmission::Completed(Err(error)) => {
                    return send_recovery_refusal(
                        queue,
                        request.id,
                        "config.recovery.apply_failed",
                        &error.to_string(),
                    )
                    .await;
                }
                crate::blocking_pool::BlockingAdmission::Refused { waited_ms } => {
                    return send_recovery_refusal(
                        queue,
                        request.id,
                        "config.recovery.apply_boundary_refused",
                        &format!("no configuration worker was free within {waited_ms}ms"),
                    )
                    .await;
                }
                crate::blocking_pool::BlockingAdmission::Unknown { waited_ms } => {
                    return send_recovery_refusal(
                        queue,
                        request.id,
                        "config.recovery.apply_boundary_unknown",
                        &format!("the configuration write did not answer within {waited_ms}ms"),
                    )
                    .await;
                }
            };
            // The reason a capability is *still* unavailable, when the recovery did
            // not clear it. Read through the entry so the loop is closed by the same
            // fact the notice and the capability check use.
            let still_unavailable = config_degradation::ConfigDependentCapability::ALL
                .into_iter()
                .filter(|capability| !entry.capability_available(*capability))
                .map(|capability| {
                    (
                        capability.key(),
                        entry.capability_reason(capability).unwrap_or_else(|| "unknown".to_owned()),
                    )
                })
                .collect::<Vec<_>>();
            append_core_log(
                "config_recovery_applied",
                json!({
                    "action": report.action.key(),
                    "domains": report
                        .outcomes
                        .iter()
                        .map(|outcome| outcome.module.key())
                        .collect::<Vec<_>>(),
                    "restored_capabilities": report.restored_capabilities,
                    "still_unavailable": still_unavailable,
                }),
            );
            send_success(queue, request.id, serde_json::to_value(report)?).await?;
            Ok(IpcRequestTerminal::Succeeded)
        }
        #[cfg(feature = "p08-contract-driver")]
        p08_contract_driver::METHOD_V1 => {
            let StartupMode::Ready(services) = &state.startup_mode else {
                return send_projection_unavailable(queue, request.id).await;
            };
            let channel = authenticated_configuration_channel_identity(state)?;
            match p08_contract_driver::dispatch_v1(state, services, request, channel).await {
                Ok(value) => {
                    send_recorded_response(
                        state,
                        queue,
                        request,
                        JsonRpcResponse::success(request.id, value),
                    )
                    .await?;
                    Ok(IpcRequestTerminal::Succeeded)
                }
                Err(error) => {
                    send_recorded_response(
                        state,
                        queue,
                        request,
                        JsonRpcResponse::failure(request.id, error),
                    )
                    .await?;
                    Ok(IpcRequestTerminal::Failed)
                }
            }
        }
        method::PROJECTION_SUBSCRIBE => {
            let subscription: ProjectionSubscription =
                serde_json::from_value(request.params.clone())
                    .context("invalid projection.subscribe params")?;
            match projection_for(state, subscription.projection_kind, subscription.projection_scope)
            {
                Ok(projection) => {
                    subscriptions
                        .subscribe(subscription.projection_kind, subscription.projection_scope)?;
                    send_success(queue, request.id, json!({ "subscribed": true })).await?;
                    send_notification(
                        queue,
                        QueuePriority::P1,
                        method::PROJECTION_CHANGE,
                        &projection,
                    )
                    .await?;
                    Ok(IpcRequestTerminal::Succeeded)
                }
                Err(_) => send_projection_unavailable(queue, request.id).await,
            }
        }
        method::PROJECTION_UNSUBSCRIBE => {
            let unsubscribe: ProjectionUnsubscribeRequest =
                serde_json::from_value(request.params.clone())
                    .context("invalid projection.unsubscribe params")?;
            subscriptions.unsubscribe(unsubscribe.projection_kind, unsubscribe.projection_scope)?;
            send_success(queue, request.id, json!({ "unsubscribed": true })).await?;
            Ok(IpcRequestTerminal::Succeeded)
        }
        method::PROJECTION_SNAPSHOT => {
            let snapshot: ProjectionSnapshotRequest =
                serde_json::from_value(request.params.clone())
                    .context("invalid projection.snapshot params")?;
            match projection_for_bounded(state, snapshot.projection_kind, snapshot.projection_scope)
                .await
            {
                Ok(projection) => {
                    send_success(queue, request.id, serde_json::to_value(projection)?).await?;
                    Ok(IpcRequestTerminal::Succeeded)
                }
                Err(_) => send_projection_unavailable(queue, request.id).await,
            }
        }
        method::RUNTIME_CANCEL => routes::cancel(state, queue, ledger, request).await,
        method_name
            if aura_ipc::routes::owned_by(method_name, aura_ipc::routes::RouteOwner::Git) =>
        {
            routes::git(state, queue, ledger, request).await
        }
        method_name
            if aura_ipc::routes::owned_by(method_name, aura_ipc::routes::RouteOwner::Agent) =>
        {
            routes::agent(state, queue, ledger, request).await
        }
        method_name if routes::is_management(method_name) => {
            routes::management(state, queue, request).await
        }
        method_name
            if aura_ipc::routes::owned_by(method_name, aura_ipc::routes::RouteOwner::Tool)
                && aura_ipc::routes::core_route(method_name)
                    .is_some_and(|r| r.auth == aura_ipc::routes::RouteAuth::AuthenticatedMain) =>
        {
            routes::tool(state, queue, request).await
        }
        method_name
            if aura_ipc::routes::owned_by(method_name, aura_ipc::routes::RouteOwner::Workspace)
                && aura_ipc::routes::core_route(method_name)
                    .is_some_and(|r| r.auth == aura_ipc::routes::RouteAuth::AuthenticatedMain) =>
        {
            routes::workspace(state, queue, request).await
        }
        method_name
            if aura_ipc::routes::owned_by(method_name, aura_ipc::routes::RouteOwner::Security)
                && aura_ipc::routes::core_route(method_name)
                    .is_some_and(|r| r.auth == aura_ipc::routes::RouteAuth::AuthenticatedMain) =>
        {
            routes::security(state, queue, request).await
        }
        method_name
            if aura_ipc::routes::owned_by(method_name, aura_ipc::routes::RouteOwner::Model)
                && aura_ipc::routes::core_route(method_name)
                    .is_some_and(|r| r.auth == aura_ipc::routes::RouteAuth::AuthenticatedMain) =>
        {
            routes::model(state, queue, subscriptions, request).await
        }
        _ => {
            let error = IpcErrorEnvelope {
                code: ErrorCode::new("ipc.method_not_found")?,
                category: ErrorCategory::NotFound,
                message: format!("Unsupported Core method: {}", request.method),
                retryable: false,
                correlation_id: request.id,
                terminal: IpcRequestTerminal::Failed,
                details: BTreeMap::new(),
            };
            send_json(queue, QueuePriority::P0, &JsonRpcResponse::failure(request.id, error))
                .await?;
            Ok(IpcRequestTerminal::Failed)
        }
    }
}

fn authenticated_configuration_channel_identity(
    state: &RuntimeState,
) -> Result<AuthenticatedConfigurationChannelIdentity> {
    let channel = state
        .authenticated_channel
        .lock()
        .map_err(|_| anyhow::anyhow!("authenticated channel state was poisoned"))?;
    let authenticated_main_runtime_instance_id = channel
        .peer_handshake
        .as_ref()
        .context("authenticated Main handshake is unavailable")?
        .runtime_instance_id;
    let authenticated_channel_id =
        channel.active_channel_id.context("authenticated Main channel is unavailable")?;
    Ok(AuthenticatedConfigurationChannelIdentity {
        core_runtime_instance_id: state.options.runtime_instance_id,
        authenticated_main_runtime_instance_id,
        authenticated_channel_id,
    })
}

/// The bounded form of [`projection_for`], for the projections that read every workspace.
///
/// A Workspace/Session or Workspace/Files snapshot is a real read over the persisted
/// workspace rows, every catalog, every buffer map and every deferred mutation. Those are
/// synchronous calls, so they run on the bounded boundary: a snapshot request must not park
/// the reactor that also has to deliver the heartbeat and answer the control lane. The
/// remaining kinds are cheap enough to answer in place and are unchanged.
async fn projection_for_bounded(
    state: &RuntimeState,
    kind: ProjectionKind,
    projection_scope: ProjectionScope,
) -> Result<ProjectionChange> {
    kind.validate_scope(projection_scope).context("invalid projection kind/scope route")?;
    let subject = crate::sync_boundary::SyncSubject::new().method("projection.snapshot");
    match kind {
        ProjectionKind::WorkspaceSession => {
            let StartupMode::Ready(services) = &state.startup_mode else {
                bail!("Workspace/Session projection is unavailable during recovery");
            };
            let persistence = Arc::clone(&services.persistence);
            let payload = match state
                .sync
                .run(crate::sync_boundary::SyncSection::Projection, subject, move || {
                    persistence.workspace_session_projection()
                })
                .await
            {
                crate::blocking_pool::BlockingAdmission::Completed(value) => value,
                crate::blocking_pool::BlockingAdmission::Refused { waited_ms } => {
                    bail!("the Workspace/Session projection did not run: no free worker after {waited_ms} ms")
                }
                crate::blocking_pool::BlockingAdmission::Unknown { waited_ms } => {
                    bail!("the Workspace/Session projection did not answer inside its budget after {waited_ms} ms")
                }
            }
            .context("failed to read Workspace/Session projection")?;
            Ok(ProjectionChange::WorkspaceSession {
                runtime_instance_id: state.options.runtime_instance_id,
                projection_scope,
                revision: Revision::new(state.workspace_session_revision.load(Ordering::Acquire)),
                payload,
            })
        }
        ProjectionKind::WorkspaceFiles => {
            let StartupMode::Ready(services) = &state.startup_mode else {
                bail!("Workspace Files projection is unavailable during recovery");
            };
            let workspace = Arc::clone(&services.p03()?.workspace);
            let payload = match state
                .sync
                .run(crate::sync_boundary::SyncSection::Projection, subject, move || {
                    workspace.workspace_files_projection()
                })
                .await
            {
                crate::blocking_pool::BlockingAdmission::Completed(value) => value,
                crate::blocking_pool::BlockingAdmission::Refused { waited_ms } => {
                    bail!("the Workspace Files projection did not run: no free worker after {waited_ms} ms")
                }
                crate::blocking_pool::BlockingAdmission::Unknown { waited_ms } => {
                    bail!("the Workspace Files projection did not answer inside its budget after {waited_ms} ms")
                }
            }
            .context("failed to read Workspace Files projection")?;
            Ok(ProjectionChange::WorkspaceFiles {
                runtime_instance_id: state.options.runtime_instance_id,
                projection_scope,
                revision: Revision::new(state.workspace_files_revision.load(Ordering::Acquire)),
                payload,
            })
        }
        _ => projection_for(state, kind, projection_scope),
    }
}

fn projection_for(
    state: &RuntimeState,
    kind: ProjectionKind,
    projection_scope: ProjectionScope,
) -> Result<ProjectionChange> {
    kind.validate_scope(projection_scope).context("invalid projection kind/scope route")?;
    match kind {
        ProjectionKind::CoreHealth => Ok(current_core_health_projection(state)),
        ProjectionKind::WorkspaceSession => {
            let StartupMode::Ready(services) = &state.startup_mode else {
                bail!("Workspace/Session projection is unavailable during recovery");
            };
            let payload = services
                .persistence
                .workspace_session_projection()
                .context("failed to read Workspace/Session projection")?;
            Ok(ProjectionChange::WorkspaceSession {
                runtime_instance_id: state.options.runtime_instance_id,
                projection_scope,
                revision: Revision::new(state.workspace_session_revision.load(Ordering::Acquire)),
                payload,
            })
        }
        ProjectionKind::WorkspaceFiles => {
            let StartupMode::Ready(services) = &state.startup_mode else {
                bail!("Workspace Files projection is unavailable during recovery");
            };
            let payload = services
                .p03()?
                .workspace
                .workspace_files_projection()
                .context("failed to read Workspace Files projection")?;
            Ok(ProjectionChange::WorkspaceFiles {
                runtime_instance_id: state.options.runtime_instance_id,
                projection_scope,
                revision: Revision::new(state.workspace_files_revision.load(Ordering::Acquire)),
                payload,
            })
        }
        ProjectionKind::SessionModelSelector => {
            let StartupMode::Ready(services) = &state.startup_mode else {
                bail!("model selector is unavailable during recovery");
            };
            let ProjectionScope::Session { session_id } = projection_scope else {
                bail!("model selector requires a Session scope");
            };
            services.p05()?.session_selector(session_id)
        }
        ProjectionKind::ModelSettings
        | ProjectionKind::ProviderCatalog
        | ProjectionKind::EndpointDetail
        | ProjectionKind::ModelProfile
        | ProjectionKind::ModelAvailabilityTest
        | ProjectionKind::ModelDiscovery
        | ProjectionKind::ContextWindowCorrection
        | ProjectionKind::PromptInspector => {
            let StartupMode::Ready(services) = &state.startup_mode else {
                bail!("persisted model projection is unavailable during recovery");
            };
            services
                .persistence
                .latest_model_projection(kind, projection_scope)
                .context("failed to read persisted model projection")?
                .context("requested model projection has not been initialized")
        }
        ProjectionKind::ToolSettings
        | ProjectionKind::ToolPackageList
        | ProjectionKind::ToolDraftList
        | ProjectionKind::WorkspaceToolPins
        | ProjectionKind::SessionToolPins
        | ProjectionKind::WorkspaceToolRegistrySummary
        | ProjectionKind::SessionEffectiveToolRegistry
        | ProjectionKind::SessionLoadedTools
        | ProjectionKind::SessionToolActivity
        | ProjectionKind::ToolPackage
        | ProjectionKind::ToolDefinition
        | ProjectionKind::ToolDraft
        | ProjectionKind::ToolConformanceRun
        | ProjectionKind::ToolLongOperation
        | ProjectionKind::ToolBatch
        | ProjectionKind::ToolCall
        | ProjectionKind::ToolExecutionAttempt => {
            let StartupMode::Ready(services) = &state.startup_mode else {
                bail!("persisted tool projection is unavailable during recovery");
            };
            services
                .persistence
                .latest_tool_projection(kind, projection_scope)
                .context("failed to read persisted tool projection")?
                .context("requested tool projection has not been initialized")
        }
    }
}

/// Refuses a configuration-recovery request as a response the caller can act on.
///
/// A recovery refusal must reach the caller as an error envelope with a
/// stable code, not as a dispatch failure. A dispatch failure tells the renderer
/// nothing it can show the user, and it hides the fact that the runtime
/// deliberately declined to act — which is the one thing the user most needs to
/// know when a repair is refused.
async fn send_recovery_refusal(
    queue: &AsyncPriorityQueue,
    request_id: CorrelationId,
    code: &str,
    message: &str,
) -> Result<IpcRequestTerminal> {
    let error = IpcErrorEnvelope {
        code: ErrorCode::new(code)?,
        category: ErrorCategory::Unavailable,
        message: message.to_owned(),
        retryable: false,
        correlation_id: request_id,
        terminal: IpcRequestTerminal::Failed,
        details: BTreeMap::new(),
    };
    send_json(queue, QueuePriority::P0, &JsonRpcResponse::failure(request_id, error)).await?;
    Ok(IpcRequestTerminal::Failed)
}

async fn send_projection_unavailable(
    queue: &AsyncPriorityQueue,
    request_id: CorrelationId,
) -> Result<IpcRequestTerminal> {
    let error = IpcErrorEnvelope {
        code: ErrorCode::new("projection.unavailable")?,
        category: ErrorCategory::Unavailable,
        message: "Aura 的数据需要恢复，暂时读不到这部分内容。".to_owned(),
        retryable: false,
        correlation_id: request_id,
        terminal: IpcRequestTerminal::Failed,
        details: BTreeMap::new(),
    };
    send_json(queue, QueuePriority::P0, &JsonRpcResponse::failure(request_id, error)).await?;
    Ok(IpcRequestTerminal::Failed)
}

async fn send_success(
    queue: &AsyncPriorityQueue,
    request_id: CorrelationId,
    value: Value,
) -> Result<()> {
    send_response(
        queue,
        QueuePriority::P1,
        response_with_size_fallback(JsonRpcResponse::success(request_id, value))?,
    )
    .await
}

async fn send_recorded_response(
    state: &RuntimeState,
    queue: &AsyncPriorityQueue,
    request: &JsonRpcRequest,
    response: JsonRpcResponse,
) -> Result<()> {
    let response = response_with_size_fallback(response)?;
    record_command_outcome(state, request, &response)?;
    // ACCEPTANCE REPLY BOUNDARY (default-off, feature-gated, marker-matched): the effect has already
    // happened once and its outcome is recorded; only the frame is delayed. It cannot touch
    // `response`, and with the feature off none of this exists.
    #[cfg(feature = "acceptance-barrier")]
    acceptance_barrier::hold_reply(&request.method, &request.params, request.id).await;
    let priority = if response.result.is_some() { QueuePriority::P1 } else { QueuePriority::P0 };
    send_response(queue, priority, response).await
}

/// Record a side-effecting command's outcome.
///
/// The record is what makes a replay of the same CommandInvocationId a
/// conflict, so it is shared by the handler path and the reader's own replies.
///
/// THREE THINGS IT DELIBERATELY DOES NOT DO:
///  * It does not record a **started-but-unanswered** command as a confirmed failure. A
///    response whose envelope carries the contract's "query the outcome" recovery field —
///    or the `command.outcome_unknown` code — is recorded as `Unknown`, which is the honest
///    answer to `runtime.command_outcome` and still refuses a replay. Recording it as
///    `Confirmed` was what turned "I stopped waiting" into "it definitely failed".
///  * It does not overwrite the row of an invocation a completion owner is answerable for.
///    That owner wrote the placeholder before the work started and is the only writer of the
///    real result; a departing caller must not race it.
///  * It does not read the message text to decide any of the above: the code and the
///    recovery field are contract values, a message is display text.
fn record_command_outcome(
    state: &RuntimeState,
    request: &JsonRpcRequest,
    response: &JsonRpcResponse,
) -> Result<()> {
    if !is_side_effecting_main_method(&request.method) {
        return Ok(());
    }
    let command_invocation_id = request
        .command_invocation_id
        .context("side-effecting response has no CommandInvocationId")?;
    let terminal = response
        .result
        .as_ref()
        .map(|result| result.terminal)
        .or_else(|| response.error.as_ref().map(|error| error.data.terminal))
        .context("recorded command response has no terminal")?;
    let stored_response = serde_json::to_value(response)?;
    let mut outcomes = state
        .command_outcomes
        .lock()
        .map_err(|_| anyhow::anyhow!("command outcome ledger was poisoned"))?;
    if outcomes.is_pending(command_invocation_id) {
        // A started operation's completion owner answers for this invocation. It holds the
        // real result, or it will; the caller's own envelope is not the final word.
        return Ok(());
    }
    if matches!(outcomes.query(command_invocation_id), CommandOutcome::Confirmed { .. }) {
        // The owner may have completed between the caller's timeout and sending its Unknown
        // response. That late caller response cannot overwrite the original authoritative value.
        return Ok(());
    }
    let outcome = if crate::command_settlement::response_is_unconfirmed(response) {
        CommandOutcome::Unknown {
            command_invocation_id,
            reason: aura_runtime_contract::CommandOutcomeUnknown::NotRecordedOrIncomplete,
        }
    } else {
        CommandOutcome::Confirmed { command_invocation_id, terminal, response: stored_response }
    };
    outcomes.insert(command_invocation_id, outcome);
    Ok(())
}

fn response_with_size_fallback(response: JsonRpcResponse) -> Result<JsonRpcResponse> {
    if serde_json::to_vec(&response)?.len() <= MAX_CONTROL_PAYLOAD_BYTES {
        return Ok(response);
    }
    Ok(JsonRpcResponse::failure(
        response.id,
        IpcErrorEnvelope {
            code: ErrorCode::new("ipc.response_too_large")?,
            category: ErrorCategory::Unavailable,
            message: "Aura Core 的回复超过了传输上限，没有送达；请缩小范围后重试。".to_owned(),
            retryable: false,
            correlation_id: response.id,
            terminal: IpcRequestTerminal::Failed,
            details: BTreeMap::new(),
        },
    ))
}

const fn projection_change_priority(change: &ProjectionChange) -> QueuePriority {
    match change {
        ProjectionChange::ProviderCatalog { .. } => QueuePriority::P1,
        _ => QueuePriority::P2,
    }
}

async fn send_response(
    queue: &AsyncPriorityQueue,
    priority: QueuePriority,
    response: JsonRpcResponse,
) -> Result<()> {
    send_json(queue, priority, &response).await
}

fn is_side_effecting_main_method(method_name: &str) -> bool {
    aura_ipc::routes::records_command_outcome(method_name)
}

async fn send_notification<T: Serialize>(
    queue: &AsyncPriorityQueue,
    priority: QueuePriority,
    method_name: &str,
    value: &T,
) -> Result<()> {
    let notification = JsonRpcNotification {
        jsonrpc: JSON_RPC_VERSION.to_owned(),
        method: method_name.to_owned(),
        params: serde_json::to_value(value)?,
    };
    send_json(queue, priority, &notification).await
}

async fn send_json<T: Serialize>(
    queue: &AsyncPriorityQueue,
    priority: QueuePriority,
    value: &T,
) -> Result<()> {
    let payload = serde_json::to_vec(value)?;
    let bytes = payload.len();
    let id = trace_request_id(&payload);
    let kind = if id == "none" { "notification" } else { "response" };
    aura_ipc::trace(
        "core.enqueue",
        &format!("kind={kind} priority={priority:?} bytes={bytes} id={id}"),
    );
    queue.send(OutboundFrame { priority, frame: WireFrame::Control(payload) }).await?;
    aura_ipc::trace("core.enqueued", &format!("kind={kind} id={id}"));
    Ok(())
}

/// Best-effort request id for tracing. Reads only the `id` field.
fn trace_request_id(payload: &[u8]) -> String {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(payload) else {
        return "unknown".to_owned();
    };
    value.get("id").and_then(|id| id.as_str()).unwrap_or("none").to_owned()
}

async fn write_json_direct_authenticated<T: Serialize>(
    server: &mut NamedPipeServer,
    header: IpcFrameHeader,
    value: &T,
) -> Result<()> {
    let payload = serde_json::to_vec(value)?;
    write_authenticated_frame(
        server,
        &AuthenticatedWireFrame { header, frame: WireFrame::Control(payload) },
    )
    .await?;
    Ok(())
}

const fn startup_message(phase: CoreStartupPhase) -> &'static str {
    match phase {
        CoreStartupPhase::TransportReady => "core.transport_ready",
        CoreStartupPhase::RecoveringRuntime => "core.recovering_runtime",
        CoreStartupPhase::RebuildingProjection => "core.rebuilding_projection",
        CoreStartupPhase::Ready => "core.ready",
        CoreStartupPhase::RecoveryRequired => "core.recovery_required",
    }
}

#[cfg(test)]
mod tests;
