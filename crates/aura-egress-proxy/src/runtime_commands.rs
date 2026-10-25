// Command loop is split from bootstrap verification so the bootstrap secret
// can be consumed and zeroized before any credential is registered.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use aura_domain_common::{ContractVersion, ErrorCategory, ErrorCode};
use aura_ipc::{
    JSON_RPC_VERSION, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, WireFrame, method,
    read_frame, write_frame,
};
use aura_network_control::{
    EgressProxyAuthorization, EgressProxyServer, NetworkControlError, ProxyDnsResolver,
    ProxyTokenAuthority,
};
use aura_permission_domain::{
    RegisterOpaqueTunnelGrantInput, RegisterOpaqueTunnelGrantResult, RevokeEgressExecutionInput,
    RevokeEgressGrantInput,
};
use aura_runtime_contract::{IpcErrorEnvelope, IpcRequestTerminal};
use serde_json::{Value, json};
use tokio::net::windows::named_pipe::NamedPipeClient;

pub async fn serve(mut pipe: NamedPipeClient) -> Result<()> {
    let authority = ProxyTokenAuthority::new();
    let resolver = ProxyDnsResolver::default();
    let server =
        EgressProxyServer::bind_loopback_with_resolver(authority.clone(), resolver.clone())
            .await
            .context("failed to bind authenticated loopback proxy")?;
    let handle = server.spawn().context("failed to start Egress Proxy listener")?;
    let proxy_address = handle.local_addr();
    let mut shutdown = false;
    while !shutdown {
        let WireFrame::Control(payload) = read_frame(&mut pipe).await? else {
            bail!("Egress Proxy control pipe does not accept binary frames");
        };
        let JsonRpcMessage::Request(request) = serde_json::from_slice(&payload)? else {
            bail!("Egress Proxy received an unsolicited JSON-RPC message");
        };
        if request.jsonrpc != JSON_RPC_VERSION {
            bail!("unsupported JSON-RPC version");
        }
        let outcome = handle_request(&authority, &resolver, proxy_address, &request).await;
        let response = match outcome {
            Ok((value, should_shutdown)) => {
                shutdown = should_shutdown;
                JsonRpcResponse::success(request.id, value)
            }
            Err(error) => {
                JsonRpcResponse::failure(request.id, ipc_error(request.id, &error.to_string())?)
            }
        };
        write_frame(&mut pipe, &WireFrame::Control(serde_json::to_vec(&response)?)).await?;
    }
    handle.shutdown().await;
    Ok(())
}

async fn handle_request(
    authority: &ProxyTokenAuthority,
    resolver: &ProxyDnsResolver,
    proxy_address: std::net::SocketAddr,
    request: &JsonRpcRequest,
) -> Result<(Value, bool), NetworkControlError> {
    match request.method.as_str() {
        method::SIDECAR_PING => {
            Ok((json!({ "ready": true, "proxy_endpoint": proxy_address }), false))
        }
        method::EGRESS_REGISTER_OPAQUE_TUNNEL => {
            let input: RegisterOpaqueTunnelGrantInput =
                serde_json::from_value(request.params.clone())
                    .map_err(|_| NetworkControlError::ProxyProtocol)?;
            let authorization = EgressProxyAuthorization::resolve(input.request, resolver).await?;
            let token = authority.issue(authorization.clone())?;
            let result = RegisterOpaqueTunnelGrantResult {
                proxy_host: proxy_address.ip().to_string(),
                proxy_port: proxy_address.port(),
                access_token: token.into_secret_nonce(),
                grant: authorization.grant,
            };
            Ok((
                serde_json::to_value(result)
                    .map_err(|error| NetworkControlError::Transport(error.to_string()))?,
                false,
            ))
        }
        method::EGRESS_REVOKE_GRANT => {
            let input: RevokeEgressGrantInput = serde_json::from_value(request.params.clone())
                .map_err(|_| NetworkControlError::ProxyProtocol)?;
            authority.revoke_grant(input.grant_id)?;
            Ok((json!({ "revoked": true }), false))
        }
        method::EGRESS_REVOKE_EXECUTION => {
            let input: RevokeEgressExecutionInput = serde_json::from_value(request.params.clone())
                .map_err(|_| NetworkControlError::ProxyProtocol)?;
            authority.revoke_execution(input.execution_id, input.execution_lease_id)?;
            Ok((json!({ "revoked": true }), false))
        }
        method::SIDECAR_SHUTDOWN => {
            authority.revoke_all()?;
            Ok((json!({ "stopping": true }), true))
        }
        _ => Err(NetworkControlError::ProxyProtocol),
    }
}

fn ipc_error(
    correlation_id: aura_domain_common::CorrelationId,
    message: &str,
) -> Result<IpcErrorEnvelope> {
    Ok(IpcErrorEnvelope {
        code: ErrorCode::new("egress.request_failed")?,
        category: ErrorCategory::Permission,
        message: message.to_owned(),
        retryable: false,
        correlation_id,
        terminal: IpcRequestTerminal::Failed,
        details: BTreeMap::from([(
            "protocol_version".to_owned(),
            serde_json::to_value(ContractVersion::CURRENT)?,
        )]),
    })
}
