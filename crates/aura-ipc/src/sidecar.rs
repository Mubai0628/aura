use std::{io, time::Duration};

use aura_domain_common::{ContentHash, CorrelationId};
use aura_runtime_contract::{
    RuntimeHandshake, SidecarHandshakeChallenge, SidecarHandshakeHello, SidecarHandshakeProof,
    SidecarHandshakeTranscript, SidecarReady, sidecar_bootstrap_proof,
};
use aura_windows_platform::open_secure_byte_pipe_client;
use serde::Serialize;
use thiserror::Error;
use tokio::{net::windows::named_pipe::NamedPipeClient, time};
use zeroize::Zeroize;

use crate::{
    JSON_RPC_VERSION, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, WireFrame, method,
    read_frame, write_frame,
};

#[derive(Debug, Error)]
pub enum SidecarHandshakeError {
    #[error("sidecar transport failed: {0}")]
    Io(#[from] io::Error),
    #[error("sidecar frame failed: {0}")]
    Frame(#[from] crate::FrameDecodeError),
    #[error("sidecar JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("sidecar handshake protocol violation")]
    Protocol,
}

pub struct SidecarBootstrapConfig {
    pub pipe_name: String,
    pub handshake: RuntimeHandshake,
    pub process_id: u32,
    pub process_creation_time_100ns: u64,
    pub bootstrap_secret: [u8; 32],
}

impl Drop for SidecarBootstrapConfig {
    fn drop(&mut self) {
        self.bootstrap_secret.zeroize();
    }
}

pub async fn connect_authenticated_sidecar<F>(
    mut config: SidecarBootstrapConfig,
    validate_server: F,
) -> Result<(NamedPipeClient, SidecarReady), SidecarHandshakeError>
where
    F: FnOnce(&NamedPipeClient) -> io::Result<()>,
{
    let mut client = connect_with_deadline(&config.pipe_name, Duration::from_secs(10)).await?;
    validate_server(&client)?;
    let hello_id = CorrelationId::new();
    let hello = SidecarHandshakeHello {
        handshake: config.handshake.clone(),
        process_id: config.process_id,
        process_creation_time_100ns: config.process_creation_time_100ns,
    };
    write_request(&mut client, hello_id, method::SIDECAR_HANDSHAKE_HELLO, &hello).await?;
    let challenge: SidecarHandshakeChallenge = read_success(&mut client, hello_id).await?;
    let parent_runtime_instance_id =
        config.handshake.parent_runtime_instance_id.ok_or(SidecarHandshakeError::Protocol)?;
    let transcript = SidecarHandshakeTranscript {
        parent_runtime_instance_id,
        child_runtime_instance_id: config.handshake.runtime_instance_id,
        process_kind: config.handshake.process_kind,
        process_id: config.process_id,
        process_creation_time_100ns: config.process_creation_time_100ns,
        transport_binding_hash: ContentHash::blake3(config.pipe_name.as_bytes()),
        challenge_nonce: challenge.challenge_nonce,
    };
    let proof = sidecar_bootstrap_proof(&config.bootstrap_secret, &transcript)?;
    config.bootstrap_secret.zeroize();
    let proof_id = CorrelationId::new();
    write_request(
        &mut client,
        proof_id,
        method::SIDECAR_HANDSHAKE_PROOF,
        &SidecarHandshakeProof { transcript, proof },
    )
    .await?;
    let ready: SidecarReady = read_success(&mut client, proof_id).await?;
    if ready.runtime_instance_id != config.handshake.runtime_instance_id {
        return Err(SidecarHandshakeError::Protocol);
    }
    Ok((client, ready))
}

async fn connect_with_deadline(
    pipe_name: &str,
    deadline: Duration,
) -> Result<NamedPipeClient, SidecarHandshakeError> {
    let started = time::Instant::now();
    loop {
        match open_secure_byte_pipe_client(pipe_name) {
            Ok(client) => return Ok(client),
            Err(error) if started.elapsed() < deadline => {
                let _ = error;
                time::sleep(Duration::from_millis(25)).await;
            }
            Err(error) => return Err(error.into()),
        }
    }
}

async fn write_request<T: Serialize>(
    client: &mut NamedPipeClient,
    id: CorrelationId,
    method_name: &str,
    params: &T,
) -> Result<(), SidecarHandshakeError> {
    let request = JsonRpcRequest {
        jsonrpc: JSON_RPC_VERSION.to_owned(),
        id,
        command_invocation_id: None,
        method: method_name.to_owned(),
        params: serde_json::to_value(params)?,
    };
    let payload = serde_json::to_vec(&request)?;
    write_frame(client, &WireFrame::Control(payload)).await?;
    Ok(())
}

async fn read_success<T: serde::de::DeserializeOwned>(
    client: &mut NamedPipeClient,
    expected_id: CorrelationId,
) -> Result<T, SidecarHandshakeError> {
    let WireFrame::Control(payload) = read_frame(client).await? else {
        return Err(SidecarHandshakeError::Protocol);
    };
    let JsonRpcMessage::Response(JsonRpcResponse { id, result: Some(result), error: None, .. }) =
        serde_json::from_slice(&payload)?
    else {
        return Err(SidecarHandshakeError::Protocol);
    };
    if id != expected_id {
        return Err(SidecarHandshakeError::Protocol);
    }
    serde_json::from_value(result.value).map_err(Into::into)
}
