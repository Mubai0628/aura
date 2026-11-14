//! A grant for a trusted MCP server reaches exactly its endpoint, by POST or DELETE,
//! with the MCP headers or the server's configured ones, never a redirect, and a local
//! address only when the trusted address names one.
use aura_domain_common::{CorrelationId, NetworkRequestId, SessionId, UtcTimestamp, WorkspaceId};
use aura_network_control::{GatewayRequest, McpHttpRequestGrant};
use aura_permission_domain::{NetworkByteLimits, NetworkRequestPlan, RedirectPolicy};
use aura_security_contract::{InvocationIdentity, NetworkCallerKind, NetworkDestinationClass};
use url::Url;

const ENDPOINT: &str = "https://mcp.example.com/mcp";

fn request(url: &str, method: &str, headers: &[(&str, &str)]) -> GatewayRequest {
    GatewayRequest {
        plan: NetworkRequestPlan {
            request_id: NetworkRequestId::new(),
            identity: InvocationIdentity::runtime_user(
                WorkspaceId::new(),
                SessionId::new(),
                CorrelationId::new(),
            ),
            caller: NetworkCallerKind::McpHttp,
            method: method.into(),
            requested_url: url.into(),
            destination: None,
            request_data: vec![],
            data_classes: vec![],
            redirect_policy: RedirectPolicy::Deny,
            byte_limits: NetworkByteLimits {
                max_upload_bytes: 1024,
                max_download_bytes: 1024,
                max_decompressed_bytes: 1024,
            },
            timeout_ms: 1_000,
            credential_refs: vec![],
            network_grant_ids: vec![],
        },
        headers: headers.iter().map(|(n, v)| ((*n).to_owned(), (*v).to_owned())).collect(),
        body: br#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#.to_vec(),
    }
}

fn soon() -> UtcTimestamp {
    UtcTimestamp::from_value(UtcTimestamp::now().value() + time::Duration::seconds(30))
}

fn issue(endpoint: &str, request: &GatewayRequest) -> bool {
    McpHttpRequestGrant::issue(endpoint, &["x-team".to_owned()], None, request, soon()).is_ok()
}

#[test]
fn an_address_reaches_only_the_class_it_names() {
    let classes =
        |url: &str| McpHttpRequestGrant::destination_classes(&Url::parse(url).expect("url"));
    assert_eq!(
        classes("https://mcp.example.com/mcp").expect("domain"),
        [NetworkDestinationClass::Public]
    );
    assert_eq!(
        classes("http://localhost:7000/mcp").expect("localhost"),
        [NetworkDestinationClass::Loopback]
    );
    assert_eq!(
        classes("http://127.0.0.1:7000/mcp").expect("loopback"),
        [NetworkDestinationClass::Loopback]
    );
    assert_eq!(
        classes("http://192.168.1.20/mcp").expect("lan"),
        [NetworkDestinationClass::Private]
    );
}

#[test]
fn only_the_trusted_endpoint_by_post_or_delete_with_known_headers() {
    let managed =
        [("accept", "application/json, text/event-stream"), ("content-type", "application/json")];
    assert!(issue(ENDPOINT, &request(ENDPOINT, "POST", &managed)));
    assert!(issue(ENDPOINT, &request(ENDPOINT, "DELETE", &[("mcp-session-id", "abc")])));
    assert!(
        issue(ENDPOINT, &request(ENDPOINT, "POST", &[("X-Team", "docs")])),
        "a configured header"
    );

    // Another path, host or method, an unknown header or a redirect policy is refused.
    assert!(!issue(ENDPOINT, &request("https://mcp.example.com/other", "POST", &managed)));
    assert!(!issue(ENDPOINT, &request("https://evil.example.com/mcp", "POST", &managed)));
    assert!(!issue(ENDPOINT, &request(ENDPOINT, "GET", &[])));
    assert!(!issue(ENDPOINT, &request(ENDPOINT, "POST", &[("cookie", "a=b")])));
    let mut follows = request(ENDPOINT, "POST", &managed);
    follows.plan.redirect_policy = RedirectPolicy::SameOrigin { max_hops: 1 };
    assert!(!issue(ENDPOINT, &follows));
    let mut tool = request(ENDPOINT, "POST", &managed);
    tool.plan.caller = NetworkCallerKind::AuraTool;
    assert!(!issue(ENDPOINT, &tool), "only the MCP caller");

    // http is for this computer only; the grant is pinned to the address's class.
    assert!(!issue(
        "http://mcp.example.com/mcp",
        &request("http://mcp.example.com/mcp", "POST", &managed)
    ));
    let local = "http://127.0.0.1:7000/mcp";
    let grant =
        McpHttpRequestGrant::issue(local, &[], None, &request(local, "POST", &managed), soon())
            .expect("a local server");
    assert_eq!(
        grant.destination_policy.allowed_destination_classes,
        [NetworkDestinationClass::Loopback]
    );
    assert_eq!(grant.redirect_policy, RedirectPolicy::Deny);
    assert_eq!(grant.methods, ["POST"]);
}
