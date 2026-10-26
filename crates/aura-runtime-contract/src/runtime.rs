use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::ReleaseDescriptor;
use aura_domain_common::{
    CommandInvocationId, ContentHash, ContentHashAlgorithm, ContractVersion, CorrelationId,
    ErrorCategory, ErrorCode, IpcChannelId, RuntimeInstanceId, SecretNonce, StreamId, UtcTimestamp,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeProcessKind {
    Launcher,
    ElectronMain,
    Renderer,
    Core,
    SandboxHost,
    EgressProxy,
    ElevatedHelper,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeHandshake {
    pub protocol_version: ContractVersion,
    pub process_kind: RuntimeProcessKind,
    pub component_version: String,
    pub build_commit: String,
    pub release_descriptor: ReleaseDescriptor,
    pub runtime_instance_id: RuntimeInstanceId,
    pub parent_runtime_instance_id: Option<RuntimeInstanceId>,
    pub supported_features: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeHandshakeKind {
    Initial,
    Reconnect,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IpcFrameHeader {
    pub channel_id: IpcChannelId,
    pub connection_epoch: u64,
    pub sequence: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeHandshakeRequest {
    pub handshake_kind: RuntimeHandshakeKind,
    pub handshake: RuntimeHandshake,
    pub channel_id: IpcChannelId,
    pub connection_epoch: u64,
    pub client_nonce: SecretNonce,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeHandshakeResponse {
    pub handshake_kind: RuntimeHandshakeKind,
    pub handshake: RuntimeHandshake,
    pub channel_id: IpcChannelId,
    pub connection_epoch: u64,
    pub client_nonce: SecretNonce,
    pub server_nonce: SecretNonce,
    pub negotiated_protocol_version: ContractVersion,
    pub transcript_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeHandshakeTranscript {
    pub handshake_kind: RuntimeHandshakeKind,
    pub channel_id: IpcChannelId,
    pub connection_epoch: u64,
    pub client_nonce: SecretNonce,
    pub server_nonce: SecretNonce,
    pub client_handshake: RuntimeHandshake,
    pub server_handshake: RuntimeHandshake,
    pub negotiated_protocol_version: ContractVersion,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeHandshakeFinish {
    pub handshake_kind: RuntimeHandshakeKind,
    pub channel_id: IpcChannelId,
    pub connection_epoch: u64,
    pub transcript_hash: ContentHash,
    pub bootstrap_proof: Option<SecretNonce>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeHandshakeComplete {
    pub handshake_kind: RuntimeHandshakeKind,
    pub channel_id: IpcChannelId,
    pub connection_epoch: u64,
    pub transcript_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthenticatedPeerIdentity {
    pub process_id: u32,
    pub process_creation_time_100ns: u64,
    pub runtime_instance_id: RuntimeInstanceId,
    pub release_descriptor: ReleaseDescriptor,
    pub windows_session_id: u32,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum RuntimeHandshakeCryptoError {
    #[error("runtime handshake transcript serialization failed: {0}")]
    Serialization(String),
    #[error("runtime handshake transcript hash must be SHA-256")]
    InvalidTranscriptHashAlgorithm,
    #[error("runtime handshake transcript hash is malformed")]
    InvalidTranscriptHash,
}

pub fn runtime_handshake_transcript_hash(
    transcript: &RuntimeHandshakeTranscript,
) -> Result<ContentHash, RuntimeHandshakeCryptoError> {
    let bytes = aura_domain_common::canonical_json_v1(transcript)
        .map_err(|error| RuntimeHandshakeCryptoError::Serialization(error.to_string()))?;
    ContentHash::new(ContentHashAlgorithm::Sha256, format!("{:x}", Sha256::digest(bytes)))
        .map_err(|_| RuntimeHandshakeCryptoError::InvalidTranscriptHash)
}

pub fn runtime_bootstrap_proof(
    secret: &[u8; 32],
    transcript_hash: &ContentHash,
) -> Result<SecretNonce, RuntimeHandshakeCryptoError> {
    let digest = transcript_hash_bytes(transcript_hash)?;
    let mut key_block = [0_u8; 64];
    key_block[..secret.len()].copy_from_slice(secret);
    let mut inner_pad = [0_u8; 64];
    let mut outer_pad = [0_u8; 64];
    for index in 0..64 {
        inner_pad[index] = key_block[index] ^ 0x36;
        outer_pad[index] = key_block[index] ^ 0x5c;
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(digest);
    let inner_digest = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner_digest);
    Ok(SecretNonce::from_bytes(outer.finalize().into()))
}

pub fn verify_runtime_bootstrap_proof(
    secret: &[u8; 32],
    transcript_hash: &ContentHash,
    supplied: &SecretNonce,
) -> Result<bool, RuntimeHandshakeCryptoError> {
    let expected = runtime_bootstrap_proof(secret, transcript_hash)?;
    let expected = expected.expose().as_bytes();
    let supplied = supplied.expose().as_bytes();
    if expected.len() != supplied.len() {
        return Ok(false);
    }
    let difference = expected
        .iter()
        .zip(supplied)
        .fold(0_u8, |difference, (left, right)| difference | (left ^ right));
    Ok(difference == 0)
}

fn transcript_hash_bytes(
    transcript_hash: &ContentHash,
) -> Result<[u8; 32], RuntimeHandshakeCryptoError> {
    if transcript_hash.algorithm != ContentHashAlgorithm::Sha256 {
        return Err(RuntimeHandshakeCryptoError::InvalidTranscriptHashAlgorithm);
    }
    let bytes = transcript_hash.hex.as_bytes();
    if bytes.len() != 64 {
        return Err(RuntimeHandshakeCryptoError::InvalidTranscriptHash);
    }
    let mut decoded = [0_u8; 32];
    for (index, pair) in bytes.chunks_exact(2).enumerate() {
        let high = decode_hex(pair[0]).ok_or(RuntimeHandshakeCryptoError::InvalidTranscriptHash)?;
        let low = decode_hex(pair[1]).ok_or(RuntimeHandshakeCryptoError::InvalidTranscriptHash)?;
        decoded[index] = (high << 4) | low;
    }
    Ok(decoded)
}

const fn decode_hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandOutcomeQuery {
    pub command_invocation_id: CommandInvocationId,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandOutcomeUnknown {
    NotRecordedOrIncomplete,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandOutcome {
    Confirmed {
        command_invocation_id: CommandInvocationId,
        terminal: IpcRequestTerminal,
        response: Value,
    },
    Unknown {
        command_invocation_id: CommandInvocationId,
        reason: CommandOutcomeUnknown,
    },
}

/// First authenticated message sent by a long-lived child component. The
/// process identity is verified independently against the connected Named Pipe
/// client and the process handle retained by Core.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SidecarHandshakeHello {
    pub handshake: RuntimeHandshake,
    pub process_id: u32,
    pub process_creation_time_100ns: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SidecarHandshakeChallenge {
    pub challenge_nonce: SecretNonce,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SidecarHandshakeTranscript {
    pub parent_runtime_instance_id: RuntimeInstanceId,
    pub child_runtime_instance_id: RuntimeInstanceId,
    pub process_kind: RuntimeProcessKind,
    pub process_id: u32,
    pub process_creation_time_100ns: u64,
    pub transport_binding_hash: ContentHash,
    pub challenge_nonce: SecretNonce,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SidecarHandshakeProof {
    pub transcript: SidecarHandshakeTranscript,
    pub proof: ContentHash,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SidecarReady {
    pub runtime_instance_id: RuntimeInstanceId,
    pub negotiated_protocol_version: ContractVersion,
}

/// Computes the challenge proof without placing the one-time bootstrap secret
/// on the transport. The transcript is canonical JSON authenticated with
/// BLAKE3's keyed mode.
pub fn sidecar_bootstrap_proof(
    secret: &[u8; 32],
    transcript: &SidecarHandshakeTranscript,
) -> Result<ContentHash, serde_json::Error> {
    let encoded = serde_json::to_vec(transcript)?;
    Ok(ContentHash {
        algorithm: aura_domain_common::ContentHashAlgorithm::Blake3,
        hex: blake3::keyed_hash(secret, &encoded).to_hex().to_string(),
    })
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreStartupPhase {
    TransportReady,
    RecoveringRuntime,
    RebuildingProjection,
    Ready,
    RecoveryRequired,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoreStartupProgress {
    pub runtime_instance_id: RuntimeInstanceId,
    pub sequence: u64,
    pub phase: CoreStartupPhase,
    pub message: String,
    pub occurred_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeHeartbeat {
    pub runtime_instance_id: RuntimeInstanceId,
    pub sequence: u64,
    pub sent_at: UtcTimestamp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreLifecycleState {
    NotStarted,
    Starting,
    Ready,
    Restarting,
    Incompatible,
    Unavailable,
    AppCrashEscalated,
    RecoveryRequired,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoreHealthProjection {
    pub state: CoreLifecycleState,
    pub read_only: bool,
    pub safe_mode: bool,
    pub restart_attempt: u8,
    pub status_code: String,
    pub status_message: String,
    /// A deletion Core is doing in the background, once it has run for a while.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub busy: Option<CoreBusyV1>,
}

/// A deletion Core is doing in the background, in short steps other requests are served
/// between, and since when.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoreBusyV1 {
    pub activity: CoreBusyActivityV1,
    pub since: UtcTimestamp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreBusyActivityV1 {
    /// Deleting a conversation and everything Aura holds for it.
    DeletingSession,
    /// Removing a workspace's records.
    RemovingWorkspace,
    /// Finishing, at start, deletions an earlier run did not complete.
    FinishingDeletion,
}

impl CoreHealthProjection {
    /// A ready Core with nothing to report: what the contract fixture shows.
    #[must_use]
    pub fn ready(status_message: &str) -> Self {
        Self {
            state: CoreLifecycleState::Ready,
            read_only: false,
            safe_mode: false,
            restart_attempt: 0,
            status_code: "core.ready".to_owned(),
            status_message: status_message.to_owned(),
            busy: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BinaryStreamFrame {
    pub stream_id: StreamId,
    pub chunk_sequence: u64,
    pub end_of_message: bool,
    pub data_length: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IpcRequestTerminal {
    Succeeded,
    Failed,
    Cancelled,
    TransportLost,
    ProtocolViolation,
    CoreRestarted,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IpcErrorEnvelope {
    pub code: ErrorCode,
    pub category: ErrorCategory,
    pub message: String,
    pub retryable: bool,
    pub correlation_id: CorrelationId,
    pub terminal: IpcRequestTerminal,
    pub details: BTreeMap<String, Value>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelDisposition {
    Accepted,
    TooLateToCancel,
    UnknownRequest,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CancellationRequest {
    pub request_id: CorrelationId,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveInstallState {
    pub active: String,
    pub release_descriptor: ReleaseDescriptor,
    pub previous: Option<String>,
    pub pending: Option<String>,
    pub last_known_good: Option<String>,
}
