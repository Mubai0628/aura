//! [`TeamPlan::ModeSwitch`]: a sub-agent sharing the workspace writes two files; the user changes
//! its permission mode between them.
use super::*;

pub(super) fn main(body: &Value, calling: &impl Fn(&str, Value) -> Value) -> Value {
    match results(body) {
        0 => calling(
            "agents.spawn",
            json!({"name":"写文件","task":{"goal":"写两个文件","done_when":"写完"}}),
        ),
        1 => calling("agents.wait", json!({"agents":["A1"],"timeout_seconds":60})),
        _ => answer("收到。"),
    }
}

pub(super) fn sub_agent(body: &Value, calling: &impl Fn(&str, Value) -> Value) -> Value {
    let write = |path: &str| {
        calling(
            "fs.write",
            json!({"path": path, "text": "x", "expected": {"expected_kind": "absent"}}),
        )
    };
    match results(body) {
        0 => write("first.txt"),
        1 => write("second.txt"),
        _ => answer(SUMMARY),
    }
}
