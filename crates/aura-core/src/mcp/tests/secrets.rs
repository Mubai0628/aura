//! A server's secret. It is written only through the secret service (the route takes
//! it on the single-use secret input), the configuration names it by ID, the gateway adds it
//! to HTTP requests and refuses an answer that carries it back, a program gets it as an
//! environment variable, and it goes with the server. These tests keep values in memory.
use super::http_server::HttpServer;
use super::support::Fixture;
use crate::mcp::connection::RequestFailure;
use aura_domain_common::{CredentialId, SessionId};
use aura_windows_platform::SecretBytes;
use serde_json::{Value, json};
use std::time::Duration;

const SECRET: &str = "s3cret-for-the-fixture-server-0001";
const LONG: Duration = Duration::from_secs(10);

fn set(f: &Fixture, server: &str, id: Option<CredentialId>, value: &str) -> CredentialId {
    f.runtime
        .secrets()
        .set(server, id, &SecretBytes::new(value.as_bytes().to_vec()))
        .expect("the secret is written")
}

fn with_auth(mut server: Value, auth: Value) -> Value {
    server["transport"]["auth"] = auth;
    server
}

fn call(name: &str) -> Option<Value> {
    Some(json!({"name": name, "arguments": {"text": "hi"}}))
}

#[test]
fn a_bearer_secret_is_sent_by_the_gateway_and_never_shown() {
    let f = Fixture::with_network(Vec::new());
    let required = format!("authorization:Bearer {SECRET}");
    let server = HttpServer::start_with(f._root.path(), &["--require-auth", &required]);
    let id = set(&f, "web", None, SECRET);
    let config = f.http_server("web", &server.url, &[]);
    f.save(with_auth(config, json!({"type": "secret", "secret_id": id})));
    let ready = f.trust("web");
    assert_eq!(ready["state"], "ready", "{ready}");
    assert_eq!(ready["tool_count"], 10, "the protected server lists `reflect` too");
    let connection = f.runtime.connection(SessionId::new(), &f.config("web")).expect("connects");
    let echoed = connection.request("tools/call", call("echo"), LONG, None).expect("signed in");
    assert_eq!(echoed["content"][0]["text"], "hi");
    assert_eq!(server.count("unauthorized"), 0);
    // The value is in no view, configuration or log Aura shows.
    let shown = [
        f.dispatch("agent.mcp.list", json!({})).expect("list").to_string(),
        f.dispatch("agent.mcp.describe", json!({"id": "web"})).expect("detail").to_string(),
        connection.stderr_tail().join("\n"),
    ];
    assert!(shown.iter().all(|text| !text.contains(SECRET)));
    // The agent finds its tools while it holds the store.
    assert!(f.listed_inside_store().iter().any(|server| server == "web"));
}

#[test]
fn a_new_value_takes_effect_at_once_without_a_new_trust() {
    let f = Fixture::with_network(Vec::new());
    let required = format!("x-api-key:{SECRET}");
    let server = HttpServer::start_with(f._root.path(), &["--require-auth", &required]);
    let id = set(&f, "web", None, "an-old-value-the-server-refuses");
    let config = f.http_server("web", &server.url, &[]);
    f.save(with_auth(config, json!({"type": "secret", "secret_id": id, "header": "X-Api-Key"})));
    let refused = f.trust("web");
    assert_eq!(refused["state"], "failed", "{refused}");
    assert!(refused["last_error"].as_str().is_some_and(|e| e.contains("401")), "{refused}");
    let trusted = refused["config"]["trusted_fingerprint"].clone();
    assert_eq!(set(&f, "web", Some(id), SECRET), id, "the same secret, its next value");
    f.dispatch("agent.mcp.refresh", json!({"id": "web"})).expect("tested again");
    let ready = f.view("web");
    assert_eq!(ready["state"], "ready", "{ready}");
    assert_eq!(ready["config"]["trusted_fingerprint"], trusted, "no new trust was needed");
}

#[test]
fn an_answer_that_carries_the_secret_back_is_unknown_and_dropped() {
    let f = Fixture::with_network(Vec::new());
    let required = format!("authorization:Bearer {SECRET}");
    let server = HttpServer::start_with(f._root.path(), &["--require-auth", &required]);
    let id = set(&f, "web", None, SECRET);
    let config = f.http_server("web", &server.url, &[]);
    f.save(with_auth(config, json!({"type": "secret", "secret_id": id})));
    f.trust("web");
    let connection = f.runtime.connection(SessionId::new(), &f.config("web")).expect("connects");
    let leaked = connection.request("tools/call", call("reflect"), LONG, None);
    match leaked {
        Err(RequestFailure::Lost(reason)) => {
            assert!(reason.contains("登录凭据") && !reason.contains(SECRET), "{reason}");
        }
        other => panic!("the answer must be dropped as unknown: {other:?}"),
    }
    assert_eq!(server.count("call reflect"), 1, "it reached the server once");
}

#[test]
fn a_configuration_names_only_its_own_secret_and_a_lost_value_is_not_started() {
    let f = Fixture::with_network(Vec::new());
    let others = set(&f, "other", None, SECRET);
    let config = f.http_server("web", "https://mcp.example.com/mcp", &[]);
    let borrowed = with_auth(config, json!({"type": "secret", "secret_id": others}));
    let refused = f.dispatch(
        "agent.mcp.save",
        json!({"server": borrowed, "replaces": null, "expected_revision": f.revision()}),
    );
    assert!(refused.is_err_and(|e| e.to_string().contains("不属于这个服务器")));

    let required = format!("authorization:Bearer {SECRET}");
    let server = HttpServer::start_with(f._root.path(), &["--require-auth", &required]);
    let id = set(&f, "web", None, SECRET);
    let config = f.http_server("web", &server.url, &[]);
    f.save(with_auth(config, json!({"type": "secret", "secret_id": id})));
    assert_eq!(f.trust("web")["state"], "ready");
    let connection = f.runtime.connection(SessionId::new(), &f.config("web")).expect("connects");
    // The Credential Manager entry disappears behind Aura's back: nothing is sent.
    f.secrets.forget_value(id);
    let lost = connection.request("tools/call", call("echo"), LONG, None);
    assert!(
        matches!(&lost, Err(RequestFailure::NotSent(reason)) if reason.contains("凭据管理器")),
        "{lost:?}"
    );
    assert_eq!(server.count("call echo"), 0);
}

#[test]
fn a_secret_goes_with_its_server_or_its_sign_in() {
    let f = Fixture::with_network(Vec::new());
    let config = f.http_server("web", "https://mcp.example.com/mcp", &[]);
    let id = set(&f, "web", None, SECRET);
    f.save(with_auth(config.clone(), json!({"type": "secret", "secret_id": id})));
    assert_eq!(f.secrets.len(), 1);
    // Saved without sign-in: the secret goes.
    f.dispatch(
        "agent.mcp.save",
        json!({"server": config.clone(), "replaces": "web", "expected_revision": f.revision()}),
    )
    .expect("saved");
    assert_eq!(f.secrets.len(), 0);
    assert!(!f.runtime.secrets().belongs_to("web", id));
    // Removed: its secret goes too.
    let again = set(&f, "web", None, SECRET);
    f.dispatch(
        "agent.mcp.save",
        json!({
            "server": with_auth(config, json!({"type": "secret", "secret_id": again})),
            "replaces": "web",
            "expected_revision": f.revision()
        }),
    )
    .expect("saved");
    assert_eq!(f.secrets.len(), 1);
    f.dispatch("agent.mcp.remove", json!({"id": "web", "expected_revision": f.revision()}))
        .expect("removed");
    assert_eq!(f.secrets.len(), 0);
}

#[test]
fn unnamed_secrets_are_swept_but_never_while_mcp_json_is_unreadable() {
    let f = Fixture::with_network(Vec::new());
    f.save(f.http_server("kept", "https://mcp.example.com/mcp", &[]));
    let unsaved = set(&f, "web", None, SECRET);
    let file = f._root.path().join("config").join("mcp.json");
    let healthy = std::fs::read(&file).expect("the saved mcp.json");
    std::fs::write(&file, b"{ not json").expect("a damaged mcp.json");
    assert!(f.runtime.configured().issue.is_some());
    f.runtime.sweep_credentials();
    assert!(f.runtime.secrets().belongs_to("web", unsaved), "kept while the file is damaged");
    std::fs::write(&file, healthy).expect("repaired");
    assert!(f.runtime.configured().issue.is_none());
    f.runtime.sweep_credentials();
    assert!(!f.runtime.secrets().belongs_to("web", unsaved), "swept: no server names it");
    assert_eq!(f.secrets.len(), 0);
}

#[test]
fn a_program_server_gets_its_secret_as_an_environment_variable() {
    let f = Fixture::with_network(Vec::new());
    let id = set(&f, "files", None, SECRET);
    let mut config = f.server("files", &["--env-tool"]);
    config["transport"]["secret_environment"] = json!([{"name": "FIXTURE_TOKEN", "secret_id": id}]);
    f.save(config);
    assert_eq!(f.trust("files")["state"], "ready");
    let connection = f.runtime.connection(SessionId::new(), &f.config("files")).expect("runs");
    let env = |name: &str| {
        connection
            .request(
                "tools/call",
                Some(json!({"name": "env", "arguments": {"name": name}})),
                LONG,
                None,
            )
            .expect("answered")["content"][0]["text"]
            .clone()
    };
    assert_eq!(env("FIXTURE_TOKEN"), format!("FIXTURE_TOKEN: set, {} characters", SECRET.len()));
    assert!(std::env::var("FIXTURE_TOKEN").is_err(), "Aura's own environment is unchanged");
}

/// The real Credential Manager, in an isolated namespace that is removed at the end.
#[test]
#[ignore = "writes the Windows Credential Manager under Aura/test/<run>; run after telling the owner"]
fn the_credential_manager_keeps_the_value_with_its_server_and_revision() {
    use crate::mcp::credentials::{SecretStore, WindowsSecretStore};
    let run = format!("l08b5{:x}", std::time::UNIX_EPOCH.elapsed().expect("time").as_nanos());
    let store = WindowsSecretStore::isolated(&run);
    let id = CredentialId::new();
    let value = SecretBytes::new(SECRET.as_bytes().to_vec());
    let written = store.write(id, "web", 1, &value);
    let read = store.read(id);
    let deleted = store.delete(id);
    let after = store.read(id);
    written.expect("written");
    let stored = read.expect("read").expect("present");
    assert_eq!((stored.server_id.as_str(), stored.revision), ("web", 1));
    assert_eq!(stored.value.expose(), SECRET.as_bytes());
    deleted.expect("deleted");
    assert!(after.expect("read after delete").is_none(), "nothing is left behind");
}
