//! Streamable HTTP servers through Core's network gateway. Trusted, listed and called
//! like a program; a forgotten session is greeted again and the request sent once more; a
//! broken answer is unknown and never resent; a redirect is never followed; a domain name
//! never reaches this computer.
use super::http_server::HttpServer;
use super::support::Fixture;
use crate::mcp::connection::RequestFailure;
use aura_domain_common::SessionId;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

const LONG: Duration = Duration::from_secs(10);

fn call(name: &str, arguments: Value) -> Option<Value> {
    Some(json!({"name": name, "arguments": arguments}))
}

fn text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap_or_default().to_owned()
}

/// A trusted HTTP server `web` at the fixture's address, with `headers`.
fn trusted(headers: &[(&str, &str)]) -> (Fixture, HttpServer) {
    let f = Fixture::with_network(Vec::new());
    let server = HttpServer::start(f._root.path());
    f.save(f.http_server("web", &server.url, headers));
    assert_eq!(f.view("web")["state"], "not_trusted");
    let ready = f.trust("web");
    assert_eq!(ready["state"], "ready", "{ready}");
    (f, server)
}

#[test]
fn calls_that_arrive_together_share_one_session() {
    let (f, server) = trusted(&[]);
    let before = server.count("initialize");
    let session = SessionId::new();
    let config = f.config("web");
    let start = Arc::new(std::sync::Barrier::new(4));
    let calls: Vec<_> = (0..4)
        .map(|n| {
            let (runtime, config, start) =
                (Arc::clone(&f.runtime), config.clone(), Arc::clone(&start));
            std::thread::spawn(move || {
                start.wait();
                let connection = runtime.connection(session, &config)?;
                connection
                    .request(
                        "tools/call",
                        call("echo", json!({"text": format!("同时 {n}")})),
                        LONG,
                        None,
                    )
                    .map(|answer| text(&answer))
                    .map_err(|failure| format!("{failure:?}"))
            })
        })
        .collect();
    for (n, call) in calls.into_iter().enumerate() {
        let answered = call.join().expect("a call").expect("every call is answered");
        assert_eq!(answered, format!("同时 {n}"));
    }
    assert_eq!(server.count("initialize") - before, 1, "one session for calls that came together");
}

#[test]
fn an_http_server_is_trusted_listed_and_called_through_the_gateway() {
    let (f, server) = trusted(&[("X-Team", "docs")]);
    assert_eq!(f.view("web")["tool_count"], 9);
    assert!(server.saw("deleted s-1"), "discovery ends its session: {:?}", server.marks());

    let connection = f.runtime.connection(SessionId::new(), &f.config("web")).expect("connects");
    let request = |name: &str, arguments: Value| {
        connection.request("tools/call", call(name, arguments), LONG, None)
    };
    assert_eq!(text(&request("echo", json!({"text": "你好"})).expect("echo")), "你好");
    assert_eq!(
        text(&request("header", json!({})).expect("header")),
        "docs",
        "the configured header goes with every request"
    );
    // An event stream: the server's ping is answered while the call waits; its log is kept.
    let streamed = request("stream", json!({"text": "hi"})).expect("stream");
    assert_eq!(text(&streamed), "streamed hi (ping answered: true)");
    assert_eq!(server.count("pong"), 1);
    assert!(connection.stderr_tail().iter().any(|line| line.contains("fixture log line")));
    assert_eq!(request("fail", json!({})).expect("answered")["isError"], true);
    assert_eq!(text(&request("grow", json!({})).expect("grown")), "grown");
    assert!(connection.take_tools_changed(), "the notice inside the stream is seen");

    // Switching the server off ends its session on the server too.
    f.runtime.stop_server("web");
    assert!(server.saw("deleted s-2"), "{:?}", server.marks());
}

#[test]
fn a_forgotten_session_is_greeted_again_and_the_request_sent_once_more() {
    let (f, server) = trusted(&[]);
    let connection = f.runtime.connection(SessionId::new(), &f.config("web")).expect("connects");
    let greeted = server.count("initialize");
    connection.request("tools/call", call("expire", json!({})), LONG, None).expect("expired");
    let echoed = connection
        .request("tools/call", call("echo", json!({"text": "again"})), LONG, None)
        .expect("sent again in a new session");
    assert_eq!(text(&echoed), "again");
    assert_eq!(server.count("initialize"), greeted + 1, "one new handshake");
    assert_eq!(server.count("call echo"), 1, "the refused request never ran; it ran once after");
    assert!(connection.take_tools_changed(), "a new session may come with other tools");
}

#[test]
fn a_stopped_or_late_call_settles_and_the_server_is_told() {
    let (f, server) = trusted(&[]);
    let connection = f.runtime.connection(SessionId::new(), &f.config("web")).expect("connects");
    let stop = Arc::new(AtomicBool::new(false));
    let stopper = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            stop.store(true, Ordering::Release);
        })
    };
    let slow = || call("slow", json!({"ms": 8_000}));
    let stopped = connection.request("tools/call", slow(), LONG, Some(&stop));
    stopper.join().expect("stopper");
    assert_eq!(stopped, Err(RequestFailure::Cancelled));
    assert_eq!(server.count("cancelled"), 1, "{:?}", server.marks());
    let late = connection.request("tools/call", slow(), Duration::from_millis(500), None);
    assert_eq!(late, Err(RequestFailure::TimedOut));
    assert_eq!(server.count("cancelled"), 2);
    let quick = connection
        .request("tools/call", call("slow", json!({"ms": 50})), LONG, None)
        .expect("answered in time");
    assert_eq!(text(&quick), "slept 50");
}

#[test]
fn a_broken_answer_is_unknown_and_never_resent() {
    let (f, server) = trusted(&[]);
    let connection = f.runtime.connection(SessionId::new(), &f.config("web")).expect("connects");
    let dropped = connection.request("tools/call", call("drop", json!({})), LONG, None);
    assert!(matches!(dropped, Err(RequestFailure::Lost(_))), "{dropped:?}");
    let failed = connection.request("tools/call", call("error500", json!({})), LONG, None);
    assert!(matches!(failed, Err(RequestFailure::Lost(_))), "{failed:?}");
    assert_eq!(server.count("call drop"), 1);
    assert_eq!(server.count("call error500"), 1);
    assert!(connection.alive(), "the session itself still works");
    assert_eq!(server.count("initialize"), 2, "discovery and this session; nothing resent");
}

#[test]
fn a_redirect_is_never_followed() {
    let f = Fixture::with_network(Vec::new());
    let server = HttpServer::start(f._root.path());
    f.save(f.http_server("moved", &server.at("/moved"), &[]));
    let failed = f.trust("moved");
    assert_eq!(failed["state"], "failed", "{failed}");
    assert!(failed["last_error"].as_str().is_some_and(|e| e.contains("跳转")), "{failed}");
    assert_eq!(server.count("initialize"), 0, "the redirect target was never asked");
}

#[test]
fn a_domain_name_never_reaches_this_computer() {
    let loopback: std::net::IpAddr = "127.0.0.1".parse().expect("loopback");
    let record = |host: &str| aura_network_control::TestFixtureDnsRecord {
        host: host.into(),
        classified_addresses: vec![loopback],
        connect_addresses: vec![loopback],
    };
    let f = Fixture::with_network(vec![record("127.0.0.1"), record("rebind.test")]);
    let server = HttpServer::start(f._root.path());
    let port = url::Url::parse(&server.url).expect("address").port().expect("port");
    f.save(f.http_server("rebind", &format!("https://rebind.test:{port}/mcp"), &[]));
    let refused = f.trust("rebind");
    assert_eq!(refused["state"], "failed", "{refused}");
    assert!(
        refused["last_error"].as_str().is_some_and(|e| e.contains("不允许的位置")),
        "{refused}"
    );
    assert_eq!(server.count("initialize"), 0);
    // The same server by its own address is fine.
    f.save(f.http_server("direct", &server.url, &[]));
    assert_eq!(f.trust("direct")["state"], "ready");
    assert_eq!(server.count("initialize"), 1);
}

#[test]
fn a_request_carrying_a_secret_never_leaves_this_computer() {
    let (f, server) = trusted(&[]);
    let connection = f.runtime.connection(SessionId::new(), &f.config("web")).expect("connects");
    let secret = json!({"text": "sk-abcdefghijklmnopqrstuvwxyz012345"});
    let refused = connection.request("tools/call", call("echo", secret), LONG, None);
    assert!(
        matches!(&refused, Err(RequestFailure::NotSent(reason)) if reason.contains("密钥")),
        "{refused:?}"
    );
    assert_eq!(server.count("call"), 0);
}

#[test]
fn a_cloud_metadata_address_is_never_a_server() {
    let f = Fixture::with_network(Vec::new());
    f.save(f.http_server("meta", "https://169.254.169.254/mcp", &[]));
    let view = f.view("meta");
    assert_eq!(view["state"], "invalid", "{view}");
    assert!(view["problems"].to_string().contains("元数据"), "{view}");
}

#[test]
fn a_server_that_is_not_listening_is_not_started_rather_than_unknown() {
    let (f, mut server) = trusted(&[]);
    let connection = f.runtime.connection(SessionId::new(), &f.config("web")).expect("connects");
    server.stop();
    let refused = connection.request("tools/call", call("echo", json!({"text": "x"})), LONG, None);
    assert!(matches!(refused, Err(RequestFailure::NotSent(_))), "{refused:?}");
}
