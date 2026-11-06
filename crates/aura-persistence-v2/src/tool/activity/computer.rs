//! How the programs on the user's desktop read in the conversation: what was done to which
//! program, what a read found, and the screenshot a call took (shown from its own record).
use aura_tool_contract::ToolResultOutcomeV1;
use aura_tool_projection::AgentToolDisplay;
use serde_json::Value;

/// The code of a refusal that changed nothing. A computer refusal's message starts with it
/// (`computer.taken_over: …`), from the desktop and from admission alike, which refuses such a
/// call as a resource it could not resolve.
fn refusal_message(outcome: &ToolResultOutcomeV1) -> Option<&str> {
    let (ToolResultOutcomeV1::OperationRejected { error }
    | ToolResultOutcomeV1::NotExecuted { error: Some(error), .. }) = outcome
    else {
        return None;
    };
    Some(&error.message)
}

fn refusal_code(outcome: &ToolResultOutcomeV1) -> Option<&str> {
    Some(refusal_message(outcome)?.split_once(':')?.0)
}

/// What the user did to take the desktop over, as the refusal names it: `用户已接管电脑（…）`.
fn takeover_cause(message: &str) -> Option<String> {
    let (_, rest) = message.split_once("用户已接管电脑（")?;
    let (cause, _) = rest.split_once('）')?;
    (!cause.is_empty()).then(|| cause.chars().take(40).collect())
}

/// The short line of a call that changed nothing because of the user: they took the desktop
/// over (and with what, when known), another conversation keeps it, or they stopped the program.
pub(super) fn refusal(outcome: &ToolResultOutcomeV1) -> Option<String> {
    Some(match refusal_code(outcome)? {
        "computer.taken_over" => match refusal_message(outcome).and_then(takeover_cause) {
            Some(cause) => format!("未执行：你已接管电脑（{cause}）"),
            None => "未执行：你已接管电脑".into(),
        },
        "computer.busy" => "未执行：另一段对话正在操控电脑".into(),
        "computer.stopped" => "未执行：你已停止操控这个程序".into(),
        _ => return None,
    })
}

pub(super) fn title(name: &str) -> Option<&'static str> {
    Some(match name {
        "computer.apps" => "电脑上的程序",
        "computer.open" => "打开程序",
        "computer.snapshot" => "读取程序窗口",
        "computer.screenshot" => "截取屏幕",
        "computer.act" => "操作程序",
        "computer.input" => "用鼠标键盘操作程序",
        "computer.window" => "调整程序窗口",
        _ => return None,
    })
}

pub(super) fn target(arguments: Option<&Value>) -> Option<String> {
    arguments?["app"]
        .as_str()
        .filter(|app| !app.is_empty())
        .map(|app| app.chars().take(256).collect())
}

/// The screenshot a result carries, as its card shows it.
fn screenshot(view: &mut AgentToolDisplay, shot: &Value) -> Option<String> {
    let id = shot["image_artifact_id"].as_str().and_then(|id| id.parse().ok())?;
    view.image = Some(id);
    let size =
        format!("{}×{}", shot["width"].as_u64().unwrap_or(0), shot["height"].as_u64().unwrap_or(0));
    Some(if shot["shown_to_model"] == true {
        format!("截图 {size} · 已发给模型")
    } else {
        format!("截图 {size} · 模型未收到图片")
    })
}

pub(super) fn summarize(view: &mut AgentToolDisplay, name: &str, value: &Value) {
    let count = |field: &str| value[field].as_u64().unwrap_or(0);
    let done = |view: &mut AgentToolDisplay| {
        let mut summary = format!("做了 {} 步", count("steps_done"));
        if let Some(shot) = screenshot(view, &value["screenshot"]) {
            summary = format!("{summary} · {shot}");
        }
        summary
    };
    view.summary = match name {
        "computer.apps" => {
            let apps = value["apps"].as_array().cloned().unwrap_or_default();
            view.details = apps
                .iter()
                .map(|app| {
                    let mut line = app["name"].as_str().unwrap_or("").to_owned();
                    if app["running"] == true {
                        line.push_str(" · 运行中");
                    }
                    if app["allowed"] == true {
                        line.push_str(" · 已允许");
                    }
                    line
                })
                .collect();
            match value["notice"].as_str() {
                Some(notice) if apps.is_empty() => notice.to_owned(),
                _ => format!("{} 个程序", apps.len() as u64 + count("more")),
            }
        }
        "computer.open" => {
            let windows = value["windows"].as_array().map_or(0, Vec::len);
            if value["already_running"] == true {
                format!("已在运行 · {windows} 个窗口")
            } else {
                format!("已打开 · {windows} 个窗口")
            }
        }
        "computer.snapshot" => {
            let read = format!("{} 个元素", count("listed"));
            let title = value["title"].as_str().filter(|title| !title.is_empty());
            view.target = title.map(str::to_owned).or(view.target.take());
            if value["complete"] == true { read } else { format!("{read} · 没有读完") }
        }
        "computer.screenshot" => screenshot(view, value).unwrap_or_else(|| "已截图".into()),
        "computer.act" | "computer.input" => done(view),
        "computer.window" => match value["action"].as_str() {
            _ if value["changed"] == false => "本来就是这样".into(),
            Some("front") => "已拿到前面".into(),
            Some("minimize") => "已最小化".into(),
            Some("maximize") => "已最大化".into(),
            Some("restore") => "已还原".into(),
            Some("close") if value["note"].is_string() => "窗口还开着".into(),
            Some("close") => "已关闭".into(),
            _ => return,
        },
        _ => return,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn takeover_during_an_action_does_not_claim_no_effect() {
        let error = aura_domain_common::ErrorEnvelope::new(
            aura_domain_common::ErrorCode::new("computer.taken_over").expect("code"),
            aura_domain_common::ErrorCategory::Unavailable,
            "computer.taken_over: 用户已接管小窗，这一步可能执行了部分",
            false,
            aura_domain_common::CorrelationId::new(),
            None,
        );
        let outcome = ToolResultOutcomeV1::Indeterminate {
            reason: aura_tool_contract::ToolIndeterminateReasonV1::DispatchOutcomeUnknown,
            error,
        };
        assert_eq!(refusal(&outcome), None, "unknown effects must not become 未执行");
    }

    fn view() -> AgentToolDisplay {
        AgentToolDisplay {
            title: String::new(),
            target: None,
            summary: "已完成".into(),
            details: vec![],
            truncated: false,
            internal: false,
            image: None,
            offer: None,
        }
    }

    #[test]
    fn every_computer_tool_has_its_words() {
        let release = aura_tool_registry::agent_release::release().expect("release");
        for entry in &release.manifest().entries {
            let name = entry.handler.name();
            if name.starts_with("computer.") {
                assert!(title(name).is_some(), "{name}");
            }
        }
    }

    #[test]
    fn a_read_an_act_and_a_screenshot_say_what_they_found() {
        let mut read = view();
        summarize(
            &mut read,
            "computer.snapshot",
            &json!({"app":"notepad","window":"w1","title":"无标题 - 记事本","elements":"",
                "listed":12,"complete":false}),
        );
        assert_eq!(
            (read.summary.as_str(), read.target.as_deref()),
            ("12 个元素 · 没有读完", Some("无标题 - 记事本"))
        );
        let id = aura_domain_common::ArtifactId::new();
        let mut acted = view();
        summarize(
            &mut acted,
            "computer.act",
            &json!({"app":"notepad","window":"w1","steps_done":2,"after":"",
                "screenshot":{"image_artifact_id":id,"width":800,"height":600,"shown_to_model":true}}),
        );
        assert_eq!(acted.summary, "做了 2 步 · 截图 800×600 · 已发给模型");
        assert_eq!(acted.image, Some(id));
        for (result, said) in [
            (json!({"action":"minimize","changed":true}), "已最小化"),
            (json!({"action":"minimize","changed":false}), "本来就是这样"),
            (json!({"action":"close","changed":true}), "已关闭"),
            (json!({"action":"close","changed":true,"note":"窗口还开着"}), "窗口还开着"),
        ] {
            let mut arranged = view();
            summarize(&mut arranged, "computer.window", &result);
            assert_eq!(arranged.summary, said, "{result}");
        }
        let mut listed = view();
        summarize(
            &mut listed,
            "computer.apps",
            &json!({"apps":[{"app":"a","name":"记事本","kind":"packaged","running":true,"allowed":true}],"more":3}),
        );
        assert_eq!(
            (listed.summary.as_str(), listed.details.as_slice()),
            ("4 个程序", ["记事本 · 运行中 · 已允许".to_owned()].as_slice())
        );
    }
}
