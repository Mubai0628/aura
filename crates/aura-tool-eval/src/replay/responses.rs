use serde_json::{Value, json};

/// Real semantic SSE, including reasoning items the product must return on later tool turns.
pub(super) fn response(index: u64, delta: &Value) -> Vec<u8> {
    let id = format!("eval_response_{index}");
    let reasoning_id = format!("eval_reasoning_{index}");
    let reason = "Check the isolated fixture and preserve the regression test.";
    let reasoning = json!({"type":"reasoning","id":reasoning_id,"content":[{"type":"reasoning_text","text":reason}],"summary":[]});
    let mut events = vec![
        json!({"type":"response.created","response":{"id":id,"status":"in_progress"}}),
        json!({"type":"response.output_item.added","output_index":0,"item":{"type":"reasoning","id":reasoning_id,"summary":[]}}),
        json!({"type":"response.reasoning_text.delta","output_index":0,"content_index":0,"item_id":reasoning_id,"delta":reason}),
        json!({"type":"response.reasoning_text.done","output_index":0,"content_index":0,"item_id":reasoning_id,"text":reason}),
        json!({"type":"response.output_item.done","output_index":0,"item":reasoning}),
    ];
    let output = if let Some(call) = delta.pointer("/tool_calls/0") {
        let item_id = format!("eval_function_{index}");
        let item = json!({"type":"function_call","id":item_id,"call_id":call["id"],"name":call["function"]["name"],"arguments":call["function"]["arguments"],"status":"completed"});
        events.extend([
            json!({"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","id":item_id,"call_id":call["id"],"name":call["function"]["name"],"arguments":"","status":"in_progress"}}),
            json!({"type":"response.function_call_arguments.delta","output_index":1,"item_id":item_id,"delta":call["function"]["arguments"]}),
            json!({"type":"response.function_call_arguments.done","output_index":1,"item_id":item_id,"arguments":call["function"]["arguments"]}),
            json!({"type":"response.output_item.done","output_index":1,"item":item}),
        ]);
        item
    } else {
        let item_id = format!("eval_message_{index}");
        let text = &delta["content"];
        let item = json!({"type":"message","id":item_id,"role":"assistant","status":"completed","content":[{"type":"output_text","text":text,"annotations":[]}]});
        events.extend([
            json!({"type":"response.output_item.added","output_index":1,"item":{"type":"message","id":item_id,"role":"assistant","content":[]}}),
            json!({"type":"response.output_text.delta","output_index":1,"content_index":0,"item_id":item_id,"delta":text}),
            json!({"type":"response.output_text.done","output_index":1,"content_index":0,"item_id":item_id,"text":text}),
            json!({"type":"response.output_item.done","output_index":1,"item":item}),
        ]);
        item
    };
    events.push(json!({"type":"response.completed","response":{"id":id,"status":"completed","output":[reasoning,output],"usage":{"input_tokens":1000,"input_tokens_details":{"cached_tokens":400},"output_tokens":100,"output_tokens_details":{"reasoning_tokens":40},"total_tokens":1100}}}));
    let mut sse = String::new();
    for (sequence, mut event) in events.into_iter().enumerate() {
        event["sequence_number"] = json!(sequence);
        sse.push_str(&format!(
            "event: {}\ndata: {event}\n\n",
            event["type"].as_str().expect("fixed replay event type")
        ));
    }
    sse.into_bytes()
}
