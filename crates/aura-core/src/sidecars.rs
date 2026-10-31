mod authentication;
use authentication::authenticate_sidecar;

mod channel;
use channel::{SidecarClient, SidecarRequest, start_sidecar};

#[allow(
    dead_code,
    reason = "only tests use the Sandbox Host Tool reconciliation helpers; no production path dispatches Tools to the Sandbox Host"
)]
mod command;

mod execution;
mod external_process;
pub(crate) use external_process::ExternalProcessExecutionService;
use external_process::{ExternalProcessService, run_external_process_service};

#[allow(
    dead_code,
    reason = "only tests use the Sandbox Host Tool reconciliation helpers; no production path dispatches Tools to the Sandbox Host"
)]
mod output_scan;
use output_scan::contains_bytes;
#[allow(
    dead_code,
    reason = "only tests use the Sandbox Host Tool reconciliation helpers; no production path dispatches Tools to the Sandbox Host"
)]
mod stdio;

mod lifecycle;
use std::{collections::BTreeMap, time::Duration};

use anyhow::{Context, Result, bail};
use aura_domain_common::{
    ContentHash, ContractVersion, CorrelationId, RuntimeInstanceId, SecretNonce,
};
use aura_ipc::{
    JSON_RPC_VERSION, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse,
    WireFrame, method, read_frame, write_frame,
};
use aura_permission_domain::{
    ExternalProcessLaunchPlan, NetworkEnforcementKind, SandboxExecutionState, SandboxHostEvent,
};
use aura_runtime_contract::{
    IpcErrorEnvelope, RuntimeProcessKind, SidecarHandshakeChallenge, SidecarHandshakeHello,
    SidecarHandshakeProof, SidecarHandshakeTranscript, SidecarReady, sidecar_bootstrap_proof,
};
use aura_tool_runtime::{
    CommandHostEventErrorV1, CommandHostTerminalEventV1, CommandSandboxHostTrackerV1,
    StdioHostEventErrorV1, StdioHostTerminalEventV1, StdioProtocolErrorV1,
    StdioSandboxHostTrackerV1, ToolRuntimeCandidateV1,
};
use aura_windows_platform::{
    ComponentJob, ManagedProcess, ProcessIdentity, create_secure_byte_pipe_server,
    validate_named_pipe_client,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::{
    net::windows::named_pipe::NamedPipeServer,
    sync::{Semaphore, broadcast, mpsc, oneshot},
    time,
};
use zeroize::Zeroizing;

use crate::{CoreOptions, append_core_log};
use aura_core_base::release::{CoreReleaseContext, SidecarReleaseIdentity};

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
        reason = "only the Sandbox Host lifecycle tests probe availability through this service"
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

#[derive(Debug, thiserror::Error)]
pub(crate) enum ToolSandboxExecutionError {
    #[error(transparent)]
    Unavailable(#[from] anyhow::Error),
}

#[derive(Clone)]
pub(crate) struct ToolSandboxExecutionService {
    sandbox: SidecarClient,
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
            ExternalProcessService { sandbox: sandbox.clone(), host_capacity },
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
            tool_sandbox_service: ToolSandboxExecutionService { sandbox },
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
