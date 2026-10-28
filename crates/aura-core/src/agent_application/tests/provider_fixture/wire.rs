use super::*;

/// Everything the fixture's tool results put in front of the model.
pub(super) fn tool_content(body: &Value) -> String {
    body["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|message| message["role"] == "tool")
        .filter_map(|message| message["content"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The absolute path the test named in the user message.
pub(super) fn named_external_path(body: &Value) -> String {
    const MARKER: &str = "AURA_EXTERNAL_PATH=";
    let text = body["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|message| message["role"] == "user")
        .filter_map(|message| message["content"].as_str())
        .find_map(|text| text.find(MARKER).map(|index| text[index + MARKER.len()..].to_owned()))
        .expect("the test names the external path in the user message");
    // The test must not put a space in the path it names.
    text.split_whitespace().next().expect("a named path").to_owned()
}

pub(in crate::agent_application::tests) fn recovery_response(
    state: &Value,
    call: &impl Fn(&str, Value, usize) -> Value,
) -> Value {
    let turn = &state["turn"];
    let lineages = state["lineages"].as_array().expect("persisted lineages");
    let active = lineages
        .iter()
        .find(|l| l["lineage_id"] == turn["lineage_id"])
        .or_else(|| lineages.iter().find(|l| l["state"] == "active"))
        .expect("resolved lineage");
    json!({"role":"assistant","tool_calls":[call("normal.complete", json!({
        "lineage_id":active["lineage_id"],"expected_lineage_revision":active["revision"],
        "final_report":"The requested read-only answer is complete.","disclosed_limitations":[]
    }), 0)]})
}

pub(super) async fn send(stream: &mut tokio::net::TcpStream, ordinal: usize, delta: Value) {
    send_with(stream, ordinal, delta, None).await
}

/// Writes one Responses SSE body, `event:` and `data:` per event, exactly as the
/// wire carries it.
pub(super) async fn send_responses(stream: &mut tokio::net::TcpStream, events: &[Value]) {
    let mut content = String::new();
    for event in events {
        let kind = event["type"].as_str().expect("real Provider fixture request and response");
        content.push_str(&format!("event: {kind}\ndata: {event}\n\n"));
    }
    content.push_str("data: [DONE]\n\n");
    stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{content}",content.len()).as_bytes()).await.expect("real Provider fixture request and response");
}

/// `finish` overrides the derived reason so a fixture can end a response the way
/// a real Provider truncates one.
pub(in crate::agent_application::tests) async fn send_with(
    stream: &mut tokio::net::TcpStream,
    ordinal: usize,
    delta: Value,
    finish: Option<&str>,
) {
    send_counted(stream, ordinal, delta, finish, 20).await;
}

/// The same reply with the Provider's count of the request it answers.
pub(super) async fn send_counted(
    stream: &mut tokio::net::TcpStream,
    ordinal: usize,
    delta: Value,
    finish: Option<&str>,
    prompt_tokens: u64,
) {
    let reason =
        finish.unwrap_or(if delta.get("tool_calls").is_some() { "tool_calls" } else { "stop" });
    let payload = json!({"id":format!("fixture-response-{ordinal}"),"object":"chat.completion.chunk","model":"fixture","choices":[{"index":0,"delta":delta,"finish_reason":reason}],"usage":{"prompt_tokens":prompt_tokens,"completion_tokens":20,"total_tokens":prompt_tokens + 20}});
    let content = format!("data: {payload}\n\ndata: [DONE]\n\n");
    stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{content}",content.len()).as_bytes()).await.expect("real Provider fixture request and response");
}

/// An answer whose Provider says how much of the request it read from its cache.
pub(super) async fn send_cached(
    stream: &mut tokio::net::TcpStream,
    ordinal: usize,
    delta: Value,
    prompt_tokens: u64,
    cached_tokens: u64,
) {
    let usage = json!({"prompt_tokens":prompt_tokens,"completion_tokens":20,
        "total_tokens":prompt_tokens + 20,"prompt_tokens_details":{"cached_tokens":cached_tokens}});
    let payload = json!({"id":format!("fixture-response-{ordinal}"),"object":"chat.completion.chunk","model":"fixture","choices":[{"index":0,"delta":delta,"finish_reason":"stop"}],"usage":usage});
    let content = format!(
        "data: {payload}

data: [DONE]

"
    );
    stream
        .write_all(
            format!(
                "HTTP/1.1 200 OK
Content-Type: text/event-stream
Content-Length: {}
Connection: close

{content}",
                content.len()
            )
            .as_bytes(),
        )
        .await
        .expect("real Provider fixture request and response");
}

pub(in crate::agent_application::tests) fn state(body: &Value) -> Value {
    json_object(body, "turn")
}

pub(super) fn json_object(body: &Value, key: &str) -> Value {
    for message in body["messages"].as_array().expect("real Provider fixture request and response")
    {
        if !matches!(message["role"].as_str(), Some("system" | "developer")) {
            continue;
        }
        let Some(content) = message["content"].as_str() else {
            continue;
        };
        for (position, _) in content.match_indices('{') {
            let mut stream =
                serde_json::Deserializer::from_str(&content[position..]).into_iter::<Value>();
            if let Some(Ok(value)) = stream.next()
                && value.get(key).is_some()
            {
                return value;
            }
        }
    }
    panic!("Core task state is missing from real request");
}

/// What every compaction request says about the summary it asks for.
pub(crate) const COMPACTION_FORMAT: &str = "Write the summary as one paragraph";

/// The instruction message that asks for a compaction — the last system or developer
/// message carrying the summary's format. On the step's prefix it is the task state; asked the
/// isolated way it is the only instruction message.
pub(super) fn compaction_ask(body: &Value) -> String {
    body["messages"]
        .as_array()
        .expect("real Provider fixture request and response")
        .iter()
        .filter(|message| matches!(message["role"].as_str(), Some("system" | "developer")))
        .filter_map(|message| message["content"].as_str())
        .rfind(|content| content.contains(COMPACTION_FORMAT))
        .expect("a compaction ask")
        .to_owned()
}

/// A compaction answered with a write instead of its summary. The request carries the
/// step's tools, so the model could ask for one; Core must never run it.
pub(super) fn compaction_tool_call(body: &Value) -> Value {
    let entry = aura_tool_registry::agent_release::release()
        .expect("real Provider fixture request and response")
        .entry("fs.write")
        .expect("real Provider fixture request and response");
    let wire = aura_model_domain::provider_wire_tool_name("fs.write", entry.definition.tool_id);
    assert!(
        body["tools"]
            .as_array()
            .expect("the compaction carries the step's tools")
            .iter()
            .any(|tool| tool["function"]["name"] == wire),
        "the compaction carries the step's write tool"
    );
    let arguments = flat_call(
        "Write while asked for a summary",
        json!({"path":super::scenario::COMPACTION_TOOL_FILE,"text":"a compaction must not write",
            "expected":{"expected_kind":"absent"}}),
    );
    json!({"role":"assistant","tool_calls":[{"index":0,"id":"fixture_compaction_write",
        "type":"function","function":{"name":wire,"arguments":arguments}}]})
}

/// One envelope-3 tool call as a Provider sends it: the tool's own fields beside
/// `aura_purpose`, in one JSON object.
pub(crate) fn flat_call(purpose: &str, fields: Value) -> String {
    let mut call = fields.as_object().cloned().expect("tool arguments are a JSON object");
    call.insert("aura_purpose".into(), Value::String(purpose.into()));
    Value::Object(call).to_string()
}

/// A compaction answer whose paragraph runs past four times the length the ask set for it, so it
/// is refused.
pub(super) fn long_compaction_answer(ask: &str) -> String {
    let target = ask
        .split("of plain prose of at most ")
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .and_then(|number| number.parse::<usize>().ok())
        .expect("the ask names the summary's length");
    "Everything that happened, retold at length. ".repeat(target * 4 / 40 + 50)
}

/// The paragraph a fixture compaction answers with.
pub(crate) const COMPACTION_PARAGRAPH: &str =
    "The prior response is summarized here; finish the original user's request.";
