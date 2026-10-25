//! OAuth sign-in against the repository's fake authorization server. A trusted server
//! that wants a sign-in waits for it; signing in reads the server's challenge and metadata,
//! registers Aura, and finishes when the "browser" (a tiny client here) comes back; the token
//! is added by the gateway, refreshed when it ends or is refused, and a refused refresh asks
//! for a new sign-in. Tokens appear in no view, configuration or log, and are stored encrypted.
use super::http_server::HttpServer;
use super::support::Fixture;
use crate::mcp::connection::RequestFailure;
use aura_domain_common::SessionId;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

const LONG: Duration = Duration::from_secs(10);

/// GETs `url` like a browser that follows redirects; returns the last status line and body.
fn browse(url: &str) -> String {
    let mut target = url.to_owned();
    for _ in 0..4 {
        let parsed = url::Url::parse(&target).expect("an address");
        let host = format!("{}:{}", parsed.host_str().expect("host"), parsed.port().expect("port"));
        let path = &parsed[url::Position::BeforePath..];
        let mut stream = std::net::TcpStream::connect(&host).expect("connect");
        write!(stream, "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")
            .expect("send");
        let mut answer = String::new();
        stream.read_to_string(&mut answer).expect("read");
        let location = answer
            .lines()
            .find_map(|line| line.strip_prefix("Location: ").or(line.strip_prefix("location: ")))
            .map(str::to_owned);
        match location {
            Some(next) if answer.starts_with("HTTP/1.1 3") => target = next.trim().to_owned(),
            _ => return answer,
        }
    }
    panic!("too many redirects");
}

fn oauth_server(f: &Fixture, server: &HttpServer, client_id: Option<&str>) {
    let mut config = f.http_server("web", &server.url, &[]);
    config["transport"]["auth"] = match client_id {
        Some(id) => json!({"type": "oauth", "client_id": id}),
        None => json!({"type": "oauth"}),
    };
    f.save(config);
}

fn settle(f: &Fixture, id: &str, wanted: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let view = f.view(id);
        if view["state"] == wanted || Instant::now() >= deadline {
            return view;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Trusts the saved OAuth server `web` and signs in through the "browser".
fn signed_in(f: &Fixture) {
    let trusted = f.trust("web");
    assert_eq!(trusted["state"], "sign_in_required", "{trusted}");
    let begun = f.dispatch("agent.mcp.oauth.sign_in", json!({"id": "web"})).expect("begun");
    assert_eq!(begun["authorization_server"], "127.0.0.1");
    assert_eq!(f.view("web")["sign_in"]["pending"], true);
    let url = begun["authorization_url"].as_str().expect("address");
    for part in ["code_challenge_method=S256", "resource=", "state=", "scope=mcp.read"] {
        assert!(url.contains(part), "{part} in {url}");
    }
    let page = browse(url);
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    let ready = settle(f, "web", "ready");
    assert_eq!(ready["state"], "ready", "{ready}");
    assert_eq!(ready["sign_in"]["signed_in"], true);
}

#[test]
fn a_server_signs_in_through_the_browser_and_calls_carry_its_token() {
    let f = Fixture::with_network(Vec::new());
    let server = HttpServer::start_with(f._root.path(), &["--oauth"]);
    oauth_server(&f, &server, None);
    signed_in(&f);
    for step in ["registered client-", "authorized", "token issued"] {
        assert_eq!(server.count(step), 1, "{step}: {:?}", server.marks());
    }
    let connection = f.runtime.connection(SessionId::new(), &f.config("web")).expect("connects");
    let echoed = connection
        .request(
            "tools/call",
            Some(json!({"name": "echo", "arguments": {"text": "签名"}})),
            LONG,
            None,
        )
        .expect("a signed-in call");
    assert_eq!(echoed["content"][0]["text"], "签名");
    // No token in any view, and the stored sign-in is encrypted.
    let shown = format!(
        "{}{}",
        f.dispatch("agent.mcp.list", json!({})).expect("list"),
        f.dispatch("agent.mcp.describe", json!({"id": "web"})).expect("detail")
    );
    assert!(!shown.contains("access-") && !shown.contains("refresh-"), "{shown}");
    let stored = {
        let journal = f.runtime_journal();
        aura_persistence_v2::mcp::oauth(
            journal.lock().expect("journal").database.connection(),
            "web",
        )
        .expect("read")
        .expect("a sign-in")
    };
    let raw = String::from_utf8_lossy(&stored.protected_tokens);
    assert!(!raw.contains("access-") && !raw.contains("refresh-"));
    // The agent finds its tools while it holds the store.
    assert!(f.listed_inside_store().iter().any(|server| server == "web"));
}

#[test]
fn a_refused_token_is_refreshed_once_and_a_refused_refresh_asks_to_sign_in_again() {
    let f = Fixture::with_network(Vec::new());
    let server = HttpServer::start_with(f._root.path(), &["--oauth"]);
    oauth_server(&f, &server, None);
    signed_in(&f);
    let connection = f.runtime.connection(SessionId::new(), &f.config("web")).expect("connects");
    let call = || {
        connection.request(
            "tools/call",
            Some(json!({"name": "echo", "arguments": {"text": "x"}})),
            LONG,
            None,
        )
    };
    // The server forgets the access token: the call is refused once, refreshed and sent again.
    browse_post(&server, "/auth/expire-access");
    call().expect("refreshed and answered");
    assert_eq!(server.count("token refreshed"), 1, "{:?}", server.marks());
    assert_eq!(server.count("call echo"), 1, "the refused request never ran");
    // Every token is gone: the refresh is refused and the sign-in ends.
    browse_post(&server, "/auth/revoke-all");
    let refused = call();
    assert!(
        matches!(&refused, Err(RequestFailure::NotSent(reason)) if reason.contains("重新登录")),
        "{refused:?}"
    );
    let view = f.view("web");
    assert_eq!(view["state"], "sign_in_required", "{view}");
    assert!(view["sign_in"]["last_error"].as_str().is_some_and(|e| e.contains("invalid_grant")));
}

#[test]
fn an_ending_token_is_refreshed_before_it_is_sent() {
    let f = Fixture::with_network(Vec::new());
    let server = HttpServer::start_with(f._root.path(), &["--oauth", "--token-lifetime", "30"]);
    oauth_server(&f, &server, None);
    signed_in(&f);
    let connection = f.runtime.connection(SessionId::new(), &f.config("web")).expect("connects");
    connection
        .request(
            "tools/call",
            Some(json!({"name": "echo", "arguments": {"text": "x"}})),
            LONG,
            None,
        )
        .expect("answered");
    // A 30-second token is within the minute Aura refreshes ahead of its end.
    assert!(server.count("token refreshed") >= 1, "{:?}", server.marks());
    assert_eq!(server.count("unauthorized"), 1, "only the first look at the server");
}

#[test]
fn sign_in_needs_trust_and_goes_with_it() {
    let f = Fixture::with_network(Vec::new());
    let server = HttpServer::start_with(f._root.path(), &["--oauth"]);
    oauth_server(&f, &server, None);
    let early = f.dispatch("agent.mcp.oauth.sign_in", json!({"id": "web"}));
    assert!(early.is_err_and(|e| e.to_string().contains("请先信任")));
    assert_eq!(server.marks().len(), 0, "nothing reached the server before trust");
    signed_in(&f);
    f.dispatch("agent.mcp.untrust", json!({"id": "web", "expected_revision": f.revision()}))
        .expect("untrusted");
    let journal = f.runtime_journal();
    let left = aura_persistence_v2::mcp::oauth(
        journal.lock().expect("journal").database.connection(),
        "web",
    )
    .expect("read");
    assert!(left.is_none(), "an untrusted server keeps no sign-in");
}

#[test]
fn a_changed_address_or_sign_in_way_ends_the_sign_in() {
    let f = Fixture::with_network(Vec::new());
    let server = HttpServer::start_with(f._root.path(), &["--oauth"]);
    oauth_server(&f, &server, None);
    signed_in(&f);
    let stored = || {
        let journal = f.runtime_journal();
        aura_persistence_v2::mcp::oauth(
            journal.lock().expect("journal").database.connection(),
            "web",
        )
        .expect("read")
        .is_some()
    };
    let save = |server: &Value| {
        f.dispatch(
            "agent.mcp.save",
            json!({"server": server, "replaces": "web", "expected_revision": f.revision()}),
        )
        .expect("saved")
    };
    // A new display name is no new server: the sign-in stays and is still used.
    let mut renamed = serde_json::to_value(f.config("web")).expect("config");
    renamed["display_name"] = json!("改名之后");
    save(&renamed);
    assert!(stored(), "a renamed server stays signed in");
    assert_eq!(f.view("web")["sign_in"]["signed_in"], true);
    // Another address: the sign-in was made for the old one.
    let mut moved = renamed;
    moved["transport"]["url"] = json!(format!("{}/v2", server.url));
    save(&moved);
    assert!(!stored(), "a sign-in belongs to the address it was made for");
}

#[test]
fn an_authorization_server_without_pkce_or_metadata_elsewhere_is_refused() {
    for (flag, words) in [("--no-pkce", "PKCE"), ("--metadata-elsewhere", "不在它自己的地址上")]
    {
        let f = Fixture::with_network(Vec::new());
        let server = HttpServer::start_with(f._root.path(), &["--oauth", flag]);
        oauth_server(&f, &server, None);
        f.trust("web");
        let refused = f.dispatch("agent.mcp.oauth.sign_in", json!({"id": "web"}));
        let reason = refused.map(|_| String::new()).unwrap_or_else(|e| format!("{e:#}"));
        assert!(reason.contains(words), "{flag}: {reason}");
        assert_eq!(server.count("registered"), 0, "{flag}: Aura never registered");
    }
}

/// Calls one of the fake authorization server's test controls.
fn browse_post(server: &HttpServer, path: &str) {
    let address = server.url.trim_start_matches("http://").trim_end_matches("/mcp").to_owned();
    let mut stream = std::net::TcpStream::connect(&address).expect("connect");
    write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: {address}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
    .expect("send");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).expect("read");
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
}
