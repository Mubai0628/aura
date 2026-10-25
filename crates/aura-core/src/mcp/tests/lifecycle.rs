//! Connections: one per session and server, requests answered by id, cancellation and
//! timeouts settled, and a server that fails affecting only itself.
use super::support::Fixture;
use crate::mcp::connection::RequestFailure;
use aura_domain_common::SessionId;
use serde_json::json;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

fn call(name: &str, arguments: serde_json::Value) -> Option<serde_json::Value> {
    Some(json!({"name": name, "arguments": arguments}))
}

#[test]
fn each_session_has_its_own_instance_and_calls_are_answered_by_id() {
    let f = Fixture::new();
    f.save(f.server("files", &[]));
    f.trust("files");
    let server = f.config("files");
    let (one, two) = (SessionId::new(), SessionId::new());
    let first = f.runtime.connection(one, &server).expect("first session");
    let again = f.runtime.connection(one, &server).expect("reused");
    let other = f.runtime.connection(two, &server).expect("second session");
    assert!(std::sync::Arc::ptr_eq(&first, &again));
    assert!(!std::sync::Arc::ptr_eq(&first, &other));
    assert_eq!(f.runtime.running_instances("files"), 2);
    let slow = std::thread::spawn({
        let first = first.clone();
        move || {
            first.request(
                "tools/call",
                call("slow", json!({"ms": 600})),
                Duration::from_secs(10),
                None,
            )
        }
    });
    let echoed = first
        .request("tools/call", call("echo", json!({"text": "并发"})), Duration::from_secs(10), None)
        .expect("answered while another call waits");
    assert_eq!(echoed["content"][0]["text"], "并发");
    assert_eq!(
        slow.join().expect("thread").expect("slow answer")["content"][0]["text"],
        "slept 600"
    );
    let error =
        first.request("tools/call", call("missing", json!({})), Duration::from_secs(10), None);
    assert!(matches!(error, Err(RequestFailure::Rpc(ref e)) if e.code == -32602), "{error:?}");
}

#[test]
fn cancellation_and_timeouts_are_settled_and_the_server_keeps_running() {
    let f = Fixture::new();
    f.save(f.server("files", &[]));
    f.trust("files");
    let connection = f.runtime.connection(SessionId::new(), &f.config("files")).expect("runs");
    let cancel = AtomicBool::new(false);
    let began = Instant::now();
    let cancelled = std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(200));
            cancel.store(true, std::sync::atomic::Ordering::Release);
        });
        connection.request(
            "tools/call",
            call("slow", json!({"ms": 20_000})),
            Duration::from_secs(30),
            Some(&cancel),
        )
    });
    assert_eq!(cancelled, Err(RequestFailure::Cancelled));
    assert!(began.elapsed() < Duration::from_secs(5), "the server stopped waiting on cancel");
    let timed_out = connection.request(
        "tools/call",
        call("slow", json!({"ms": 20_000})),
        Duration::from_millis(300),
        None,
    );
    assert_eq!(timed_out, Err(RequestFailure::TimedOut));
    assert!(connection.alive(), "a cancelled or slow call does not end the server");
    let echoed = connection
        .request("tools/call", call("echo", json!({"text": "ok"})), Duration::from_secs(10), None)
        .expect("still answers");
    assert_eq!(echoed["content"][0]["text"], "ok");
}

#[test]
fn a_crash_is_lost_not_failed_and_restarts_are_bounded() {
    let f = Fixture::new();
    f.save(f.server("files", &[]));
    f.save(f.server("other", &[]));
    f.trust("files");
    f.trust("other");
    let session = SessionId::new();
    let other = f.runtime.connection(session, &f.config("other")).expect("other runs");
    for attempt in 0..3 {
        let connection = f.runtime.connection(session, &f.config("files")).expect("restarted");
        let crashed = connection.request(
            "tools/call",
            call("crash", json!({})),
            Duration::from_secs(10),
            None,
        );
        assert!(matches!(crashed, Err(RequestFailure::Lost(_))), "attempt {attempt}: {crashed:?}");
        let deadline = Instant::now() + Duration::from_secs(5);
        while connection.alive() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!connection.alive());
    }
    let paused = f.runtime.connection(session, &f.config("files")).map(|_| ()).expect_err("paused");
    assert!(paused.contains("已暂停"), "{paused}");
    assert!(other.alive(), "another server keeps running");
    let echoed = other
        .request(
            "tools/call",
            call("echo", json!({"text": "仍可用"})),
            Duration::from_secs(10),
            None,
        )
        .expect("other answers");
    assert_eq!(echoed["content"][0]["text"], "仍可用");
}

#[test]
fn a_server_that_ends_at_start_fails_with_its_error_output() {
    let f = Fixture::new();
    f.save(f.server("broken", &["--exit-at-start"]));
    let failed = f.trust("broken");
    assert_eq!(failed["state"], "failed", "{failed}");
    assert!(failed["last_error"].as_str().is_some_and(|e| e.contains("握手失败")), "{failed}");
    let detail = f.dispatch("agent.mcp.describe", json!({"id": "broken"})).expect("detail");
    assert!(
        detail["stderr"].as_array().is_some_and(|lines| lines
            .iter()
            .any(|l| l.as_str().is_some_and(|l| l.contains("exiting at start")))),
        "{detail}"
    );
    f.save(f.server("old", &["--version", "2024-11-05"]));
    let old = f.trust("old");
    assert_eq!(old["state"], "failed", "an unsupported protocol version is refused: {old}");
}

#[test]
fn a_tools_changed_notice_lists_the_tools_again_for_the_registry() {
    let f = Fixture::new();
    f.save(f.server("files", &[]));
    assert_eq!(f.trust("files")["tool_count"], 5);
    let session = SessionId::new();
    let connection = f.runtime.connection(session, &f.config("files")).expect("runs");
    connection
        .request("tools/call", call("grow", json!({})), Duration::from_secs(10), None)
        .expect("grown");
    // The next use of the instance notices the change and lists the tools again.
    f.runtime.connection(session, &f.config("files")).expect("reused");
    let deadline = Instant::now() + Duration::from_secs(10);
    while f.view("files")["tool_count"] != 6 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(f.view("files")["tool_count"], 6);
    let detail = f.dispatch("agent.mcp.describe", json!({"id": "files"})).expect("detail");
    assert!(
        detail["tools"]
            .as_array()
            .is_some_and(|tools| tools.iter().any(|t| t["canonical_name"] == "mcp.files.extra"))
    );
}

/// Like Git's, an MCP program may have other hard links (cargo puts a second
/// link to each `target\debug\*.exe` in `deps`). It is trusted and started as usual, nothing can
/// write it through the other link while it runs, and a change made through that link while it
/// is stopped needs trust again.
#[test]
fn a_program_with_another_hard_link_runs_and_a_change_through_that_link_needs_trust_again() {
    let f = Fixture::new();
    let other = f.server_exe.with_file_name("other-link.exe");
    std::fs::hard_link(&f.server_exe, &other).expect("a second hard link");
    f.save(f.server("files", &[]));
    assert_eq!(f.view("files")["problems"], json!([]));
    f.trust("files");
    let connection = f.runtime.connection(SessionId::new(), &f.config("files")).expect("started");
    connection
        .request("tools/call", call("echo", json!({"text": "链接"})), Duration::from_secs(10), None)
        .expect("answered");
    let write = std::fs::OpenOptions::new().write(true).open(&other);
    assert!(write.is_err(), "the running program is not writable through its other link");
    f.runtime.stop_server("files");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut file = loop {
        match std::fs::OpenOptions::new().append(true).open(&other) {
            Ok(file) => break file,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => panic!("the stopped program is released: {error}"),
        }
    };
    std::io::Write::write_all(&mut file, b"changed").expect("changed through the other link");
    drop(file);
    assert_eq!(f.view("files")["state"], "config_changed");
    let Err(refused) = f.runtime.connection(SessionId::new(), &f.config("files")) else {
        panic!("a program changed through its other link is not started");
    };
    assert!(refused.contains("重新信任"), "{refused}");
}
