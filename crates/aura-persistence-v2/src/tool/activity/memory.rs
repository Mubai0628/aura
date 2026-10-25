//! The managed Vault scope and titles come from the authorized operation, never a body dump.
use aura_execution_contract::SecuredOperation;
use aura_security_contract::{MemoryVaultScope, ResourceDescriptor};
use aura_tool_projection::AgentToolDisplay;

pub(super) fn decorate(
    mut view: AgentToolDisplay,
    operation: Option<&SecuredOperation>,
) -> AgentToolDisplay {
    let Some(operation) = operation else { return view };
    let Some(scope) = operation.content.resources.iter().find_map(|r| match r {
        ResourceDescriptor::MemoryVault { scope, .. } => Some(scope),
        _ => None,
    }) else {
        return view;
    };
    view.title = match operation.content.action_kind.as_str() {
        "memory.search" => "搜索记忆",
        "memory.read" => "读取记忆",
        "memory.list_recent" => "最近记忆",
        "memory.create" => "创建记忆",
        "memory.update" => "更新记忆",
        "memory.link" => "关联记忆",
        "memory.forget" => "标记遗忘",
        "memory.archive" => "归档记忆",
        "memory.delete" => "删除记忆",
        "memory.validate" => "校验记忆",
        "memory.repair" => "修复记忆",
        _ => return view,
    }
    .into();
    view.target = Some(
        match scope {
            MemoryVaultScope::Global => "全局记忆",
            MemoryVaultScope::Project { .. } => "当前项目记忆",
        }
        .into(),
    );
    if let Some(titles) = operation.content.action["titles"].as_array() {
        for title in titles.iter().filter_map(serde_json::Value::as_str).take(5) {
            view.details.push(format!("记忆：{}", title.chars().take(200).collect::<String>()));
        }
        view.truncated |= titles.len() > 5;
    }
    view
}
