use super::*;

#[tokio::test]
async fn s07_n4_actual_lan_listener_uses_the_same_ordinary_permission() {
    let address = std::env::var("AURA_S07_LAN_ADDRESS").unwrap_or_else(|_| {
        // UDP connect selects the OS route/source address without sending a packet.
        let route = std::net::UdpSocket::bind("0.0.0.0:0").expect("local route probe");
        route.connect("192.0.2.1:9").expect("a LAN route is required for N4");
        route.local_addr().expect("observed local interface").ip().to_string()
    });
    assert!(!address.starts_with("127."), "N4 must exercise a real non-loopback interface");
    let server = Server::start_at(&address).await;
    let f = fixture(PermissionMode::WorkspaceReadWrite, &["http.request"]).await;
    let batch = f.admit(&[("http.request", json!({"url":server.url}))]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute(batch).await;
    assert_eq!(f.output(id)["status"], 200);
    assert_eq!(server.count(), 1);
}

#[tokio::test]
async fn s07_n4_web_fetch_allows_ordinary_loopback_and_hostname() {
    let server = Server::start().await;
    for url in [server.url.clone(), server.url.replace("127.0.0.1", "localhost")] {
        let f = fixture(PermissionMode::WorkspaceReadWrite, &["web.fetch"]).await;
        let batch = f.admit(&[(
            "web.fetch",
            json!({"url":format!("{url}/page"),"mode":"http_only","max_bytes":4096}),
        )]);
        let id = batch.batch.calls[0].tool_call_id;
        f.execute(batch).await;
        assert!(f.output(id)["document_artifact_id"].is_string());
    }
    assert_eq!(server.count(), 2);
}

#[tokio::test]
async fn s07_n5_get_follows_a_cross_origin_redirect() {
    let first = Server::start().await;
    let second = Server::start().await;
    let f = fixture(PermissionMode::WorkspaceReadWrite, &["http.request"]).await;
    let batch =
        f.admit(&[("http.request", json!({"url":format!("{}/redirect/{}",first.url,second.url)}))]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute(batch).await;
    assert_eq!(f.output(id)["status"], 200);
    assert_eq!(first.count(), 1);
    assert_eq!(second.count(), 1);
}

#[tokio::test]
async fn s07_n5_cross_origin_redirect_preserves_ordinary_model_headers() {
    let first = Server::start().await;
    let second = Server::start().await;
    let f = fixture(PermissionMode::WorkspaceReadWrite, &["http.request"]).await;
    let batch = f.admit(&[(
        "http.request",
        json!({
            "url": format!("{}/redirect/{}", first.url, second.url),
            "headers": [
                {"name": "X-S07-Trace", "value": "kept"}
            ]
        }),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute(batch).await;
    assert_eq!(f.output(id)["status"], 200);
    assert!(first.requests.lock().expect("first hop")[0].contains("kept"));
    let next = second.requests.lock().expect("second hop")[0].to_ascii_lowercase();
    assert!(next.contains("x-s07-trace: kept"), "ordinary headers still follow the redirect");
}

#[test]
fn s07_n2_web_read_is_local_session_authority() {
    let entry = aura_tool_registry::agent_release::release()
        .expect("published release")
        .entry("web.read")
        .expect("web.read exists");
    assert_eq!(
        entry.definition.capabilities,
        vec![ResolvedCapability::AgentTaskRead],
        "reading a fetched document does not require network approval"
    );
}
