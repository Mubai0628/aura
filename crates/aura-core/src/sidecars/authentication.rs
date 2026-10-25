use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) async fn authenticate_sidecar(
    server: &mut NamedPipeServer,
    options: &CoreOptions,
    release: &CoreReleaseContext,
    component: &SidecarReleaseIdentity,
    runtime_instance_id: RuntimeInstanceId,
    pipe_name: &str,
    expected_process: &ProcessIdentity,
    secret: &[u8; 32],
) -> Result<()> {
    let hello_request = read_request_with_timeout(server).await?;
    if hello_request.method != method::SIDECAR_HANDSHAKE_HELLO {
        bail!("security Sidecar did not begin with handshake hello");
    }
    let hello: SidecarHandshakeHello =
        serde_json::from_value(hello_request.params).context("invalid security Sidecar hello")?;
    validate_handshake(&hello, options, release, component, runtime_instance_id, expected_process)?;
    let mut challenge = [0_u8; 32];
    getrandom::fill(&mut challenge).context("failed to generate Sidecar challenge")?;
    let challenge =
        SidecarHandshakeChallenge { challenge_nonce: SecretNonce::from_bytes(challenge) };
    write_response(
        server,
        JsonRpcResponse::success(hello_request.id, serde_json::to_value(&challenge)?),
    )
    .await?;
    let proof_request = read_request_with_timeout(server).await?;
    if proof_request.method != method::SIDECAR_HANDSHAKE_PROOF {
        bail!("security Sidecar did not answer handshake challenge");
    }
    let proof: SidecarHandshakeProof =
        serde_json::from_value(proof_request.params).context("invalid security Sidecar proof")?;
    let expected_transcript = SidecarHandshakeTranscript {
        parent_runtime_instance_id: options.runtime_instance_id,
        child_runtime_instance_id: runtime_instance_id,
        process_kind: component.process_kind,
        process_id: expected_process.process_id,
        process_creation_time_100ns: expected_process.creation_time_100ns,
        transport_binding_hash: ContentHash::blake3(pipe_name.as_bytes()),
        challenge_nonce: challenge.challenge_nonce,
    };
    if proof.transcript != expected_transcript {
        bail!("security Sidecar challenge transcript was not bound to the frozen process");
    }
    let expected_proof = sidecar_bootstrap_proof(secret, &expected_transcript)?;
    if !constant_time_equal(proof.proof.hex.as_bytes(), expected_proof.hex.as_bytes())
        || proof.proof.algorithm != expected_proof.algorithm
    {
        bail!("security Sidecar bootstrap proof was rejected");
    }
    write_response(
        server,
        JsonRpcResponse::success(
            proof_request.id,
            serde_json::to_value(SidecarReady {
                runtime_instance_id,
                negotiated_protocol_version: ContractVersion::CURRENT,
            })?,
        ),
    )
    .await?;
    Ok(())
}

pub(super) fn validate_handshake(
    hello: &SidecarHandshakeHello,
    options: &CoreOptions,
    release: &CoreReleaseContext,
    component: &SidecarReleaseIdentity,
    runtime_instance_id: RuntimeInstanceId,
    expected_process: &ProcessIdentity,
) -> Result<()> {
    let handshake = &hello.handshake;
    let sandbox_tool_features_available = component.process_kind != RuntimeProcessKind::SandboxHost
        || ["aura.sandbox.authorized-tool-dispatch.v1", "aura.ipc.tool-runtime-secret.v1"]
            .iter()
            .all(|required| handshake.supported_features.iter().any(|actual| actual == required));
    if hello.process_id != expected_process.process_id
        || hello.process_creation_time_100ns != expected_process.creation_time_100ns
        || handshake.process_kind != component.process_kind
        || handshake.runtime_instance_id != runtime_instance_id
        || handshake.parent_runtime_instance_id != Some(options.runtime_instance_id)
        || handshake.release_descriptor != release.release_descriptor
        || handshake.component_version != component.component_version
        || handshake.build_commit != release.build_commit
        || handshake.protocol_version.major != ContractVersion::CURRENT.major
        || handshake.protocol_version.minor > ContractVersion::CURRENT.minor
        || !sandbox_tool_features_available
    {
        bail!("security Sidecar identity/Release Bundle handshake mismatch");
    }
    Ok(())
}

pub(super) async fn read_request_with_timeout(
    server: &mut NamedPipeServer,
) -> Result<JsonRpcRequest> {
    let frame = time::timeout(STARTUP_TIMEOUT, read_frame(server))
        .await
        .context("security Sidecar handshake timed out")??;
    let WireFrame::Control(payload) = frame else {
        bail!("security Sidecar handshake used a binary frame");
    };
    let JsonRpcMessage::Request(request) = serde_json::from_slice(&payload)? else {
        bail!("security Sidecar handshake did not contain a request");
    };
    if request.jsonrpc != JSON_RPC_VERSION {
        bail!("security Sidecar JSON-RPC version mismatch");
    }
    Ok(request)
}

pub(super) async fn write_response(
    server: &mut NamedPipeServer,
    response: JsonRpcResponse,
) -> Result<()> {
    write_frame(server, &WireFrame::Control(serde_json::to_vec(&response)?)).await?;
    Ok(())
}

pub(super) fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter().zip(right).fold(0_u8, |difference, (left, right)| difference | (left ^ right)) == 0
}
