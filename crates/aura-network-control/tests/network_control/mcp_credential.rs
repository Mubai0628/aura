//! A server's secret reaches it only as the gateway adds it after the secret scan,
//! and an answer that carries the secret back (in one chunk or split across two) is refused
//! before the caller reads it.
use aura_domain_common::{
    CorrelationId, CredentialId, CredentialRevision, NetworkRequestId, SessionId, UtcTimestamp,
    WorkspaceId,
};
use aura_network_control::{
    CoreHttpClientGateway, GatewayRequest, InMemoryNetworkGrantStore, McpHttpRequestGrant,
    NetworkControlError, NetworkControlOptions, ResolvedToolCredential, ToolCredentialBinding,
    ToolCredentialResolver,
};
use aura_permission_domain::{
    NetworkByteLimits, NetworkRequestPlan, NormalizedOrigin, RedirectPolicy,
    ToolCredentialPlacementV1, ToolCredentialUseV1,
};
use aura_security_contract::{InvocationIdentity, NetworkCallerKind, NetworkDataClass};
use aura_tool_contract::{CredentialSlotKey, EnvironmentVariableNameV1, HttpHeaderNameV1};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const SECRET: &str = "s3cret-value-for-the-mcp-test";

/// One HTTP exchange per connection: records the request, then writes `parts` with a pause
/// between them so the client reads them as separate chunks.
async fn server(parts: Vec<Vec<u8>>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listen");
    let url = format!("http://{}/mcp", listener.local_addr().expect("address"));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    tokio::spawn(async move {
        let Ok((mut socket, _)) = listener.accept().await else { return };
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            match socket.read(&mut buffer).await {
                Ok(0) | Err(_) => return,
                Ok(read) => bytes.extend_from_slice(&buffer[..read]),
            }
        }
        record.lock().expect("record").push(String::from_utf8_lossy(&bytes).into_owned());
        let _ = socket
            .write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n",
            )
            .await;
        for part in parts {
            let _ = socket.write_all(&part).await;
            let _ = socket.flush().await;
            tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        }
    });
    (url, seen)
}

fn request(url: &str) -> GatewayRequest {
    GatewayRequest {
        plan: NetworkRequestPlan {
            request_id: NetworkRequestId::new(),
            identity: InvocationIdentity::runtime_user(
                WorkspaceId::new(),
                SessionId::new(),
                CorrelationId::new(),
            ),
            caller: NetworkCallerKind::McpHttp,
            method: "POST".into(),
            requested_url: url.into(),
            destination: None,
            request_data: vec![],
            data_classes: vec![NetworkDataClass::Unknown],
            redirect_policy: RedirectPolicy::Deny,
            byte_limits: NetworkByteLimits {
                max_upload_bytes: 4096,
                max_download_bytes: 65_536,
                max_decompressed_bytes: 65_536,
            },
            timeout_ms: 5_000,
            credential_refs: vec![],
            network_grant_ids: vec![],
        },
        headers: vec![("content-type".into(), "application/json".into())],
        body: br#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#.to_vec(),
    }
}

struct Fixed(ToolCredentialBinding);

impl ToolCredentialResolver for Fixed {
    fn resolve(
        &self,
        requested: &ToolCredentialUseV1,
    ) -> Result<ResolvedToolCredential, NetworkControlError> {
        assert_eq!(requested.credential_id, self.0.credential_id);
        Ok(ResolvedToolCredential {
            binding: self.0.clone(),
            secret_header_value: format!("Bearer {SECRET}").into_bytes(),
        })
    }
}

/// Opens one stream to `url` with the secret as a bearer and reads it to the end.
async fn read_all(url: &str) -> Result<Vec<u8>, NetworkControlError> {
    let parsed = url::Url::parse(url).expect("url");
    let id = CredentialId::new();
    let revision = CredentialRevision::INITIAL;
    let resolver = Fixed(ToolCredentialBinding {
        credential_id: id,
        credential_revision: revision,
        origin: NormalizedOrigin {
            scheme: "http".into(),
            host: "127.0.0.1".into(),
            port: parsed.port().expect("port"),
        },
        header_name: "authorization".into(),
    });
    let credential = ToolCredentialUseV1 {
        credential_id: id,
        credential_revision: revision,
        slot_key: CredentialSlotKey::new("mcp").expect("slot"),
        placement: ToolCredentialPlacementV1::BearerAuthorization,
    };
    let request = request(url);
    let expires =
        UtcTimestamp::from_value(UtcTimestamp::now().value() + time::Duration::minutes(1));
    let grant = McpHttpRequestGrant::issue_with_credential(
        url,
        &[],
        None,
        Some(credential),
        &request,
        expires,
    )
    .expect("a grant that carries the secret");
    let gateway = CoreHttpClientGateway::new(
        Arc::new(InMemoryNetworkGrantStore::default()),
        None,
        NetworkControlOptions::default(),
    );
    let mut response = gateway.stream_with_credentials(grant, request, Some(&resolver)).await?;
    let mut body = Vec::new();
    while let Some(chunk) = response.body.next_chunk().await? {
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[tokio::test]
async fn the_secret_is_added_by_the_gateway_and_never_comes_back() {
    let (url, seen) = server(vec![b"data: {\"ok\":true}\n\n".to_vec()]).await;
    let body = read_all(&url).await.expect("an answer without the secret");
    assert_eq!(body, b"data: {\"ok\":true}\n\n");
    let sent = seen.lock().expect("seen")[0].to_ascii_lowercase();
    assert!(sent.contains(&format!("authorization: bearer {}", SECRET.to_ascii_lowercase())));

    let (url, _) = server(vec![format!("data: {{\"echo\":\"{SECRET}\"}}\n\n").into_bytes()]).await;
    assert!(matches!(
        read_all(&url).await,
        Err(NetworkControlError::CredentialReflectionDetected { .. })
    ));
    let (half, rest) = SECRET.split_at(SECRET.len() / 2);
    let (url, _) = server(vec![
        format!("data: {{\"echo\":\"{half}").into_bytes(),
        format!("{rest}\"}}\n\n").into_bytes(),
    ])
    .await;
    assert!(
        matches!(
            read_all(&url).await,
            Err(NetworkControlError::CredentialReflectionDetected { .. })
        ),
        "a secret split across two chunks is still found"
    );
}

#[test]
fn a_secret_never_goes_in_an_aura_or_configured_header_or_the_environment() {
    let url = "http://127.0.0.1:7000/mcp";
    let request = request(url);
    let expires =
        UtcTimestamp::from_value(UtcTimestamp::now().value() + time::Duration::minutes(1));
    let placed = |placement: ToolCredentialPlacementV1, configured: &[String]| {
        McpHttpRequestGrant::issue_with_credential(
            url,
            configured,
            None,
            Some(ToolCredentialUseV1 {
                credential_id: CredentialId::new(),
                credential_revision: CredentialRevision::INITIAL,
                slot_key: CredentialSlotKey::new("mcp").expect("slot"),
                placement,
            }),
            &request,
            expires,
        )
        .is_ok()
    };
    let header = |name: &str| ToolCredentialPlacementV1::FixedHeader {
        header_name: HttpHeaderNameV1::new(name).expect("header name"),
    };
    assert!(placed(ToolCredentialPlacementV1::BearerAuthorization, &[]));
    assert!(placed(header("x-api-key"), &[]));
    assert!(!placed(header("content-type"), &[]));
    assert!(!placed(header("x-team"), &["X-Team".to_owned()]));
    assert!(!placed(
        ToolCredentialPlacementV1::Environment {
            environment_variable: EnvironmentVariableNameV1::new("API_TOKEN").expect("variable"),
        },
        &[]
    ));
}
