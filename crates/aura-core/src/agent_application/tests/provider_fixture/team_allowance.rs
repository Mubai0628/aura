//! [`TeamPlan::AllowanceCard`] and [`TeamPlan::AllowancePresets`]: a sub-agent in a worktree
//! deletes with commands that do not name their targets, and touches the path the user's message
//! names outside its workspace.
use super::own::marked;
use super::*;

/// The committed files the sub-agent's commands delete in its copy, by variable.
pub const ALLOWANCE_FIRST: &str = "one.txt";
pub const ALLOWANCE_SECOND: &str = "two.txt";
/// What the sub-agent writes outside its workspace when the settings let it.
pub const ALLOWANCE_WRITTEN: &str = "preset-write";

pub(super) fn main(body: &Value, calling: &impl Fn(&str, Value) -> Value) -> Value {
    match results(body) {
        0 => {
            let goal = format!(
                "清理 AURA_EXTERNAL_PATH={} AURA_COPIES={}",
                named_external_path(body),
                marked(body, "AURA_COPIES=")
            );
            calling(
                "agents.spawn",
                json!({"name":"清理","task":{"goal":goal,"done_when":"清理完"},"worktree":true}),
            )
        }
        1 => calling("agents.wait", json!({"agents":["A1"],"timeout_seconds":120})),
        _ => answer("收到。"),
    }
}

fn command(calling: &impl Fn(&str, Value) -> Value, script: String) -> Value {
    calling("shell.powershell", json!({ "script": script }))
}

/// Deletes by variable twice, then deletes the path outside by name.
pub(super) fn card(body: &Value, calling: &impl Fn(&str, Value) -> Value) -> Value {
    match results(body) {
        0 => command(calling, format!("$png = '{ALLOWANCE_FIRST}'; Remove-Item $png")),
        1 => command(calling, format!("$old = '{ALLOWANCE_SECOND}'; Remove-Item $old")),
        2 => command(calling, format!("Remove-Item '{}'", named_external_path(body))),
        _ => answer(SUMMARY),
    }
}

/// Deletes by variable, writes the path outside, then deletes it by name.
pub(super) fn presets(body: &Value, calling: &impl Fn(&str, Value) -> Value) -> Value {
    let outside = named_external_path(body);
    match results(body) {
        0 => command(calling, format!("$png = '{ALLOWANCE_FIRST}'; Remove-Item $png")),
        1 => command(calling, format!("Set-Content -Path '{outside}' -Value {ALLOWANCE_WRITTEN}")),
        2 => command(calling, format!("Remove-Item '{outside}'")),
        _ => answer(SUMMARY),
    }
}
