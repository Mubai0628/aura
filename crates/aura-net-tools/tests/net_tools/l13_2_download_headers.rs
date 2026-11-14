//! A download may send User-Agent, Referer, Accept and
//! Accept-Language, for a site that serves a file only to its own pages; no other header, and never
//! a cookie or a credential.
use aura_domain_common::{CorrelationId, SessionId, WorkspaceId};
use aura_net_tools::download_request;
use aura_security_contract::InvocationIdentity;
use serde_json::json;

fn build(input: serde_json::Value) -> anyhow::Result<aura_network_control::GatewayRequest> {
    download_request(
        &serde_json::from_value(input)?,
        InvocationIdentity::runtime_user(
            WorkspaceId::new(),
            SessionId::new(),
            CorrelationId::new(),
        ),
    )
}

#[test]
fn a_download_sends_the_page_headers_it_names_and_no_other() {
    let request = build(json!({
        "url": "https://img.example.com/a.jpg",
        "path": "media/a.jpg",
        "headers": [
            {"name": "Referer", "value": "https://image.example.com/search"},
            {"name": "user-agent", "value": "Mozilla/5.0"},
            {"name": "Accept", "value": "image/avif,image/webp,*/*"},
            {"name": "Accept-Language", "value": "zh-CN,zh;q=0.9"}
        ]
    }))
    .expect("the page headers");
    assert_eq!(request.plan.method, "GET");
    assert_eq!(
        request.headers,
        [
            ("referer".to_owned(), "https://image.example.com/search".to_owned()),
            ("user-agent".to_owned(), "Mozilla/5.0".to_owned()),
            ("accept".to_owned(), "image/avif,image/webp,*/*".to_owned()),
            ("accept-language".to_owned(), "zh-CN,zh;q=0.9".to_owned()),
        ]
    );
    let plain = build(json!({"url": "https://img.example.com/a.jpg", "path": "a.jpg"}))
        .expect("no headers");
    assert!(plain.headers.is_empty(), "a download that names none sends none, as before");
    for (name, value) in [
        ("Cookie", "session=1"),
        ("Authorization", "Bearer token"),
        ("X-Requested-With", "XMLHttpRequest"),
        ("Referer", "https://a.example.com/\r\nCookie: a=b"),
    ] {
        assert!(
            build(json!({"url": "https://img.example.com/a.jpg", "path": "a.jpg",
                "headers": [{"name": name, "value": value}]}))
            .is_err(),
            "{name}"
        );
    }
    let twice = json!({"url": "https://img.example.com/a.jpg", "path": "a.jpg", "headers": [
        {"name": "Referer", "value": "https://a.example.com/"},
        {"name": "referer", "value": "https://b.example.com/"}
    ]});
    assert!(build(twice).is_err(), "each header once");
}
