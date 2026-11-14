//! [`TeamPlan::ReviewAllow`], [`TeamPlan::ReviewAskUser`] and [`TeamPlan::DownloadPreset`]: a
//! sub-agent in a worktree deletes in its copy by variable, which its main agent answers first
//! when the settings let it, or downloads, which the settings answer.
use super::*;

/// The committed file the sub-agent deletes in its copy, by variable.
pub const REVIEWED_FILE: &str = "old.png";
/// The committed file it deletes next, by variable, under the main agent's allowance.
pub const REVIEWED_SECOND: &str = "older.png";
/// The reason the main agent gives with its answer.
pub const REVIEW_REASON: &str = "只删它自己工作副本里的旧截图";

/// The first question id the last tool result names: the one agents.wait gave to answer.
fn given_question(body: &Value) -> Option<String> {
    let text = body["messages"].as_array()?.iter().rev().find(|m| m["role"] == "tool")?["content"]
        .as_str()?
        .to_owned();
    let at = text.find("apr_")?;
    Some(text[at..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect())
}

/// Starts A1 in a worktree and waits; answers the question the wait gives with `answer_with`,
/// waits again, then answers the user.
pub(super) fn main(
    body: &Value,
    calling: &impl Fn(&str, Value) -> Value,
    answer_with: &str,
) -> Value {
    match results(body) {
        0 | 1 => super::allowance::main(body, calling),
        2 => match given_question(body) {
            Some(id) => calling(
                "agents.approve",
                json!({"agent":"A1","approval_id":id,"answer":answer_with,"reason":REVIEW_REASON}),
            ),
            None => answer("没有要代批的请求。"),
        },
        3 => calling("agents.wait", json!({"agents":["A1"],"timeout_seconds":120})),
        _ => answer("收到。"),
    }
}

/// Deletes in its copy by variable, then reports.
pub(super) fn deletes(body: &Value, calling: &impl Fn(&str, Value) -> Value) -> Value {
    match results(body) {
        0 => calling(
            "shell.powershell",
            json!({"script": format!("$png = '{REVIEWED_FILE}'; Remove-Item $png")}),
        ),
        _ => answer(SUMMARY),
    }
}

/// Deletes in its copy by variable twice, then reports.
pub(super) fn deletes_twice(body: &Value, calling: &impl Fn(&str, Value) -> Value) -> Value {
    match results(body) {
        0 => calling(
            "shell.powershell",
            json!({"script": format!("$png = '{REVIEWED_FILE}'; Remove-Item $png")}),
        ),
        1 => calling(
            "shell.powershell",
            json!({"script": format!("$old = '{REVIEWED_SECOND}'; Remove-Item $old")}),
        ),
        _ => answer(SUMMARY),
    }
}

/// Fetches in its copy, a download that reaches no network here: the repository has no remote.
pub(super) fn downloads(body: &Value, calling: &impl Fn(&str, Value) -> Value) -> Value {
    match results(body) {
        0 => calling("shell.powershell", json!({"script": "git fetch"})),
        _ => answer(SUMMARY),
    }
}
