//! MCP resources as Aura reads them: whether a server offers them, its list page by page, and
//! one read as text, with binary parts described and never decoded.
use aura_mcp::{
    McpResource, ResourcePart, parse_initialize_result, parse_read_result, parse_resources_page,
    resource_text,
};
use serde_json::json;

#[test]
fn a_server_offers_resources_when_its_capabilities_say_so() {
    let offered = parse_initialize_result(&json!({
        "protocolVersion": "2025-11-25",
        "capabilities": {"tools": {}, "resources": {"listChanged": true}},
        "serverInfo": {"name": "notes", "version": "1"},
    }))
    .expect("parsed");
    assert!(offered.resources_supported);
    let tools_only = parse_initialize_result(&json!({
        "protocolVersion": "2025-11-25",
        "capabilities": {"tools": {}},
        "serverInfo": {"name": "notes", "version": "1"},
    }))
    .expect("parsed");
    assert!(!tools_only.resources_supported);
}

#[test]
fn a_page_of_resources_keeps_what_each_is_and_where_the_next_page_is() {
    let page = parse_resources_page(&json!({
        "resources": [
            {"uri": "fixture://notes/readme", "name": "readme", "title": "Notes", "description": "The notes", "mimeType": "text/markdown"},
            {"uri": "fixture://logo", "name": "logo"},
        ],
        "nextCursor": "2",
    }))
    .expect("parsed");
    assert_eq!(
        page.resources[0],
        McpResource {
            uri: "fixture://notes/readme".into(),
            name: "readme".into(),
            title: Some("Notes".into()),
            description: Some("The notes".into()),
            mime_type: Some("text/markdown".into()),
        }
    );
    assert_eq!(page.resources[1].title, None);
    assert_eq!(page.next_cursor.as_deref(), Some("2"));
    // A resource without its address or name is not one Aura can name.
    assert!(parse_resources_page(&json!({"resources": [{"name": "x"}]})).is_err());
    assert!(parse_resources_page(&json!({})).is_err());
}

#[test]
fn a_read_resource_is_its_text_with_binary_parts_described() {
    let parts = parse_read_result(&json!({"contents": [
        {"uri": "fixture://notes/readme", "mimeType": "text/markdown", "text": "# Notes\nfirst"},
        {"uri": "fixture://logo", "mimeType": "image/png", "blob": "iVBORw0KGgo="},
    ]}))
    .expect("parsed");
    assert_eq!(
        parts[0],
        ResourcePart::Text {
            uri: "fixture://notes/readme".into(),
            mime_type: Some("text/markdown".into()),
            text: "# Notes\nfirst".into(),
        }
    );
    assert!(matches!(&parts[1], ResourcePart::Blob { bytes: 8, .. }), "{:?}", parts[1]);
    let (text, truncated) = resource_text(&parts, 1024);
    assert_eq!(
        text,
        "# Notes\nfirst\n[fixture://logo: binary image/png, about 8 bytes, not included]"
    );
    assert!(!truncated);
    let (cut, truncated) = resource_text(&parts, 5);
    assert_eq!(cut, "# Not");
    assert!(truncated);
    assert!(parse_read_result(&json!({"contents": "x"})).is_err());
}
