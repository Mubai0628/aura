//! The team tools in the conversation: the main agent's `agents.*` and a sub-agent's report.
use super::display::clipped;
use serde_json::Value;

pub(super) const REPORT: &str = "agent.report";

/// The most of a report's first line its row shows.
const LINE_CHARS: usize = 120;

/// Whether `name` is one of the team tools.
pub(super) fn is_team(name: &str) -> bool {
    name.starts_with("agents.") || name == REPORT
}

/// A team tool in the words the 多 Agent page uses for it.
pub(super) fn title(name: &str) -> Option<&'static str> {
    Some(match name {
        REPORT => "交报告",
        "agents.spawn" => "派子 Agent",
        "agents.status" => "查看子 Agent",
        "agents.wait" => "等子 Agent 报告",
        "agents.send" => "给子 Agent 发消息",
        "agents.result" => "读子 Agent 结果",
        "agents.stop" => "停止子 Agent",
        "agents.merge" => "合回工作副本",
        "agents.discard" => "丢弃工作副本",
        "agents.approve" => "代批子 Agent 的请求",
        _ => return None,
    })
}

/// A report's status in words; for another call, the sub-agent it names: the name a new one is
/// given, the one written to, or the ones waited for.
pub(super) fn target(name: &str, arguments: Option<&Value>) -> Option<String> {
    let arguments = arguments?;
    let text = |key: &str| {
        arguments.get(key).and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty())
    };
    match name {
        REPORT => status_words(text("status")?).map(str::to_owned),
        "agents.spawn" => text("name").map(|name| clipped(name, 80)),
        "agents.wait" => {
            let agents: Vec<&str> = arguments
                .get("agents")?
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|agent| !agent.is_empty())
                .collect();
            (!agents.is_empty()).then(|| agents.join("、"))
        }
        _ => text("agent").map(str::to_owned),
    }
}

fn status_words(status: &str) -> Option<&'static str> {
    Some(match status {
        "progress" => "进展",
        "waiting" => "提问",
        "ready" => "完成",
        "blocked" => "受阻",
        "failed" => "没做成",
        _ => return None,
    })
}

/// The first line a handed-in report says, for its row.
pub(super) fn report_line(arguments: Option<&Value>) -> Option<String> {
    let summary = arguments?.get("summary")?.as_str()?;
    let line = summary.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(clipped(line, LINE_CHARS))
}
