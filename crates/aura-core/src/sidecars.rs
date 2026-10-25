mod authentication;
use authentication::authenticate_sidecar;

mod channel;
use channel::{SidecarClient, SidecarRequest, start_sidecar};

mod command;

mod execution;
mod external_process;
pub(crate) use external_process::ExternalProcessExecutionService;
use external_process::{ExternalProcessService, run_external_process_service};

mod output_scan;
use output_scan::{CredentialCanaryOutputScannerV1, contains_bytes};
mod stdio;

mod lifecycle;
use std::{collections::BTreeMap, time::Duration};

use anyhow::{Context, Result, bail};
use aura_domain_common::{
    ContentHash, ContractVersion, CorrelationId, RuntimeInstanceId, SecretNonce,
};
use aura_ipc::{
    JSON_RPC_VERSION, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse,
    ToolConformanceRuntimeSecretFrameV1, ToolRuntimeSecretFrameV1, WireFrame, method, read_frame,
    write_frame,
};
use aura_permission_domain::{
    AuthorizedToolConformanceSandboxExecutionV1, AuthorizedToolSandboxExecutionV1,
    CancelSandboxExecutionInput, ExternalProcessLaunchPlan, NetworkEnforcementKind,
    SandboxExecutionState, SandboxHostEvent,
};
use aura_runtime_contract::{
    IpcErrorEnvelope, RuntimeProcessKind, SidecarHandshakeChallenge, SidecarHandshakeHello,
    SidecarHandshakeProof, SidecarHandshakeTranscript, SidecarReady, sidecar_bootstrap_proof,
};
use aura_tool_runtime::{
    CommandHostEventErrorV1, CommandHostTerminalEventV1, CommandSandboxHostTrackerV1,
    StdioHostEventErrorV1, StdioHostTerminalEventV1, StdioObservedArtifactReadyV1,
    StdioObservedProgressV1, StdioProtocolErrorV1, StdioSandboxHostTrackerV1,
    ToolRuntimeCandidateV1, ToolStdioHostInputV1,
};
use aura_windows_platform::{
    ComponentJob, ManagedProcess, ProcessIdentity, create_secure_byte_pipe_server,
    validate_named_pipe_client,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::{
    net::windows::named_pipe::NamedPipeServer,
    sync::{OwnedSemaphorePermit, Semaphore, broadcast, mpsc, oneshot},
    time,
};
use zeroize::{Zeroize, Zeroizing};

use crate::p07_output::VerifiedToolOutputArtifactV1;
use crate::{CoreOptions, CoreReleaseContext, SidecarReleaseIdentity, append_core_log};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
const HEARTBEAT_FAILURE_WINDOW: Duration = Duration::from_secs(6);

pub struct SecuritySidecars {
    _backend_tasks: Vec<crate::RuntimeBackgroundTask>,
    pub(crate) readiness: crate::readiness::ReadinessStore,
    _sandbox: SidecarClient,
    _egress: SidecarClient,
    // Legacy P04 external-process adapter, retained until the R08 orchestrator cutover.
    _external_process_service: ExternalProcessExecutionService,
    #[allow(
        dead_code,
        reason = "the W7 authorization-bound adapter is committed before its gated fixture-runtime wiring"
    )]
    pub(crate) tool_sandbox_service: ToolSandboxExecutionService,
}

#[derive(Debug, thiserror::Error)]
#[error("security Sidecar rejected the request: {code}", code = .envelope.code)]
pub struct SidecarRemoteError {
    pub envelope: IpcErrorEnvelope,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExternalProcessExecutionOutcome {
    pub(crate) execution_id: aura_domain_common::SandboxExecutionId,
    pub(crate) state: SandboxExecutionState,
    pub(crate) process_id: Option<u32>,
    pub(crate) exit_code: Option<u32>,
    pub(crate) network_enforcement: NetworkEnforcementKind,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ExternalProcessExecutionError {
    #[error("Sandbox Host rejected external process execution: {code}", code = .0.code)]
    Rejected(IpcErrorEnvelope),
    #[error(transparent)]
    Unavailable(#[from] anyhow::Error),
}

#[allow(
    dead_code,
    reason = "the W7 authorization-bound adapter is committed before its gated fixture-runtime wiring"
)]
#[derive(Debug, thiserror::Error)]
pub(crate) enum ToolSandboxExecutionError {
    #[error("Sandbox Host rejected authorized Tool execution: {code}", code = .0.code)]
    Rejected(IpcErrorEnvelope),
    #[error(transparent)]
    Unavailable(#[from] anyhow::Error),
}

#[allow(
    dead_code,
    reason = "the W7 authorization-bound adapter is committed before its gated fixture-runtime wiring"
)]
pub(crate) struct ToolSandboxHostPermit {
    _permit: OwnedSemaphorePermit,
}

#[allow(
    dead_code,
    reason = "the W7 Stdio evidence is consumed by the gated ordinary Tool dispatch orchestrator"
)]
pub(crate) struct ToolStdioDispatchEvidenceV1 {
    pub(crate) candidate: ToolRuntimeCandidateV1,
    pub(crate) stderr: Vec<u8>,
    pub(crate) internal_protocol_error: Option<String>,
    pub(crate) credential_canary_detected: bool,
    /// Runtime declarations accepted by the protocol tracker. Core must still
    /// perform no-follow file verification before any declaration becomes an
    /// Artifact.
    pub(crate) artifact_declarations: Vec<StdioObservedArtifactReadyV1>,
    /// Handle-verified Attempt-owned Artifacts in first-accepted protocol
    /// sequence order. Access remains unavailable until Result finalization.
    pub(crate) verified_artifacts: Vec<VerifiedToolOutputArtifactV1>,
}

pub(crate) enum ToolRuntimeDispatchLifecycleUpdateV1 {
    RuntimeStarted,
    Progress(StdioObservedProgressV1),
}

#[allow(
    dead_code,
    reason = "the W7 command evidence is consumed by the gated ordinary Tool dispatch orchestrator"
)]
pub(crate) struct ToolCommandDispatchEvidenceV1 {
    pub(crate) candidate: ToolRuntimeCandidateV1,
    pub(crate) stderr: Vec<u8>,
    pub(crate) internal_event_error: Option<String>,
    pub(crate) credential_canary_detected: bool,
}

#[allow(
    dead_code,
    reason = "the W7 authorization-bound adapter is committed before its gated fixture-runtime wiring"
)]
#[derive(Clone)]
pub(crate) struct ToolSandboxExecutionService {
    sandbox: SidecarClient,
    host_capacity: std::sync::Arc<Semaphore>,
    #[cfg(test)]
    backend_entries: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
}

impl SecuritySidecars {
    #[cfg(test)]
    pub(crate) async fn shutdown_sandbox_for_test(&self) -> Result<()> {
        let _: Value =
            self.tool_sandbox_service.sandbox.request(method::SIDECAR_SHUTDOWN, &json!({})).await?;
        Ok(())
    }

    pub fn deferred(options: &CoreOptions, release: &CoreReleaseContext) -> Self {
        let readiness = crate::readiness::ReadinessStore::default();
        let (sandbox, sandbox_task) = lifecycle::deferred(
            options.clone(),
            release.clone(),
            release.sandbox_host.clone(),
            "strict_appcontainer",
            readiness.clone(),
        );
        let (egress, egress_task) = lifecycle::deferred(
            options.clone(),
            release.clone(),
            release.egress_proxy.clone(),
            "egress_proxy",
            readiness.clone(),
        );
        let host_capacity = std::sync::Arc::new(Semaphore::new(1));
        let (requests, receiver) = mpsc::channel(16);
        let external_task = tokio::spawn(run_external_process_service(
            ExternalProcessService {
                sandbox: sandbox.clone(),
                host_capacity: host_capacity.clone(),
            },
            receiver,
        ));
        Self {
            _backend_tasks: vec![
                sandbox_task,
                egress_task,
                crate::RuntimeBackgroundTask(external_task),
            ],
            readiness,
            _sandbox: sandbox.clone(),
            _egress: egress,
            _external_process_service: ExternalProcessExecutionService { requests },
            tool_sandbox_service: ToolSandboxExecutionService {
                sandbox,
                host_capacity,
                #[cfg(test)]
                backend_entries: None,
            },
        }
    }

    #[allow(
        dead_code,
        reason = "P04 freezes the Egress Proxy request contract for later consumers"
    )]
    pub async fn egress_request<T: Serialize, R: DeserializeOwned>(
        &self,
        method_name: &'static str,
        input: &T,
    ) -> Result<R> {
        self._egress.request(method_name, input).await
    }

    #[allow(dead_code, reason = "P04 freezes Sandbox events before P09 consumes them")]
    pub fn subscribe_sandbox_events(&self) -> broadcast::Receiver<Value> {
        self._sandbox._events.subscribe()
    }
}

#[cfg(test)]
mod tests;
