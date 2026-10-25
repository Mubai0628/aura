//! One sign-in request through Core's network gateway. Metadata, registration and tokens go
//! under a grant for exactly their URL (`McpOAuthRequestGrant`); the first look at the server
//! itself goes under the server's own grant, without any credential, to read its challenge.
use crate::mcp::McpNetwork;
use crate::mcp::http::describe_network;
use aura_domain_common::{CorrelationId, NetworkRequestId, PrincipalId, UtcTimestamp};
use aura_network_control::{GatewayRequest, McpHttpRequestGrant, McpOAuthRequestGrant};
use aura_permission_domain::{NetworkByteLimits, NetworkRequestPlan, RedirectPolicy};
use aura_security_contract::{
    InvocationActor, InvocationIdentity, NetworkCallerKind, NetworkDataClass,
};
use std::time::Duration;

const MAX_BODY: usize = 1 << 20;
const TIMEOUT: Duration = Duration::from_secs(30);

/// What one request answered.
pub(super) struct Answer {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Answer {
    pub(super) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub(super) fn json(&self) -> Result<serde_json::Value, String> {
        serde_json::from_slice(&self.body).map_err(|_| "回应不是合法的 JSON".to_owned())
    }
}

fn plan(method: &str, url: &str) -> NetworkRequestPlan {
    let limit = u64::try_from(MAX_BODY).unwrap_or(u64::MAX);
    NetworkRequestPlan {
        request_id: NetworkRequestId::new(),
        identity: InvocationIdentity {
            principal_id: PrincipalId::new(),
            actor: InvocationActor::SystemRuntime,
            workspace_id: None,
            session_id: None,
            turn_task_id: None,
            agent_run_id: None,
            parent_agent_run_id: None,
            correlation_id: CorrelationId::new(),
            permission_snapshot_id: None,
        },
        caller: NetworkCallerKind::McpHttp,
        method: method.into(),
        requested_url: url.into(),
        destination: None,
        request_data: Vec::new(),
        data_classes: vec![NetworkDataClass::Unknown],
        redirect_policy: RedirectPolicy::Deny,
        byte_limits: NetworkByteLimits {
            max_upload_bytes: 64 * 1024,
            max_download_bytes: limit,
            max_decompressed_bytes: limit,
        },
        timeout_ms: u64::try_from(TIMEOUT.as_millis()).unwrap_or(u64::MAX),
        credential_refs: Vec::new(),
        network_grant_ids: Vec::new(),
    }
}

fn expires() -> UtcTimestamp {
    UtcTimestamp::from_value(UtcTimestamp::now().value() + time::Duration::seconds(90))
}

fn read(
    network: &McpNetwork,
    open: impl std::future::Future<
        Output = Result<
            aura_network_control::GatewayStreamResponse,
            aura_network_control::NetworkControlError,
        >,
    >,
) -> Result<Answer, String> {
    network.runtime.block_on(async {
        let work = async {
            let mut response = open.await.map_err(|error| describe_network(&error))?;
            let mut body = Vec::new();
            while let Some(chunk) =
                response.body.next_chunk().await.map_err(|error| describe_network(&error))?
            {
                body.extend_from_slice(&chunk);
                if body.len() > MAX_BODY {
                    return Err("回应太大".to_owned());
                }
            }
            Ok(Answer { status: response.status, headers: response.headers, body })
        };
        tokio::time::timeout(TIMEOUT + Duration::from_secs(5), work)
            .await
            .map_err(|_| "连接超时".to_owned())?
    })
}

/// A metadata, registration or token request to exactly `url`.
pub(super) fn send(
    network: &McpNetwork,
    proxy: Option<aura_tool_contract::ToolNetworkProxyV1>,
    method: &str,
    url: &str,
    body: Option<(&str, Vec<u8>)>,
) -> Result<Answer, String> {
    let mut headers = vec![("accept".to_owned(), "application/json".to_owned())];
    let body = match body {
        Some((content_type, bytes)) => {
            headers.push(("content-type".to_owned(), content_type.to_owned()));
            bytes
        }
        None => Vec::new(),
    };
    let request = GatewayRequest { plan: plan(method, url), headers, body };
    let grant = McpOAuthRequestGrant::issue(url, proxy, &request, expires())
        .map_err(|error| describe_network(&error))?;
    read(network, network.gateway.stream(grant, request))
}

/// The server's answer to a first request without a credential: a server that wants a
/// sign-in answers 401 with its challenge.
pub(super) fn probe(
    network: &McpNetwork,
    proxy: Option<aura_tool_contract::ToolNetworkProxyV1>,
    endpoint: &str,
    configured_headers: &[(String, String)],
) -> Result<Answer, String> {
    let mut headers = vec![
        ("accept".to_owned(), "application/json, text/event-stream".to_owned()),
        ("content-type".to_owned(), "application/json".to_owned()),
    ];
    headers.extend(configured_headers.iter().cloned());
    let names: Vec<String> = configured_headers.iter().map(|(name, _)| name.clone()).collect();
    let id = aura_mcp::RequestId::Number(0);
    let body = aura_mcp::encode_request(
        &id,
        "initialize",
        Some(aura_mcp::initialize_params(env!("CARGO_PKG_VERSION"))),
    );
    let request = GatewayRequest { plan: plan("POST", endpoint), headers, body };
    let grant = McpHttpRequestGrant::issue(endpoint, &names, proxy, &request, expires())
        .map_err(|error| describe_network(&error))?;
    read(network, network.gateway.stream(grant, request))
}
