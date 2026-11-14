use aura_mcp::{
    ContentBlock, FrameError, LineFramer, MAX_MESSAGE_BYTES, McpTool, Message, RequestId,
    cancelled_params, encode_request, initialize_params, model_text, normalize_tools,
    parse_call_result, parse_initialize_result, parse_messages, parse_tools_page,
};
use serde_json::json;

#[test]
fn lines_frame_messages_and_an_endless_line_is_refused() {
    let mut framer = LineFramer::new();
    assert_eq!(framer.push(b"{\"a\":1}\r\n\n{\"b\"").expect("frames"), vec![b"{\"a\":1}".to_vec()]);
    assert_eq!(framer.push(b":2}\n").expect("frames"), vec![b"{\"b\":2}".to_vec()]);
    let mut endless = LineFramer::new();
    let chunk = vec![b'x'; 1024 * 1024];
    let mut result = Ok(vec![]);
    for _ in 0..=(MAX_MESSAGE_BYTES / chunk.len()) {
        result = endless.push(&chunk);
        if result.is_err() {
            break;
        }
    }
    assert_eq!(result, Err(FrameError::TooLarge));
}

#[test]
fn messages_are_routed_by_shape() {
    let parsed = parse_messages(
        br#"[{"jsonrpc":"2.0","id":7,"result":{"ok":true}},
            {"jsonrpc":"2.0","method":"notifications/tools/list_changed"},
            {"jsonrpc":"2.0","id":"p","method":"ping"},
            {"jsonrpc":"2.0","id":8,"error":{"code":-32602,"message":"bad"}}]"#,
    )
    .expect("batch");
    assert!(matches!(&parsed[0], Message::Response { id: RequestId::Number(7), .. }));
    assert!(
        matches!(&parsed[1], Message::Notification { method, .. } if method == "notifications/tools/list_changed")
    );
    assert!(
        matches!(&parsed[2], Message::Request { id: RequestId::Text(id), method, .. } if id == "p" && method == "ping")
    );
    assert!(matches!(&parsed[3], Message::Error { error, .. } if error.code == -32602));
    assert!(parse_messages(br#"{"id":1,"result":{}}"#).is_err(), "jsonrpc 2.0 is required");
    assert!(parse_messages(b"not json").is_err());
    let line = encode_request(&RequestId::Number(3), "tools/list", Some(json!({})));
    assert_eq!(line.last(), Some(&b'\n'));
    assert_eq!(line.iter().filter(|b| **b == b'\n').count(), 1, "one message is one line");
}

#[test]
fn initialize_negotiates_a_known_version_and_declares_no_client_capability() {
    let params = initialize_params("0.1.0");
    assert_eq!(params["capabilities"], json!({}), "no roots, sampling or elicitation");
    assert_eq!(params["clientInfo"]["name"], "Aura");
    let accepted = parse_initialize_result(&json!({
        "protocolVersion": "2025-06-18",
        "capabilities": {"tools": {"listChanged": true}},
        "serverInfo": {"name": "fixture", "version": "1.0"},
        "instructions": "use tools carefully"
    }))
    .expect("known version");
    assert!(accepted.tools_supported && accepted.tools_list_changed);
    assert_eq!(accepted.server_info.name, "fixture");
    assert!(
        parse_initialize_result(&json!({"protocolVersion": "2024-11-05", "capabilities": {}}))
            .is_err()
    );
}

#[test]
fn tool_pages_and_results_are_read_without_trusting_binary_content() {
    let page = parse_tools_page(&json!({
        "tools": [{"name": "echo", "description": "Echo", "inputSchema": {"type": "object"}}],
        "nextCursor": "2"
    }))
    .expect("page");
    assert_eq!(page.tools[0].name, "echo");
    assert_eq!(page.next_cursor.as_deref(), Some("2"));
    let result = parse_call_result(&json!({
        "content": [
            {"type": "text", "text": "hello"},
            {"type": "image", "data": "AAAA", "mimeType": "image/png"},
            {"type": "resource_link", "uri": "file:///x", "name": "x"}
        ],
        "isError": false
    }))
    .expect("result");
    assert!(matches!(result.content[1], ContentBlock::Image { bytes: 3, .. }));
    let (text, truncated) = model_text(&result, 10_000);
    assert!(
        text.starts_with("hello\n[图片 image/png") && text.contains("未发给模型") && !truncated
    );
    let (cut, truncated) = model_text(&result, 3);
    assert_eq!((cut.as_str(), truncated), ("hel", true));
    let structured =
        parse_call_result(&json!({"structuredContent": {"n": 1}})).expect("structured");
    assert_eq!(model_text(&structured, 100).0, "{\"n\":1}");
    assert!(parse_call_result(&json!({"content": "x"})).is_err());
    assert_eq!(cancelled_params(&RequestId::Number(4), "用户停止")["requestId"], 4);
}

fn tool(name: &str, schema: serde_json::Value) -> McpTool {
    McpTool { name: name.into(), title: None, description: Some("d".into()), input_schema: schema }
}

#[test]
fn tools_get_stable_aura_names_and_bad_ones_are_skipped_with_the_reason() {
    let object = json!({"type": "object"});
    let catalog = normalize_tools(
        "files",
        vec![
            tool("read_file", object.clone()),
            tool("Read File", object.clone()),
            tool("read-file", object.clone()),
            tool("_hidden", object.clone()),
            tool("no_schema", json!(null)),
            tool("array_schema", json!({"type": "array"})),
            tool("read_file", object.clone()),
        ],
    );
    let names: Vec<_> = catalog.tools.iter().map(|t| t.canonical_name.as_str()).collect();
    assert_eq!(
        names,
        [
            "mcp.files.read_file",
            "mcp.files.read_file_2",
            "mcp.files.read-file",
            "mcp.files.t_hidden"
        ]
    );
    assert_eq!(catalog.tools[1].wire_name, "Read File", "the call uses the server's own name");
    assert_eq!(catalog.skipped.len(), 3, "{:?}", catalog.skipped);
    assert!(catalog.skipped.iter().any(|s| s.contains("同名")));
    let again = normalize_tools("files", vec![tool("read_file", object.clone())]);
    let changed = normalize_tools(
        "files",
        vec![McpTool { description: Some("changed".into()), ..tool("read_file", object) }],
    );
    assert_ne!(again.tools[0].fingerprint, changed.tools[0].fingerprint);
    assert_ne!(again.fingerprint, changed.fingerprint);
}
