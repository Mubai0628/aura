//! `/init`'s write: the files it proposes, and after success the files it wrote.
use aura_tool_projection::AgentToolDisplay;
use serde_json::Value;

pub(super) const NAME: &str = "workspace_init.complete";

/// "AGENTS.md", or "AGENTS.md 等 3 个文件" when guides are proposed too.
pub(super) fn target(arguments: Option<&Value>) -> Option<String> {
    let guides = arguments.and_then(|value| value["guides"].as_array()).map_or(0, Vec::len);
    Some(if guides == 0 {
        "AGENTS.md".to_owned()
    } else {
        format!("AGENTS.md 等 {} 个文件", guides + 1)
    })
}

pub(super) fn summarize(view: &mut AgentToolDisplay, value: &Value) {
    let Some(files) = value["files"].as_array() else { return };
    if value["disposition"] == "deferred_by_editor_activity" {
        view.summary = "文件正在编辑器中修改，未写入".into();
        return;
    }
    view.summary = format!("已写入 {} 个文件", files.len());
    view.details = files
        .iter()
        .filter_map(|file| {
            let path = file["path"].as_str()?;
            let action = if file["created"] == true { "新建" } else { "更新" };
            Some(format!("{action} {}", path.chars().take(200).collect::<String>()))
        })
        .collect();
}
