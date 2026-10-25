use super::*;

/// One step of the raw OpenAI Responses stream the binding incident needs.
///
/// Step 1 asks for one real file write and completes normally. Step 2 names its
/// function call `_aura`, which no frozen request exposes. Later steps — a new
/// human continuation — answer in plain text. Nothing here passes a name from an
/// arguments object: `_aura` appears as a function name only where a Provider
/// really put it there.
pub(super) async fn responses_binding_step(
    stream: &mut tokio::net::TcpStream,
    ordinal: usize,
    body: &Value,
) {
    let created = json!({
        "type": "response.created",
        "response": {"id": format!("resp_fixture_{ordinal}")}
    });
    let completed = json!({
        "type": "response.completed",
        "response": {
            "id": format!("resp_fixture_{ordinal}"),
            "usage": {"input_tokens": 24, "output_tokens": 12, "total_tokens": 36}
        }
    });
    let mut events = vec![created];
    match ordinal {
        0 => {
            let release = aura_tool_registry::agent_release::release()
                .expect("real Provider fixture request and response");
            let entry =
                release.entry("fs.write").expect("real Provider fixture request and response");
            let wire = provider_wire_tool_name("fs.write", entry.definition.tool_id);
            assert!(
                body["tools"]
                    .as_array()
                    .expect("real Provider fixture request and response")
                    .iter()
                    .any(|t| t["name"] == wire),
                "the frozen request must expose fs.write"
            );
            let arguments = json!({
                "aura_purpose": "Write the fixture file exactly once",
                "path": "agent-result.txt",
                "text": RESPONSES_WRITE_TOKEN,
                "expected": {"expected_kind": "absent"}
            })
            .to_string();
            let (first, second) = arguments.split_at(arguments.len() / 2);
            events.extend([
                json!({
                    "type": "response.output_item.added",
                    "output_index": 0,
                    "item": {
                        "type": "function_call",
                        "id": "item_write",
                        "call_id": RESPONSES_WRITE_CALL_ID,
                        "name": wire
                    }
                }),
                json!({
                    "type": "response.function_call_arguments.delta",
                    "item_id": "item_write",
                    "output_index": 0,
                    "delta": first
                }),
                json!({
                    "type": "response.function_call_arguments.delta",
                    "item_id": "item_write",
                    "output_index": 0,
                    "delta": second
                }),
                json!({
                    "type": "response.output_item.done",
                    "output_index": 0,
                    "item": {
                        "type": "function_call",
                        "id": "item_write",
                        "call_id": RESPONSES_WRITE_CALL_ID,
                        "name": wire,
                        "arguments": arguments
                    }
                }),
            ]);
        }
        1 => {
            // The illegal shape: the Provider really named its function `_aura`.
            let arguments = json!({"aura_purpose": "Illegal function name"}).to_string();
            events.extend([
                json!({
                    "type": "response.output_item.added",
                    "output_index": 0,
                    "item": {
                        "type": "function_call",
                        "id": "item_illegal",
                        "call_id": "call_illegal",
                        "name": "_aura"
                    }
                }),
                json!({
                    "type": "response.function_call_arguments.delta",
                    "item_id": "item_illegal",
                    "output_index": 0,
                    "delta": arguments
                }),
                json!({
                    "type": "response.output_item.done",
                    "output_index": 0,
                    "item": {
                        "type": "function_call",
                        "id": "item_illegal",
                        "call_id": "call_illegal",
                        "name": "_aura",
                        "arguments": arguments
                    }
                }),
            ]);
        }
        _ => {
            // A new human continuation after the interruption: a complete text
            // answer, so the new turn can finish without dispatching anything.
            events.push(json!({
                "type": "response.output_text.delta",
                "item_id": "msg_continuation",
                "output_index": 0,
                "content_index": 0,
                "delta": "The earlier write is recorded; nothing was replayed."
            }));
        }
    }
    events.push(completed);
    send_responses(stream, &events).await;
}
