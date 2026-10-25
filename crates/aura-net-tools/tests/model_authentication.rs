use aura_domain_common::{CorrelationId, SessionId, WorkspaceId};
use aura_network_control::NetworkControlError;
use aura_security_contract::InvocationIdentity;
use serde_json::json;

fn build(input: serde_json::Value) -> anyhow::Result<aura_network_control::GatewayRequest> {
    aura_net_tools::http_request(
        &serde_json::from_value(input)?,
        InvocationIdentity::runtime_user(
            WorkspaceId::new(),
            SessionId::new(),
            CorrelationId::new(),
        ),
    )
}

#[test]
fn model_authentication_headers_are_rejected_by_the_gateway_scanner_before_approval() {
    for name in [
        "Authorization",
        "Proxy-Authorization",
        "Cookie",
        "Set-Cookie",
        "Cookie2",
        "X-API-Key",
        "Api-Key",
        "X-Auth-Token",
        "X-Access-Token",
    ] {
        let result = build(
            json!({"url":"http://localhost/", "headers":[{"name":name,"value":"synthetic-auth"}]}),
        );
        assert!(result.is_err(), "model authentication header {name} must be refused in planning");
        let error = result.expect_err("authentication cannot be offered for approval");
        assert!(
            matches!(
                error.downcast_ref::<NetworkControlError>(),
                Some(NetworkControlError::SecretDetected { .. })
            ),
            "{name} must retain the scanner's typed rejection"
        );
        assert!(
            !error.to_string().contains("synthetic-auth"),
            "candidate values stay out of diagnostics"
        );
    }
}

#[test]
fn model_secret_patterns_in_headers_and_bodies_are_rejected_during_planning() {
    let canary = "sk-s07-authentication-canary-0123456789";
    for input in [
        json!({"url":"http://localhost/", "headers":[{"name":"X-Trace","value":canary}]}),
        json!({"url":"http://localhost/", "method":"POST", "body":{"kind":"text","text":canary}}),
        json!({"url":"http://localhost/", "method":"POST", "body":{"kind":"json","value":{"copied":canary}}}),
    ] {
        let error = build(input).expect_err("the same wire bytes must be scanned before asking");
        assert!(matches!(
            error.downcast_ref::<NetworkControlError>(),
            Some(NetworkControlError::SecretDetected { .. })
        ));
        assert!(!error.to_string().contains(canary));
    }
}

#[test]
fn configured_slot_names_and_ordinary_request_data_remain_valid() {
    let request = build(json!({"url":"http://localhost/", "credential":"api_key", "headers":[{"name":"X-Trace","value":"ordinary"}]}))
        .expect("a slot selection is metadata, not model-supplied authentication");
    assert_eq!(request.headers, vec![("x-trace".into(), "ordinary".into())]);
}
