use super::*;
use crate::package_program::tests::support::{Fixture, Kind};
use aura_tool_domain::{
    PackageConformanceReasonV1 as Reason, PackageEndpointBindingV1, PackageToolSettingsV1,
};
use serde_json::json;

fn tool(fixture: &Fixture) -> aura_tool_domain::CanonicalToolPackageToolV1 {
    fixture.staged.canonical_manifest().tools[0].clone()
}
fn case() -> serde_json::Value {
    json!({"payload": null, "resource": "fixture", "tags": ["a", "b"]})
}
fn identity() -> aura_security_contract::InvocationIdentity {
    aura_security_contract::InvocationIdentity {
        principal_id: aura_domain_common::PrincipalId::new(),
        actor: aura_security_contract::InvocationActor::SystemRuntime,
        workspace_id: None,
        session_id: None,
        turn_task_id: None,
        agent_run_id: None,
        parent_agent_run_id: None,
        correlation_id: aura_domain_common::CorrelationId::new(),
        permission_snapshot_id: None,
    }
}

#[test]
fn declared_request_is_built_exactly_and_the_fixture_answer_passes_the_real_evaluator() {
    let fixture = Fixture::new(Kind::Http, |_| {});
    let tool = tool(&fixture);
    let request = build(&tool, &case(), None, identity()).expect("declared mapping");
    assert_eq!(request.gateway.plan.method, "POST");
    assert_eq!(
        request.gateway.plan.requested_url,
        "https://example.invalid/v1/echo/fixture?tag=a&tag=b"
    );
    assert_eq!(request.gateway.body, br#"{"payload":null}"#);
    assert!(
        request.gateway.headers.iter().all(|(name, _)| name != "authorization"),
        "credential is placed only by the trusted resolver"
    );
    assert_eq!(request.gateway.plan.redirect_policy, aura_permission_domain::RedirectPolicy::Deny);
    let (reason, _) = run_case(&tool, &case(), None);
    assert_eq!(reason, Reason::Completed);
}

#[test]
fn an_argument_the_mapping_cannot_place_fails_before_any_transport() {
    let fixture = Fixture::new(Kind::Http, |_| {});
    let (reason, _) =
        run_case(&tool(&fixture), &json!({"payload": null, "resource": "a/b", "tags": []}), None);
    assert_eq!(reason, Reason::InvalidProtocol);
    let (reason, _) = run_case(&tool(&fixture), &json!({"resource": "x"}), None);
    assert_eq!(reason, Reason::InvalidProtocol, "arguments are validated against the schema");
}

#[test]
fn a_secret_typed_into_an_argument_is_refused_like_the_built_in_http_tool() {
    let fixture = Fixture::new(Kind::Http, |_| {});
    let secret = json!({"payload": {"token": "ghp_0123456789abcdefghijklmnopqrstuvwxyzAB"}, "resource": "fixture", "tags": []});
    assert!(build(&tool(&fixture), &secret, None, identity()).is_err());
}

#[test]
fn an_unsynthesizable_result_shape_is_reported_unchecked_never_passed() {
    let fixture = Fixture::new(Kind::Http, |raw| {
        raw["tools"][0]["model"]["result_contract"] = json!({"type":"structured_json",
            "result_schema":{"type":"object","required":["id"],"properties":{"id":{"type":"string","pattern":"^[0-9]{6}$"}}}});
    });
    let (reason, _) = run_case(&tool(&fixture), &case(), None);
    assert_eq!(reason, Reason::CompletedResultUnchecked);
}

#[test]
fn evaluator_refuses_an_undeclared_status_and_a_body_outside_the_contract() {
    let fixture = Fixture::new(Kind::Http, |raw| {
        raw["tools"][0]["model"]["result_contract"] = json!({"type":"structured_json",
            "result_schema":{"type":"object","required":["id"],"properties":{"id":{"type":"integer"}}}});
    });
    let tool = tool(&fixture);
    let ranges = [(200, 204)];
    assert_eq!(
        evaluate(&tool, &ranges, 500, br#"{"id":1}"#),
        PackageHttpEvaluation::UnexpectedStatus { status: 500 }
    );
    assert_eq!(
        evaluate(&tool, &ranges, 200, br#"{"id":"x"}"#),
        PackageHttpEvaluation::InvalidResult
    );
    assert_eq!(
        evaluate(&tool, &ranges, 200, br#"{"id":7}"#),
        PackageHttpEvaluation::Completed { value: json!({"id":7}) }
    );
}

#[test]
fn a_user_bound_endpoint_uses_only_the_address_the_user_supplied() {
    let fixture = Fixture::new(Kind::Http, |raw| {
        raw["tools"][0]["runtime"]["request"]["target"] =
            json!({"type":"user_bound_endpoint","endpoint_slot_key":"service"});
    });
    let tool = tool(&fixture);
    let (missing, _) = run_case(&tool, &case(), None);
    assert_eq!(missing, Reason::InvalidProtocol, "no address is substituted");
    let settings = PackageToolSettingsV1 {
        tool_key: tool.tool_key.clone(),
        configuration: None,
        endpoints: vec![PackageEndpointBindingV1 {
            endpoint_slot_key: aura_tool_contract::HttpEndpointSlotKeyV1::new("service")
                .expect("slot"),
            origin: aura_tool_contract::FixedHttpsOriginV1::new("https://api.example.test")
                .expect("origin"),
            base_path: aura_tool_contract::HttpBasePathV1::new("/base").expect("path"),
        }],
    };
    let request = build(&tool, &case(), Some(&settings), identity()).expect("bound endpoint");
    assert!(request.gateway.plan.requested_url.starts_with("https://api.example.test/base/echo/"));
    assert_eq!(run_case(&tool, &case(), Some(&settings)).0, Reason::Completed);
}
