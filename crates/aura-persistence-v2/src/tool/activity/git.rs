//! Native Git facts are projected by their trusted source; Renderer does not infer backend policy.
use aura_execution_contract::SideEffectStateV1;
use aura_tool_contract::{ToolResultOutcomeV1, ToolResultV1, ToolSourceKindV1};
use aura_tool_projection::AgentToolDisplay;
use aura_workspace_domain::GIT_NATIVE_EXECUTION_NOTICE;
use serde_json::Value;

/// A Git write that ran says it ran as Git in a terminal would; one that never ran says only why.
pub(super) fn decorate(
    mut view: AgentToolDisplay,
    name: &str,
    source: ToolSourceKindV1,
    result: Option<&ToolResultV1>,
) -> AgentToolDisplay {
    let outcome = result.map(|result| &result.outcome);
    let ran = matches!(
        outcome,
        Some(
            ToolResultOutcomeV1::Succeeded
                | ToolResultOutcomeV1::Failed { .. }
                | ToolResultOutcomeV1::Indeterminate { .. }
        )
    );
    if ran
        && source == ToolSourceKindV1::BuiltIn
        && matches!(name, "git.commit" | "git.branch" | "git.sync" | "git.push")
    {
        if name == "git.commit"
            && matches!(result, Some(ToolResultV1 {
                side_effect_state: SideEffectStateV1::NoEffect,
                outcome: ToolResultOutcomeV1::Failed { error }, ..
            })
                if error.message.starts_with("git.nothing_to_commit: "))
        {
            view.summary = "没有可提交的改动，没有生成新提交".into();
        }
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
            "推送 {} → {}/{}（{} 个 ref 已核对）",
            text(value["local_branch"].as_str().unwrap_or("")),
            text(value["remote"].as_str().unwrap_or("")),
            text(value["remote_branch"].as_str().unwrap_or("")),
            value["refs"].as_array().map_or(0, Vec::len)
        );
    } else if name == "git.sync" {
        view.summary = format!(
            "{} {}（{} 个 ref 已更新）",
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
    use aura_domain_common::{ToolCallId, UtcTimestamp};
    fn view() -> AgentToolDisplay {
        AgentToolDisplay {
            title: "fixture".into(),
            target: None,
            summary: "结果未知：没能确认是否生效，Aura 不会自动重做".into(),
            details: vec![],
            truncated: false,
            internal: false,
            image: None,
            offer: None,
        }
    }
    #[test]
    fn the_native_notice_preserves_unknown_facts_and_does_not_apply_to_reads_or_other_sources() {
        let result = ToolResultV1 {
            tool_call_id: ToolCallId::new(),
            outcome: ToolResultOutcomeV1::Succeeded,
            side_effect_state: SideEffectStateV1::Unknown,
            attempts: vec![],
            model_visible_result_ref: None,
            user_audit_result_ref: None,
            internal_diagnostic_ref: None,
            aggregate_side_effect_evidence_ref: None,
            started_at: None,
            completed_at: UtcTimestamp::now(),
        };
        let ran = Some(&result);
        let unknown = decorate(view(), "git.commit", ToolSourceKindV1::BuiltIn, ran);
        assert!(unknown.summary.starts_with("结果未知："));
        assert!(unknown.summary.ends_with(GIT_NATIVE_EXECUTION_NOTICE));
        assert_eq!(
            decorate(view(), "git.status", ToolSourceKindV1::BuiltIn, ran).summary,
            view().summary
        );
        assert_eq!(
            decorate(view(), "git.commit", ToolSourceKindV1::Mcp, ran).summary,
            view().summary
        );
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
        summarize(
            &mut v,
            "git.push",
            &serde_json::json!({"local_branch":"main","remote":"origin","remote_branch":"main",
                "refs":[{}]}),
        );
        assert_eq!(v.summary, "推送 main → origin/main（1 个 ref 已核对）");
        summarize(
            &mut v,
            "git.sync",
            &serde_json::json!({"kind":"pull","remote":"origin","refs":[{}]}),
        );
        assert_eq!(v.summary, "快进同步 origin（1 个 ref 已更新）");
    }
}
