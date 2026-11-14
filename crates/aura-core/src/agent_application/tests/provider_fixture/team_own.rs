//! [`TeamPlan::OwnPlaces`]: a sub-agent in a worktree works in its copy and in its temporary
//! folder, and the main agent then changes files in that copy and outside its workspace.
use super::*;
use std::path::PathBuf;

/// The picture the test commits; the sub-agent reads it in its copy.
pub const OWN_PICTURE: &str = "shot.png";
/// The committed file the sub-agent's command deletes in its copy.
pub const OWN_DELETED: &str = "lib.txt";
/// The committed file the main agent deletes in the sub-agent's copy.
pub const MAIN_DELETED: &str = "readme.txt";
/// What the sub-agent's command writes in its temporary folder, and reads back.
pub const OWN_NOTE: &str = "own-note-text";

/// The text after `marker` in the user's message or the sub-agent's task sheet, up to a space.
pub(super) fn marked(body: &Value, marker: &str) -> String {
    body["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|message| message["role"] == "user")
        .filter_map(|message| message["content"].as_str())
        .find_map(|text| text.find(marker).map(|at| text[at + marker.len()..].to_owned()))
        .and_then(|rest| rest.split_whitespace().next().map(str::to_owned))
        .unwrap_or_else(|| panic!("the test names {marker}"))
}

/// The sub-agent's copy: the one folder under the worktree folder the user's message names.
fn copy(body: &Value) -> PathBuf {
    let one = |path: PathBuf| {
        let mut found = std::fs::read_dir(&path)
            .map(|entries| entries.flatten().map(|entry| entry.path()).collect::<Vec<_>>())
            .unwrap_or_default();
        assert_eq!(found.len(), 1, "one folder in {}: {found:?}", path.display());
        found.remove(0)
    };
    one(one(PathBuf::from(marked(body, "AURA_COPIES="))))
}

/// What a command printed after `name`, on a line of its own.
fn printed(body: &Value, name: &str) -> String {
    fn find(value: &Value, name: &str) -> Option<String> {
        match value {
            Value::String(text) => text
                .lines()
                .find_map(|line| line.trim().strip_prefix(name).map(str::to_owned))
                .or_else(|| serde_json::from_str::<Value>(text).ok().and_then(|v| find(&v, name))),
            Value::Array(values) => values.iter().find_map(|value| find(value, name)),
            Value::Object(fields) => fields.values().find_map(|value| find(value, name)),
            _ => None,
        }
    }
    body["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|message| message["role"] == "tool")
        .find_map(|message| find(&message["content"], name))
        .unwrap_or_else(|| panic!("a command printed {name}"))
}

pub(super) fn main(body: &Value, calling: &impl Fn(&str, Value) -> Value) -> Value {
    match results(body) {
        0 => {
            let goal = format!("改代码 AURA_COPIES={}", marked(body, "AURA_COPIES="));
            calling(
                "agents.spawn",
                json!({"name":"改代码","task":{"goal":goal,"done_when":"改完"},"worktree":true}),
            )
        }
        1 => calling("agents.wait", json!({"agents":["A1"],"timeout_seconds":60})),
        2 => calling(
            "fs.delete",
            json!({"path": copy(body).join(MAIN_DELETED).display().to_string()}),
        ),
        3 => calling("fs.delete", json!({"path": named_external_path(body)})),
        _ => answer("收到。"),
    }
}

pub(super) fn sub_agent(body: &Value, calling: &impl Fn(&str, Value) -> Value) -> Value {
    let read = |path: String| {
        calling(
            "fs.read",
            json!({"path": path, "range": {"range_kind": "full"}, "explicit_encoding": null}),
        )
    };
    match results(body) {
        // Writes its temporary folder and deletes in its copy, with no question.
        0 => calling(
            "shell.powershell",
            json!({"script": format!(
                "Write-Output \"TEMP=$env:TEMP\"; Set-Content -Path \"$env:TEMP\\note.txt\" -Value {OWN_NOTE}; Remove-Item {OWN_DELETED}"
            )}),
        ),
        // Names its folder in full.
        1 => calling(
            "shell.powershell",
            json!({"script": "Write-Output here", "working_directory": copy(body).display().to_string()}),
        ),
        2 => calling(
            "fs.write",
            json!({
                "path": format!("{}\\written.txt", printed(body, "TEMP=")),
                "text": "x",
                "expected": {"expected_kind": "absent"}
            }),
        ),
        3 => read(format!("{}\\note.txt", printed(body, "TEMP="))),
        4 => read(OWN_PICTURE.to_owned()),
        _ => answer(SUMMARY),
    }
}
