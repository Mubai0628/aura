//! An MCP resource the composer refers to: which server's, its address and the name it is kept
//! under. Core reads it when the message is sent; the draft carries no contents of its own.
use aura_agent_domain::{AgentAttachmentDraft, AttachmentError, validate_attachment_drafts};

fn resource(server_id: &str, uri: &str, name: &str) -> AgentAttachmentDraft {
    AgentAttachmentDraft::McpResource {
        server_id: server_id.into(),
        uri: uri.into(),
        name: name.into(),
    }
}

#[test]
fn a_resource_is_checked_for_its_server_address_and_name() {
    assert_eq!(
        validate_attachment_drafts(&[resource("notes", "fixture://notes/readme", "Fixture notes")]),
        Ok(())
    );
    assert_eq!(
        validate_attachment_drafts(&[resource("notes", "fixture://x", "a/b")]),
        Err(AttachmentError::NameInvalid)
    );
    for (server, uri) in [("", "fixture://x"), ("notes", ""), ("notes", "fixture://\u{0}")] {
        assert_eq!(
            validate_attachment_drafts(&[resource(server, uri, "x")]),
            Err(AttachmentError::ResourceInvalid),
            "{server:?} {uri:?}"
        );
    }
    let long = format!("fixture://{}", "x".repeat(2048));
    assert_eq!(
        validate_attachment_drafts(&[resource("notes", &long, "x")]),
        Err(AttachmentError::ResourceInvalid)
    );
}

#[test]
fn a_resources_receipt_is_the_reference_itself() {
    let draft = resource("notes", "fixture://notes/readme", "readme");
    assert_eq!(draft.receipt(), draft);
    let json = serde_json::to_value(&draft).expect("json");
    assert_eq!(
        json,
        serde_json::json!({
            "kind": "mcp_resource",
            "server_id": "notes",
            "uri": "fixture://notes/readme",
            "name": "readme",
        })
    );
}
