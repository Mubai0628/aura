//! A request to a trusted MCP server at a public https address goes through the user's
//! proxy, with the host name reaching the proxy; its body is read as a stream.
use super::proxy::{Behavior, proxy};
use super::*;

fn mcp_request(url: &str) -> GatewayRequest {
    let mut request = request(url.to_owned());
    request.plan.caller = NetworkCallerKind::McpHttp;
    request.plan.method = "POST".into();
    request.plan.redirect_policy = RedirectPolicy::Deny;
    request.plan.data_classes = vec![NetworkDataClass::Unknown];
    request.headers = vec![("content-type".into(), "application/json".into())];
    request.body = br#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#.to_vec();
    request
}

#[tokio::test]
async fn an_mcp_request_to_a_public_https_address_uses_the_users_proxy() {
    let target = target(true, vec![Reply::ok()]).await;
    let proxy = proxy(ToolNetworkProxyProtocolV1::Http, Behavior::Relay, &[target.address]).await;
    let gateway = gateway(&[("proxy-target.test", &["8.8.8.8"])]);
    let url = target.url("https", "proxy-target.test");
    let request = mcp_request(&url);
    let expires = aura_domain_common::UtcTimestamp::from_value(
        aura_domain_common::UtcTimestamp::now().value() + time::Duration::minutes(1),
    );
    let grant = crate::McpHttpRequestGrant::issue(
        &url,
        &[],
        Some(proxy.settings.clone()),
        &request,
        expires,
    )
    .expect("a grant for the trusted address");
    let mut response = gateway.stream(grant, request).await.expect("through the proxy");
    let mut body = Vec::new();
    while let Some(chunk) = response.body.next_chunk().await.expect("body") {
        body.extend_from_slice(&chunk);
    }
    assert_eq!(body, b"local proxy fixture");
    assert_eq!(
        proxy.destinations.lock().expect("destinations").as_slice(),
        &[("proxy-target.test".into(), target.address.port(), true)],
        "the host name reaches the proxy"
    );
    let sent = target.requests.lock().expect("requests");
    let [received] = sent.as_slice() else { panic!("one request: {}", sent.len()) };
    assert!(received.starts_with(b"POST /resource "), "{}", String::from_utf8_lossy(received));
}
