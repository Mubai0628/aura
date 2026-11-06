//! A conversation's MCP resources: the "@" list names those of the servers that are ready and
//! said they offer resources, starting only those, and a resource is read as text when a message
//! that refers to it is sent.
use super::support::Fixture;
use aura_domain_common::SessionId;
use serde_json::json;

#[test]
fn the_list_names_the_resources_of_ready_servers_that_offer_them() {
    let f = Fixture::new();
    f.save(f.server("notes", &["--resources"]));
    assert_eq!(f.trust("notes")["state"], "ready");
    f.save(f.server("files", &[]));
    assert_eq!(f.trust("files")["state"], "ready");
    // Discovery's own instances have ended; only the list starts one again.
    assert_eq!(f.runtime.running_instances("notes"), 0);
    let session = SessionId::new();
    let listed = f.dispatch("agent.mcp.resources", json!({"session_id": session})).expect("listed");
    assert_eq!(
        listed["resources"],
        json!([
            {"server_id": "notes", "server_name": "测试 notes", "uri": "fixture://notes/readme", "name": "readme", "title": "Fixture notes", "mime_type": "text/markdown"},
            {"server_id": "notes", "server_name": "测试 notes", "uri": "fixture://logo", "name": "logo", "mime_type": "image/png"},
        ])
    );
    assert_eq!(listed["problems"], json!([]));
    // A server that offers no resources is not started for the list.
    assert_eq!(f.runtime.running_instances("files"), 0);
    assert_eq!(f.runtime.running_instances("notes"), 1);

    // Read as text for the message, binary parts described.
    let read = f.runtime.read_resource(session, "notes", "fixture://notes/readme").expect("read");
    assert_eq!(read.server_name, "测试 notes");
    assert_eq!(read.text, "# Fixture notes\nThe first line.");
    assert!(!read.truncated);
    let logo = f.runtime.read_resource(session, "notes", "fixture://logo").expect("read");
    assert!(logo.text.contains("binary image/png"), "{}", logo.text);
    // Only a ready server's resource, by its id.
    let missing =
        f.runtime.read_resource(session, "gone", "fixture://logo").expect_err("no server");
    assert!(missing.to_string().starts_with("mcp.not_found"), "{missing}");
}

#[test]
fn a_server_switched_off_is_not_listed_and_its_resources_are_not_read() {
    let f = Fixture::new();
    f.save(f.server("notes", &["--resources"]));
    f.trust("notes");
    f.dispatch(
        "agent.mcp.set_enabled",
        json!({"id": "notes", "enabled": false, "expected_revision": f.revision()}),
    )
    .expect("switched off");
    let session = SessionId::new();
    let listed = f.dispatch("agent.mcp.resources", json!({"session_id": session})).expect("listed");
    assert_eq!(listed["resources"], json!([]));
    let refused = f
        .runtime
        .read_resource(session, "notes", "fixture://notes/readme")
        .expect_err("switched off");
    assert!(refused.to_string().contains("mcp.resource_unavailable"), "{refused}");
}
