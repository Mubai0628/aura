use aura_domain_common::{CorrelationId, SessionId, WorkspaceId};
use aura_net_tools::http_request;
use aura_security_contract::InvocationIdentity;
use serde_json::json;

fn build(input: serde_json::Value) -> anyhow::Result<aura_network_control::GatewayRequest> {
    http_request(
        &serde_json::from_value(input)?,
        InvocationIdentity::runtime_user(
            WorkspaceId::new(),
            SessionId::new(),
            CorrelationId::new(),
        ),
    )
}
#[test]
fn s07_n4_local_private_and_public_urls_have_the_same_request_contract() {
    for url in [
        "http://127.0.0.1:43210/a",
        "http://localhost:43210/a",
        "http://192.168.1.2:43210/a",
        "https://example.invalid/a",
    ] {
        let request = build(json!({"url":url}))
            .expect("syntactic destination validation does not invent network approval");
        assert_eq!(request.plan.method, "GET");
        assert_eq!(request.plan.byte_limits.max_download_bytes, 8 * 1024 * 1024);
    }
}
#[test]
fn s07_n9_invalid_framing_identity_headers_urls_and_upload_budgets_are_rejected() {
    for url in
        ["file:///C:/secret", "http://user:password@localhost/", "http://localhost/#fragment"]
    {
        assert!(build(json!({"url":url})).is_err());
    }
    for (name, value) in [
        ("Host", "elsewhere"),
        ("Content-Length", "7"),
        ("X-Aura-Principal", "someone"),
        ("X-Test", "a\r\nb: c"),
        ("Proxy-Authorization", "credential"),
    ] {
        assert!(
            build(json!({"url":"http://localhost/","headers":[{"name":name,"value":value}]}))
                .is_err()
        );
    }
    assert!(
        build(
            json!({"url":"http://localhost/","method":"GET","body":{"kind":"text","text":"body"}})
        )
        .is_err()
    );
    assert!(build(json!({"url":"http://localhost/","method":"POST","body":{"kind":"text","text":"x".repeat(1024*1024+1)}})).is_err());
}
#[test]
fn s07_n9_post_has_no_automatic_redirect_and_json_is_encoded_once() {
    let request=build(json!({"url":"http://localhost/","method":"post","body":{"kind":"json","value":{"name":"value"}}})).expect("JSON POST");
    assert_eq!(request.plan.method, "POST");
    assert!(matches!(request.plan.redirect_policy, aura_permission_domain::RedirectPolicy::Deny));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&request.body).expect("wire JSON"),
        json!({"name":"value"})
    );
}
