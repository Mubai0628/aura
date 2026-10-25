//! Native Git facts are projected by their trusted source; Renderer does not infer backend policy.
use aura_tool_contract::ToolSourceKindV1;
use aura_tool_projection::AgentToolDisplay;
use aura_workspace_domain::GIT_NATIVE_EXECUTION_NOTICE;
use serde_json::Value;

pub(super) fn decorate(
    mut view: AgentToolDisplay,
    name: &str,
    source: ToolSourceKindV1,
) -> AgentToolDisplay {
    if source == ToolSourceKindV1::BuiltIn
        && matches!(name, "git.commit" | "git.branch" | "git.sync" | "git.push")
    {
        if !view.summary.is_empty() {
            view.summary.push('；');
        }
        view.summary.push_str(GIT_NATIVE_EXECUTION_NOTICE);
    }
    view
}

pub(super) fn summarize(view: &mut AgentToolDisplay, name: &str, value: &Value) {
    let text = |text: &str| text.chars().take(160).collect::<String>();
    if name == "git.commit" {
        let commit = value["commit"]["commit"].as_str().unwrap_or("");
        let subject = value["commit"]["subject"].as_str().unwrap_or("");
        view.summary =
            format!("提交 {}：{}", commit.chars().take(12).collect::<String>(), text(subject));
    } else if name == "git.push" {
        view.summary = format!(
            "推送 {} → {}/{}（{} 个ref已核对）",
            text(value["local_branch"].as_str().unwrap_or("")),
            text(value["remote"].as_str().unwrap_or("")),
            text(value["remote_branch"].as_str().unwrap_or("")),
            value["refs"].as_array().map_or(0, Vec::len)
        );
    } else if name == "git.sync" {
        view.summary = format!(
            "{} {}（{} 个ref已更新）",
            if value["kind"] == "pull" { "快进同步" } else { "获取远端" },
            text(value["remote"].as_str().unwrap_or("")),
            value["refs"].as_array().map_or(0, Vec::len)
        );
    } else {
        view.summary = format!(
            "{}分支 {}",
            if value["created"] == true && value["switched"] == true {
                "新建并切换"
            } else if value["created"] == true {
                "新建"
            } else {
                "切换"
            },
            text(value["branch"].as_str().unwrap_or(""))
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn view() -> AgentToolDisplay {
        AgentToolDisplay {
            title: "fixture".into(),
            target: None,
            summary: "操作已返回，实际影响尚未确认".into(),
            details: vec![],
            truncated: false,
            internal: false,
            image: None,
            offer: None,
        }
    }
    #[test]
    fn the_native_notice_preserves_unknown_facts_and_does_not_apply_to_reads_or_other_sources() {
        let unknown = decorate(view(), "git.commit", ToolSourceKindV1::BuiltIn);
        assert!(unknown.summary.starts_with("操作已返回，实际影响尚未确认"));
        assert!(unknown.summary.ends_with(GIT_NATIVE_EXECUTION_NOTICE));
        assert_eq!(
            decorate(view(), "git.status", ToolSourceKindV1::BuiltIn).summary,
            view().summary
        );
        assert_eq!(decorate(view(), "git.commit", ToolSourceKindV1::Mcp).summary, view().summary);
    }
    #[test]
    fn success_summary_names_only_the_recorded_commit_or_branch() {
        let mut v = view();
        summarize(
            &mut v,
            "git.commit",
            &serde_json::json!({"commit":{"commit":"a".repeat(40),"subject":"真实回执"}}),
        );
        assert_eq!(v.summary, "提交 aaaaaaaaaaaa：真实回执");
        summarize(
            &mut v,
            "git.branch",
            &serde_json::json!({"branch":"feature/local","created":true,"switched":true}),
        );
        assert_eq!(v.summary, "新建并切换分支 feature/local");
    }
}
