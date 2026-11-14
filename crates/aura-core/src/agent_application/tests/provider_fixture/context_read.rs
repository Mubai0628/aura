//! A model reading a call's stored result again: a short command, then `context.read_tool_result`
//! of that call through the unified entry, as after a cut-off note names a call. The history
//! tools took its place in the default list, so the entry is how a model reaches it.
use super::*;

/// What the command prints; the read must give it back.
pub const READ_RESULT_MARK: &str = "AURA-READ-RESULT-MARK";

pub(super) fn reply(body: &Value, call: &impl Fn(&str, Value, usize) -> Value) -> Value {
    let results: Vec<Value> = body["messages"]
        .as_array()
        .map(|messages| messages.iter().filter(|m| m["role"] == "tool").cloned().collect())
        .unwrap_or_default();
    match results.as_slice() {
        [] => json!({"role":"assistant","tool_calls":[call(
            "shell.powershell",
            json!({"script": format!("'{READ_RESULT_MARK}'")}),
            0,
        )]}),
        [shell] => {
            let view: Value =
                serde_json::from_str(shell["content"].as_str().unwrap_or("{}")).unwrap_or_default();
            let id = view["outcome"]["payload"]["value"]["tool_call_id"].clone();
            json!({"role":"assistant","tool_calls":[call(
                "tools.call",
                json!({
                    "tool": "context.read_tool_result",
                    "arguments": {"tool_call_id": id, "include_input": false, "cursor": null},
                }),
                1,
            )]})
        }
        _ => json!({"role":"assistant","content":"Done."}),
    }
}
