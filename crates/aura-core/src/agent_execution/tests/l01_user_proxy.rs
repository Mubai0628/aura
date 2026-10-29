//! Frozen step settings route public HTTPS. The admitted plan itself stays direct.
use super::*;
use aura_network_control::{
    GatewayHopRouteV1, GatewayRouteV1, TestFixtureDnsRecord, UserProxyFailureKindV1,
};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        ServerConfig,
        pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    },
};

pub(super) const CERTIFICATE: &[u8] =
    include_bytes!("../../../../../fixtures/network/l02/certificate.pem");
// Same public synthetic key as the network-control proxy fixture. Not an owner credential.
const KEY: &[u8] = br#"-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgWnMihkcZABZdwVb3
6z7FPjDKVr5urHHr0F0i2+c6WmqhRANCAATghvDuEXFevIkWyS8vQDeTQF7VZBFC
/9J/SyLQ9KgQZIMV6E/sY5Fh/5a+rF4iiFiXDZnGPbHYvT+C/hUzg7RK
-----END PRIVATE KEY-----
"#;

struct Target {
    port: u16,
    requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Target {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Target {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("tls target");
        let port = listener.local_addr().expect("target port").port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&requests);
        let acceptor = tls_acceptor();
        let task = tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else { break };
                let acceptor = acceptor.clone();
                let observed = Arc::clone(&observed);
                tokio::spawn(async move {
                    let Ok(mut io) = acceptor.accept(socket).await else { return };
                    let Ok(head) = read_headers(&mut io).await else { return };
                    let text = String::from_utf8_lossy(&head).into_owned();
                    observed.lock().expect("requests").push(text);
                    let body = b"<html><body>proxied-ok<ul><li class=\"b_algo\"><h2><a href=\"https://example.com/aura\">Aura fixture result</a></h2><p>Local proxy search fixture</p></li></ul></body></html>";
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = io.write_all(header.as_bytes()).await;
                    let _ = io.write_all(body).await;
                });
            }
        });
        Self { port, requests, task }
    }
    fn count(&self) -> usize {
        self.requests.lock().expect("requests").len()
    }
}

struct Proxy {
    port: u16,
    authorities: Arc<Mutex<Vec<String>>>,
    first_bytes: Arc<Mutex<Vec<u8>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Proxy {
    async fn start(target: SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("connect proxy");
        let port = listener.local_addr().expect("proxy port").port();
        let authorities = Arc::new(Mutex::new(Vec::new()));
        let first_bytes = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&authorities);
        let bytes = Arc::clone(&first_bytes);
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut client, _)) = listener.accept().await else { break };
                let seen = Arc::clone(&seen);
                let bytes = Arc::clone(&bytes);
                tokio::spawn(async move {
                    let Ok(head) = read_headers(&mut client).await else { return };
                    let text = String::from_utf8_lossy(&head);
                    let authority = text.split_whitespace().nth(1).unwrap_or("").to_owned();
                    seen.lock().expect("authorities").push(authority);
                    if !text.starts_with("CONNECT ") {
                        return;
                    }
                    if client
                        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                        .await
                        .is_err()
                    {
                        return;
                    }
                    let mut first = [0_u8; 1];
                    if client.read_exact(&mut first).await.is_err() {
                        return;
                    }
                    bytes.lock().expect("first byte").push(first[0]);
                    let Ok(mut upstream) = TcpStream::connect(target).await else { return };
                    if upstream.write_all(&first).await.is_err() {
                        return;
                    }
                    let (mut client_read, mut client_write) = client.into_split();
                    let (mut upstream_read, mut upstream_write) = upstream.into_split();
                    let _ = tokio::join!(
                        tokio::io::copy(&mut client_read, &mut upstream_write),
                        tokio::io::copy(&mut upstream_read, &mut client_write),
                    );
                });
            }
        });
        Self { port, authorities, first_bytes, task }
    }
    fn authority(&self) -> String {
        self.authorities.lock().expect("authorities").join(",")
    }
    fn first(&self) -> Vec<u8> {
        self.first_bytes.lock().expect("first byte").clone()
    }
}

pub(super) fn tls_acceptor() -> TlsAcceptor {
    let certs = CertificateDer::pem_slice_iter(CERTIFICATE)
        .collect::<Result<Vec<_>, _>>()
        .expect("l01 certificate");
    let key = PrivateKeyDer::from_pem_slice(KEY).expect("synthetic test key");
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .expect("local tls");
    TlsAcceptor::from(Arc::new(config))
}

pub(super) async fn read_headers(io: &mut (impl AsyncReadExt + Unpin)) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        let byte = io.read_u8().await?;
        bytes.push(byte);
        if bytes.ends_with(b"\r\n\r\n") {
            return Ok(bytes);
        }
        if bytes.len() > 16_384 {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
    }
}

async fn closed_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("closed port");
    let port = listener.local_addr().expect("closed port").port();
    drop(listener);
    port
}

fn install(f: &super::fixture::Fixture, host: &str) {
    f.service()
        .agent_execution_required()
        .web
        .install_l01_fixtures(
            vec![TestFixtureDnsRecord {
                host: host.into(),
                classified_addresses: vec![IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))],
                connect_addresses: vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
            }],
            CERTIFICATE.to_vec(),
        )
        .expect("install loopback fixtures");
}

fn freeze_proxy(f: &super::fixture::Fixture, port: u16) {
    let proxy =
        ToolNetworkProxyV1::new(true, ToolNetworkProxyProtocolV1::Http, "127.0.0.1".into(), port)
            .expect("loopback proxy");
    let settings = f.settings.clone().with_network_proxy(Some(proxy)).expect("proxy snapshot");
    assert!(f.settings.network_proxy.is_none(), "admission settings stay direct");
    f.service()
        .broker
        .lock()
        .expect("journal")
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::agent::application::freeze_step_tool_settings(
                tx,
                f.step,
                &settings,
                UtcTimestamp::now(),
            )
        })
        .expect("freeze the step proxy");
}

fn approve(f: &super::fixture::Fixture, call: &CallContext) {
    let engine = f.service().agent_execution_required();
    let backend = crate::agent_execution::broker::descriptor(engine.runtime);
    let policy = call.policy(UtcTimestamp::now(), engine.runtime);
    let PolicyAuthorization::Ask { proposal } =
        aura_permission_engine::policy::authorize(&policy, &backend, None)
    else {
        return;
    };
    let now = UtcTimestamp::now();
    let request = AuthorizationApproval {
        approval_id: ApprovalRequestId::new(),
        revision: Revision::INITIAL,
        scope: proposal.scope,
        allowed_decisions: proposal.allowed_decisions,
        state: ApprovalState::Pending,
        requested_at: now,
        expires_at: UtcTimestamp::from_value(now.value() + time::Duration::minutes(10)),
    };
    f.service()
        .broker
        .lock()
        .expect("approval")
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::security::insert_approval(tx, &request)?;
            aura_persistence_v2::security_approval::resolve(
                tx,
                request.approval_id,
                request.revision,
                &call.operation,
                ApprovalDecisionKind::AllowSession,
                backend.class,
                call.snapshot.session_permission_revision,
                engine.runtime,
                now,
            )
        })
        .expect("persist approval")
        .expect("allow");
}

fn route(f: &super::fixture::Fixture, operation: SecurityOperationId) -> Vec<GatewayHopRouteV1> {
    let store = f.service().broker.lock().expect("evidence");
    let db = store.database.connection();
    let attempt = aura_persistence_v2::execution::load(db, operation, 1).expect("attempt");
    let aura_execution_contract::AttemptState::Terminal { result } = &attempt.state else {
        panic!("attempt is not terminal");
    };
    let bytes = aura_persistence_core::artifact::read(
        db,
        result.receipt.evidence_ref,
        &operation.to_string(),
    )
    .expect("web evidence");
    let evidence: serde_json::Value = serde_json::from_slice(&bytes).expect("evidence json");
    serde_json::from_value(evidence["network_route"].clone()).expect("network route")
}

fn activity_details(f: &super::fixture::Fixture, id: ToolCallId) -> Vec<String> {
    let store = f.service().broker.lock().expect("activity");
    let page = aura_persistence_v2::tool::activity::page(
        store.database.connection(),
        &aura_tool_projection::AgentToolActivityRequest {
            session_id: f.turn.session_id,
            before_call_id: None,
            limit: 20,
            call_ids: Some(vec![id]),
        },
    )
    .expect("activity page");
    page.calls.into_iter().flat_map(|call| call.display.details).collect()
}

fn paired(f: &super::fixture::Fixture, id: ToolCallId) -> aura_tool_contract::ToolResultV1 {
    let store = f.service().broker.lock().expect("result");
    aura_persistence_v2::tool::execution::call(store.database.connection(), id)
        .expect("call")
        .result
        .expect("paired result")
}

async fn fixture(names: &[&str]) -> super::fixture::Fixture {
    super::fixture::Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, names).await
}

#[tokio::test]
async fn l01_http_request_uses_the_frozen_proxy_and_hides_the_route_from_the_model() {
    let target = Target::start().await;
    let proxy = Proxy::start(SocketAddr::from(([127, 0, 0, 1], target.port))).await;
    let f = fixture(&["http.request"]).await;
    install(&f, "proxy-target.test");
    freeze_proxy(&f, proxy.port);
    let batch = f.admit(&[(
        "http.request",
        serde_json::json!({"url": format!("https://proxy-target.test:{}/resource", target.port)}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    let operation = batch.calls[&id].operation.operation_id;
    approve(&f, &batch.calls[&id]);
    f.execute(batch).await;
    let output = f.output(id);
    assert_eq!(output["status"], 200);
    assert!(output["text"].as_str().unwrap_or("").contains("proxied-ok"));
    assert!(output.get("network_route").is_none());
    assert_eq!(target.count(), 1);
    assert!(proxy.authority().contains(&format!("proxy-target.test:{}", target.port)));
    assert_eq!(proxy.first(), vec![0x16], "the tunnel starts with TLS, not a visible GET");
    let hops = route(&f, operation);
    assert!(matches!(
        hops[0].route,
        GatewayRouteV1::UserProxy { protocol: ToolNetworkProxyProtocolV1::Http, ref host, port }
            if host == "127.0.0.1" && port == proxy.port
    ));
    assert!(hops[0].proxy_failure.is_none());
    let result = paired(&f, id);
    assert!(matches!(result.outcome, ToolResultOutcomeV1::Succeeded));
    assert_eq!(result.side_effect_state, SideEffectStateV1::NoEffect);
    let details = activity_details(&f, id);
    assert!(details.iter().any(|line| line == "经代理"), "{details:?}");
    assert!(!details.iter().any(|line| line.contains("已改走直连")), "{details:?}");
}

#[tokio::test]
async fn l01_unreachable_proxy_falls_back_once_and_keeps_the_proxy_failure() {
    let target = Target::start().await;
    let proxy_port = closed_port().await;
    let f = fixture(&["http.request"]).await;
    install(&f, "proxy-target.test");
    freeze_proxy(&f, proxy_port);
    let batch = f.admit(&[(
        "http.request",
        serde_json::json!({"url": format!("https://proxy-target.test:{}/resource", target.port)}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    let operation = batch.calls[&id].operation.operation_id;
    approve(&f, &batch.calls[&id]);
    f.execute(batch).await;
    assert_eq!(f.output(id)["status"], 200);
    assert_eq!(target.count(), 1, "direct recovery sends the request once");
    let hops = route(&f, operation);
    assert!(matches!(hops[0].route, GatewayRouteV1::Direct));
    assert_eq!(
        hops[0].proxy_failure.as_ref().expect("proxy reason").kind,
        UserProxyFailureKindV1::Unavailable
    );
    let result = paired(&f, id);
    assert!(matches!(result.outcome, ToolResultOutcomeV1::Succeeded));
    assert_eq!(result.side_effect_state, SideEffectStateV1::NoEffect);
    // A stopped proxy is not silent when the direct attempt succeeds.
    let details = activity_details(&f, id);
    assert!(!details.iter().any(|line| line == "经代理"), "{details:?}");
    assert!(
        details.iter().any(|line| line.starts_with(&format!("代理 127.0.0.1:{proxy_port} 不可用"))
            && line.ends_with("已改走直连")),
        "the row must say the proxy was down and the call went direct: {details:?}"
    );
}

#[tokio::test]
async fn l01_proxy_and_direct_failure_before_http_does_not_claim_a_replay() {
    let port = closed_port().await;
    let proxy_port = closed_port().await;
    let f = fixture(&["http.request"]).await;
    install(&f, "proxy-target.test");
    freeze_proxy(&f, proxy_port);
    let batch = f.admit(&[(
        "http.request",
        serde_json::json!({"url": format!("https://proxy-target.test:{port}/resource")}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    approve(&f, &batch.calls[&id]);
    f.execute_rejecting(batch).await.expect("batch answered");
    let result = paired(&f, id);
    let ToolResultOutcomeV1::Failed { error } = result.outcome else {
        panic!("expected a failed GET, got {:?}", result.outcome);
    };
    assert!(error.message.contains("network.user_proxy_unavailable"), "{}", error.message);
    assert!(error.message.contains("尚未向目标发出 HTTP 请求"), "{}", error.message);
    assert!(!error.message.contains("没有自动重放"), "{}", error.message);
    assert!(!error.message.contains("已经发出"), "{}", error.message);
    assert_eq!(result.side_effect_state, SideEffectStateV1::NoEffect);
}

/// Nothing listens at the target (a development server that ended with Aura's restart): the
/// result names the target and the next step, is a retryable transport failure, and is not shown
/// as a tool that is unavailable. The path stays with the model's own call.
#[tokio::test]
async fn a_target_with_nothing_listening_is_named_with_what_to_do_next() {
    let port = closed_port().await;
    let f = fixture(&["http.request"]).await;
    install(&f, "proxy-target.test");
    let batch = f.admit(&[(
        "http.request",
        serde_json::json!({"url": format!("https://proxy-target.test:{port}/api/health")}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    approve(&f, &batch.calls[&id]);
    f.execute_rejecting(batch).await.expect("batch answered");
    let result = paired(&f, id);
    let ToolResultOutcomeV1::Failed { error } = result.outcome else {
        panic!("expected a failed GET, got {:?}", result.outcome);
    };
    assert_eq!(error.code.as_str(), "tool.network.connection_refused", "{}", error.message);
    assert_eq!(error.category, aura_domain_common::ErrorCategory::Transport);
    assert!(error.retryable, "nothing was sent, so trying again is the next step");
    assert!(
        error.message.contains(&format!("proxy-target.test:{port} 没有程序在监听")),
        "{}",
        error.message
    );
    assert!(error.message.contains("先启动它"), "{}", error.message);
    assert!(!error.message.contains("/api/health"), "{}", error.message);
    assert_eq!(result.side_effect_state, SideEffectStateV1::NoEffect);
}

#[tokio::test]
async fn l01_fetch_download_and_search_use_the_frozen_proxy() {
    let target = Target::start().await;
    let proxy = Proxy::start(SocketAddr::from(([127, 0, 0, 1], target.port))).await;
    let fetch = fixture(&["web.fetch"]).await;
    install(&fetch, "proxy-target.test");
    freeze_proxy(&fetch, proxy.port);
    let batch = fetch.admit(&[(
        "web.fetch",
        serde_json::json!({
            "url": format!("https://proxy-target.test:{}/resource", target.port),
            "mode": "http_only",
            "max_bytes": 4096
        }),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    approve(&fetch, &batch.calls[&id]);
    fetch.execute(batch).await;
    assert!(fetch.output(id)["excerpt"].as_str().unwrap_or("").contains("proxied-ok"));
    assert!(proxy.authority().contains(&format!("proxy-target.test:{}", target.port)));

    let download = fixture(&["http.download"]).await;
    install(&download, "proxy-target.test");
    freeze_proxy(&download, proxy.port);
    let batch = download.admit(&[(
        "http.download",
        serde_json::json!({
            "url": format!("https://proxy-target.test:{}/resource", target.port),
            "path": "l01.txt"
        }),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    approve(&download, &batch.calls[&id]);
    download.execute(batch).await;
    let saved = std::fs::read_to_string(download.root.join("l01.txt")).expect("download");
    assert!(saved.contains("proxied-ok"), "{saved}");

    let before = proxy.authority();
    let search = fixture(&["web.search"]).await;
    install(&search, "cn.bing.com");
    freeze_proxy(&search, proxy.port);
    let batch =
        search.admit(&[("web.search", serde_json::json!({"query": "aura", "max_results": 1}))]);
    let id = batch.batch.calls[0].tool_call_id;
    approve(&search, &batch.calls[&id]);
    search.execute_rejecting(batch).await.expect("search answered");
    assert!(
        proxy.authority().contains("cn.bing.com:443"),
        "search must open a proxy tunnel, saw {} then {}",
        before,
        proxy.authority()
    );
    let result = paired(&search, id);
    assert!(
        matches!(result.outcome, ToolResultOutcomeV1::Succeeded),
        "search must finish through the local TLS proxy: {:?}",
        result.outcome
    );
    assert_eq!(search.output(id)["hits"][0]["title"], "Aura fixture result");
    assert_eq!(search.output(id)["hits"][0]["url"], "https://example.com/aura");
}

#[tokio::test]
async fn l02_b0_unsent_mutations_have_no_effect_with_or_without_a_proxy() {
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        for proxied in [false, true] {
            let f = fixture(&["http.request"]).await;
            install(&f, "proxy-target.test");
            if proxied {
                freeze_proxy(&f, closed_port().await);
            }
            let port = closed_port().await;
            let batch = f.admit(&[(
                "http.request",
                serde_json::json!({
                    "url": format!("https://proxy-target.test:{port}/mutation"),
                    "method": method,
                    "body": {"kind":"text", "text":"temporary-test-value"}
                }),
            )]);
            let id = batch.batch.calls[0].tool_call_id;
            approve(&f, &batch.calls[&id]);
            f.execute_rejecting(batch).await.expect("paired failure");
            let result = paired(&f, id);
            assert_eq!(
                result.side_effect_state,
                SideEffectStateV1::NoEffect,
                "{method}, proxy={proxied}: {:?}",
                result.outcome
            );
            assert!(matches!(result.outcome, ToolResultOutcomeV1::Failed { .. }));
        }
    }
}
