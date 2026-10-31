//! Package HTTP tools in the real Agent path: the exact declared request through
//! Core's controlled HTTP, the package-bound session answer and the result check.
use super::super::l01_user_proxy::{CERTIFICATE, read_headers, tls_acceptor};
use super::support::*;
use super::*;
use aura_network_control::TestFixtureDnsRecord;
use aura_package_service::package_program::tests::support::Kind;
use std::{
    net::{IpAddr, Ipv4Addr},
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const HOST: &str = "proxy-target.test";

/// A loopback HTTPS service that answers `{"ok":true}` and records each request.
struct Service {
    port: u16,
    requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Service {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Service {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("loopback service");
        let port = listener.local_addr().expect("service port").port();
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
                    let length = text
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|value| value.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    let mut body = vec![0_u8; length];
                    if io.read_exact(&mut body).await.is_err() {
                        return;
                    }
                    observed
                        .lock()
                        .expect("requests")
                        .push(format!("{text}{}", String::from_utf8_lossy(&body)));
                    let answer = b"{\"ok\":true}";
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        answer.len()
                    );
                    let _ = io.write_all(header.as_bytes()).await;
                    let _ = io.write_all(answer).await;
                    let _ = io.shutdown().await;
                });
            }
        });
        Self { port, requests, task }
    }
    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("requests").clone()
    }
}

fn loopback(f: &Fixture) {
    f.service()
        .agent_execution_required()
        .web
        .install_l01_fixtures(
            vec![TestFixtureDnsRecord {
                host: HOST.into(),
                classified_addresses: vec![IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))],
                connect_addresses: vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
            }],
            CERTIFICATE.to_vec(),
        )
        .expect("install loopback fixtures");
}

#[tokio::test]
async fn l04_http_tool_sends_exactly_the_declared_request_and_checks_the_answer() {
    let service = Service::start().await;
    let port = service.port;
    let (mut f, installed) =
        Fixture::with_packages(PermissionMode::ReadOnly, &[], move |runtime| {
            install(runtime, Kind::Http, move |raw| {
                let request = &mut raw["tools"][0]["runtime"]["request"];
                request["credential_slot"] = serde_json::Value::Null;
                request["target"]["origin"] = json!(format!("https://{HOST}:{port}"));
            })
        })
        .await;
    loopback(&f);
    let name = installed.tool_name();
    let arguments = json!({"resource": "item", "tags": ["a", "b"], "payload": {"n": 1}});
    let batch = f.admit(&[(name.as_str(), arguments.clone())]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let call = &batch.calls[&id];
    assert_eq!(
        call.operation.content.action["request"]["url"],
        json!(format!("https://{HOST}:{port}/v1/echo/item?tag=a&tag=b"))
    );
    assert_eq!(call.operation.content.action["arguments"], arguments);
    assert!(call.operation.content.credentials.is_empty());
    let proposal = question(&f, call).expect("read-only asks before the network");
    assert!(proposal.allowed_decisions.contains(&ApprovalDecisionKind::AllowSession));
    let grant = answer(&f, call, ApprovalDecisionKind::AllowSession);
    assert!(matches!(
        grant.scope.network_consent.as_ref().map(|consent| &consent.destinations),
        Some(NetworkConsentDestinations::Package { package_id, .. })
            if *package_id == installed.summary.package_id
    ));
    let group = f.execute(batch).await;
    assert_eq!(f.output(id), json!({"ok": true}));
    let seen = service.requests();
    assert_eq!(seen.len(), 1, "one request, no redirect, no retry");
    assert!(seen[0].starts_with("POST /v1/echo/item?tag=a&tag=b HTTP/1.1"), "{}", seen[0]);
    let lower = seen[0].to_ascii_lowercase();
    assert!(lower.contains("x-aura-fixture: manifest-v1"));
    assert!(!lower.contains("authorization"), "no credential was bound or sent");
    assert!(seen[0].ends_with("{\"payload\":{\"n\":1}}"), "{}", seen[0]);
    let (summary, details) = activity(&f, id);
    // A POST that reached the service cannot prove what it changed there.
    assert_eq!(summary, "操作已返回，实际影响尚未确认");
    assert_eq!(details, vec!["由 Aura 代为发出 HTTPS 请求，只访问工具声明的地址".to_owned()]);
    f.continue_after_tools(batch_id, &group);

    // The stored package answer covers the next call: execution does not stop to ask.
    let again = f.admit(&[(name.as_str(), arguments)]);
    let second = again.batch.calls[0].tool_call_id;
    f.execute(again).await;
    assert_eq!(f.output(second), json!({"ok": true}));
    assert_eq!(service.requests().len(), 2);
}

#[tokio::test]
async fn l04_http_tool_with_an_unbound_credential_is_not_exposed() {
    let (f, installed) = Fixture::with_packages(PermissionMode::FullAccess, &[], |runtime| {
        install(runtime, Kind::Http, |_| {})
    })
    .await;
    let tool = &installed.summary.installed.as_ref().expect("installed").tools[0];
    assert!(tool.credential.as_ref().expect("declared slot").binding.is_none());
    let derived = crate::agent_execution::package_tools::installed_tools(
        f.service().broker.lock().expect("package facts").database.connection(),
    )
    .expect("installed tools derive");
    let candidate = derived
        .iter()
        .find(|candidate| candidate.definition.tool_id == tool.tool_id)
        .expect("installed and trusted, so a candidate")
        .candidate();
    assert!(matches!(
        candidate.source_evidence,
        aura_tool_registry::ToolSourceEligibilityEvidenceV1::Package {
            credential_binding_ready: false,
            ..
        }
    ));
    // The fixture offered it as a loaded candidate; the production builder left it out.
    let registry = f.registry();
    assert!(!registry.catalog_entries.iter().any(|entry| entry.tool_id == tool.tool_id));
    assert!(!registry.effective_bindings.iter().any(|binding| binding.tool_id == tool.tool_id));
}
