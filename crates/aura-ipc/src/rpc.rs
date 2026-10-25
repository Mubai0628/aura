use aura_domain_common::{CommandInvocationId, CorrelationId};
use aura_runtime_contract::{IpcErrorEnvelope, IpcRequestTerminal};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const JSON_RPC_VERSION: &str = "2.0";

pub mod method {
    pub const RUNTIME_HANDSHAKE_BEGIN: &str = "runtime.handshake.begin";
    pub const RUNTIME_HANDSHAKE_FINISH: &str = "runtime.handshake.finish";
    pub const RUNTIME_COMMAND_OUTCOME: &str = "runtime.command_outcome";
    pub const RUNTIME_HEARTBEAT: &str = "runtime.heartbeat";
    pub const RUNTIME_STARTUP_PROGRESS: &str = "runtime.startup_progress";
    pub const RUNTIME_CANCEL: &str = "aura.cancel";
    pub const SIDECAR_HANDSHAKE_HELLO: &str = "sidecar.handshake.hello";
    pub const SIDECAR_HANDSHAKE_PROOF: &str = "sidecar.handshake.proof";
    pub const SIDECAR_PING: &str = "sidecar.ping";
    pub const SIDECAR_SHUTDOWN: &str = "sidecar.shutdown";
    pub const SANDBOX_EXECUTE: &str = "sandbox.execute";
    pub const SANDBOX_EXECUTE_EXTERNAL: &str = "sandbox.execute_external";
    pub const SANDBOX_EXECUTE_AUTHORIZED_TOOL: &str = "sandbox.execute_authorized_tool";
    pub const SANDBOX_EXECUTE_AUTHORIZED_TOOL_CONFORMANCE: &str =
        "sandbox.execute_authorized_tool_conformance";
    pub const SANDBOX_TOOL_STDIO_INPUT: &str = "sandbox.tool_stdio_input";
    pub const SANDBOX_CANCEL: &str = "sandbox.cancel";
    pub const SANDBOX_EVENT: &str = "sandbox.event";
    pub const EGRESS_REGISTER_OPAQUE_TUNNEL: &str = "egress.register_opaque_tunnel";
    pub const EGRESS_REVOKE_GRANT: &str = "egress.revoke_grant";
    pub const EGRESS_REVOKE_EXECUTION: &str = "egress.revoke_execution";
    pub const PROJECTION_SUBSCRIBE: &str = "projection.subscribe";
    pub const PROJECTION_UNSUBSCRIBE: &str = "projection.unsubscribe";
    pub const PROJECTION_SNAPSHOT: &str = "projection.snapshot";
    pub const PROJECTION_CHANGE: &str = "projection.change";
    pub const STREAM_OPEN: &str = "stream.open";
    pub const STREAM_CLOSE: &str = "stream.close";
    pub const STREAM_ERROR: &str = "stream.error";
    pub const WORKSPACE_CREATE: &str = "workspace.create";
    pub const WORKSPACE_RESTORE: &str = "workspace.restore";
    pub const WORKSPACE_RELOCATE: &str = "workspace.relocate";
    pub const WORKSPACE_ARCHIVE: &str = "workspace.archive";
    pub const WORKSPACE_DELETE_PERMANENTLY: &str = "workspace.delete_permanently";
    pub const WORKSPACE_LIFECYCLE_BEGIN: &str = "workspace.lifecycle.begin";
    pub const WORKSPACE_LIFECYCLE_PREPARE: &str = "workspace.lifecycle.prepare";
    pub const WORKSPACE_LIFECYCLE_PREPARE_DELIVERED: &str = "workspace.lifecycle.prepare_delivered";
    pub const WORKSPACE_LIFECYCLE_ACK: &str = "workspace.lifecycle.ack";
    pub const WORKSPACE_LIFECYCLE_ISSUE_DISCARD_CONFIRMATION: &str =
        "workspace.lifecycle.issue_discard_confirmation";
    pub const WORKSPACE_LIFECYCLE_RESOLVE: &str = "workspace.lifecycle.resolve";
    pub const WORKSPACE_LIFECYCLE_RENDERER_UNAVAILABLE: &str =
        "workspace.lifecycle.renderer_unavailable";
    pub const BUFFER_OPEN: &str = "workspace.buffer.open";
    pub const BUFFER_EDIT: &str = "workspace.buffer.edit";
    pub const BUFFER_SAVE: &str = "workspace.buffer.save";
    pub const FS_STAT: &str = "fs.stat";
    pub const FS_LIST: &str = "fs.list";
    pub const FS_FIND_PATHS: &str = "fs.find_paths";
    pub const FS_GLOB: &str = "fs.glob";
    pub const FS_READ_TEXT: &str = "fs.read_text";
    pub const FS_SEARCH_TEXT: &str = "fs.search_text";
    pub const FS_HASH: &str = "fs.hash";
    pub const FS_INSPECT: &str = "fs.inspect";
    pub const FS_READ_BINARY_RANGE: &str = "fs.read_binary_range";
    pub const FS_WRITE_FILE: &str = "fs.write_file";
    pub const FS_APPLY_PATCH: &str = "fs.apply_patch";
    pub const FS_CREATE_DIRECTORY: &str = "fs.create_directory";
    pub const FS_COPY: &str = "fs.copy";
    pub const FS_MOVE: &str = "fs.move";
    pub const FS_DELETE: &str = "fs.delete";
    pub const FS_APPLY_CHANGESET: &str = "fs.apply_changeset";
    pub const ARTIFACT_CAPTURE_WORKSPACE_FILE: &str = "artifact.capture_workspace_file";
    pub const ARTIFACT_EXPORT: &str = "artifact.export";
    pub const PERMISSION_GET_DEFAULTS: &str = "permission.get_defaults";
    pub const PERMISSION_SET_DEFAULTS: &str = "permission.set_defaults";
    pub const PERMISSION_GET_SESSION_STATE: &str = "permission.get_session_state";
    pub const PERMISSION_SET_SESSION_MODE: &str = "permission.set_session_mode";
    pub const PERMISSION_LIST_GRANTS: &str = "permission.list_grants";
    pub const PERMISSION_REVOKE_GRANT: &str = "permission.revoke_grant";
    /// The reusable Native session consent the user approved, and its withdrawal.
    /// This is the only authorization that outlives the Operation it was approved
    /// for, so it is the only one with its own management routes.
    pub const FILE_SESSION_CONSENT_LIST: &str = "security.file_session_consent.list";
    pub const NETWORK_SESSION_CONSENT_LIST: &str = "security.network_session_consent.list";
    pub const NETWORK_SESSION_CONSENT_REVOKE: &str = "security.network_session_consent.revoke";
    pub const FILE_SESSION_CONSENT_REVOKE: &str = "security.file_session_consent.revoke";
    /// A ReadOnly session's "allow for this session" on one memory Vault.
    pub const MEMORY_SESSION_CONSENT_LIST: &str = "security.memory_session_consent.list";
    pub const MEMORY_SESSION_CONSENT_REVOKE: &str = "security.memory_session_consent.revoke";
    pub const NATIVE_SESSION_CONSENT_LIST: &str = "security.native_session_consent.list";
    pub const NATIVE_SESSION_CONSENT_REVOKE: &str = "security.native_session_consent.revoke";
    pub const APPROVAL_LIST_PENDING: &str = "approval.list_pending";
    pub const APPROVAL_RESOLVE: &str = "approval.resolve";
    pub const APPROVAL_GET_DETAILS: &str = "approval.get_details";
    pub const SECURITY_HOST_PREPARATION_STATUS: &str = "security.host_preparation.status";
    pub const SECURITY_HOST_PREPARATION_PROBE: &str = "security.host_preparation.probe";
    pub const SECURITY_HOST_PREPARATION_PREPARE: &str = "security.host_preparation.prepare";
    pub const SECURITY_HOST_PREPARATION_REPAIR: &str = "security.host_preparation.repair";
    pub const SECURITY_HOST_PREPARATION_REMOVE: &str = "security.host_preparation.remove";
    pub const SECURITY_HOST_PREPARATION_RESOLVE: &str = "security.host_preparation.resolve";
    pub const SECURITY_EXTERNAL_GRANT_ATTACH_READ_ONLY: &str =
        "security.external_grant.attach_read_only_selected";
    pub const SECURITY_EXTERNAL_GRANT_DERIVE_WORKSPACE: &str =
        "security.external_grant.derive_workspace";
    pub const SECURITY_EXTERNAL_GRANT_LIST: &str = "security.external_grant.list";
    pub const SECURITY_EXTERNAL_GRANT_REVALIDATE: &str = "security.external_grant.revalidate";
    pub const SECURITY_EXTERNAL_GRANT_REVOKE: &str = "security.external_grant.revoke";
    pub const SECURITY_QUARANTINE_LIST: &str = "security.quarantine.list";
    pub const SECURITY_QUARANTINE_PREPARE_RELEASE: &str = "security.quarantine.prepare_release";
    pub const SECURITY_QUARANTINE_RELEASE: &str = "security.quarantine.release";
    pub const SECURITY_QUARANTINE_ISSUE_DISCARD_CONFIRMATION: &str =
        "security.quarantine.issue_discard_confirmation";
    pub const SECURITY_QUARANTINE_DISCARD: &str = "security.quarantine.discard";
    pub const SECURITY_PROJECTION_CHANGE: &str = "security.projection.change";
    pub const TOOL_ACTIVITY_PAGE: &str = "tools.activity.page";
    pub const TOOL_CREDENTIAL_CANDIDATES: &str = "tools.credential.candidates";
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: CorrelationId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_invocation_id: Option<CommandInvocationId>,
    pub method: String,
    pub params: Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JsonRpcSuccess {
    pub terminal: IpcRequestTerminal,
    pub value: Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    pub data: IpcErrorEnvelope,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: CorrelationId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<JsonRpcSuccess>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn success(id: CorrelationId, value: Value) -> Self {
        Self {
            jsonrpc: JSON_RPC_VERSION.to_owned(),
            id,
            result: Some(JsonRpcSuccess { terminal: IpcRequestTerminal::Succeeded, value }),
            error: None,
        }
    }

    pub fn failure(id: CorrelationId, error: IpcErrorEnvelope) -> Self {
        Self {
            jsonrpc: JSON_RPC_VERSION.to_owned(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code: json_rpc_error_code(error.terminal),
                message: error.message.clone(),
                data: error,
            }),
        }
    }

    #[must_use]
    pub const fn has_exactly_one_terminal(&self) -> bool {
        self.result.is_some() != self.error.is_some()
    }
}

const fn json_rpc_error_code(terminal: IpcRequestTerminal) -> i32 {
    match terminal {
        IpcRequestTerminal::Cancelled => -32_800,
        IpcRequestTerminal::TransportLost => -32_801,
        IpcRequestTerminal::ProtocolViolation => -32_600,
        IpcRequestTerminal::CoreRestarted => -32_802,
        IpcRequestTerminal::Failed | IpcRequestTerminal::Succeeded => -32_603,
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Notification(JsonRpcNotification),
    Response(JsonRpcResponse),
}
