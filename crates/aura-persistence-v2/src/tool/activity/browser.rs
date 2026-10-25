//! How the built-in browser's calls read in the conversation — what was done, on which page,
//! what the page reported, and the screenshot a call took (shown from its own record).
use aura_tool_projection::AgentToolDisplay;
use serde_json::Value;

fn clipped(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let mut result: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        result.push('…');
    }
    result
}

pub(super) fn title(name: &str) -> Option<&'static str> {
    Some(match name {
        "browser.open" => "打开网页",
        "browser.navigate" => "网页前进后退",
        "browser.snapshot" => "读取网页",
        "browser.screenshot" => "网页截图",
        "browser.click" => "点击网页",
        "browser.type" => "网页输入",
        "browser.press" => "网页按键",
        "browser.scroll" => "滚动网页",
        "browser.hover" => "悬停",
        "browser.select" => "选择选项",
        "browser.wait" => "等待网页",
        "browser.console" => "网页控制台",
        "browser.close" => "关闭网页",
        "browser.request_user" => "请你在网页上操作",
        "browser.evaluate" => "网页脚本",
        "browser.network" => "网页请求记录",
        "browser.inspect" => "检查元素",
        "browser.performance" => "网页性能",
        _ => return None,
    })
}

/// The address or file a call opens, before it has a result.
pub(super) fn target(name: &str, arguments: Option<&Value>) -> Option<String> {
    let arguments = arguments?;
    (name == "browser.open")
        .then(|| arguments["url"].as_str().or_else(|| arguments["path"].as_str()))
        .flatten()
        .map(|value| clipped(value, 256))
}

/// What a call's own input adds to its expanded row.
pub(super) fn details(name: &str, arguments: Option<&Value>) -> Vec<String> {
    let Some(arguments) = arguments else { return vec![] };
    let line = match name {
        "browser.type" => {
            arguments["text"].as_str().map(|text| format!("输入：{}", clipped(text, 200)))
        }
        "browser.press" => arguments["key"].as_str().map(|key| format!("按键：{key}")),
        "browser.evaluate" => {
            arguments["expression"].as_str().map(|text| format!("脚本：{}", clipped(text, 400)))
        }
        "browser.inspect" => {
            arguments["element"].as_str().map(|element| format!("元素：{element}"))
        }
        "browser.request_user" => {
            arguments["reason"].as_str().map(|text| format!("请求：{}", clipped(text, 300)))
        }
        _ => None,
    };
    line.into_iter().collect()
}

/// A page result's facts: its title, what stopped or changed the call, dialogs, console errors.
fn page(view: &mut AgentToolDisplay, value: &Value) {
    if let Some(title) = value["title"].as_str().filter(|title| !title.is_empty()) {
        view.summary = clipped(title, 80);
    }
    if let Some(notice) = value["notice"].as_str() {
        view.details.push(clipped(notice, 300));
    }
    for dialog in value["dialogs"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        view.details.push(format!("弹框 {}", clipped(dialog, 200)));
    }
    let errors = value["console"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| {
            matches!(entry["level"].as_str(), Some("error" | "exception" | "failed_load"))
        })
        .filter_map(|entry| entry["text"].as_str())
        .collect::<Vec<_>>();
    for text in errors.iter().take(3) {
        view.details.push(format!("页面报错 {}", clipped(text, 200)));
    }
    view.truncated |= errors.len() > 3 || value["snapshot_truncated"] == true;
}

pub(super) fn summarize(view: &mut AgentToolDisplay, name: &str, value: &Value) {
    let url = value["url"].as_str().or_else(|| value["page"]["url"].as_str());
    if let Some(url) = url.filter(|url| !url.is_empty() && *url != "about:blank") {
        view.target = Some(clipped(url, 256));
    }
    match name {
        "browser.screenshot" => {
            let size = format!(
                "{}×{}",
                value["width"].as_u64().unwrap_or(0),
                value["height"].as_u64().unwrap_or(0)
            );
            view.summary = if value["shown_to_model"] == true {
                format!("截图 {size} · 已发给模型")
            } else {
                format!("截图 {size} · 模型未收到图片")
            };
            view.image = value["image_artifact_id"].as_str().and_then(|id| id.parse().ok());
        }
        "browser.request_user" => {
            view.summary = match value["outcome"].as_str() {
                Some("done") => "你已完成",
                Some("declined") => "你已取消",
                Some("timed_out") => "等待已超时",
                Some("closed") => "网页已关闭",
                _ => "已结束",
            }
            .into();
        }
        "browser.evaluate" => match value["exception"].as_str() {
            Some(exception) => {
                view.summary = "脚本出错".into();
                view.details.push(clipped(exception, 400));
            }
            None => {
                view.summary = "脚本已运行".into();
                if let Some(result) = value["value"].as_str() {
                    view.details.push(format!("结果：{}", clipped(result, 400)));
                }
            }
        },
        "browser.network" | "browser.console" => {
            let entries = value["entries"].as_array().cloned().unwrap_or_default();
            view.summary = format!(
                "{} 条{}",
                entries.len(),
                if name == "browser.network" { "请求" } else { "记录" }
            );
            view.details = entries
                .iter()
                .take(5)
                .map(|entry| match name {
                    "browser.network" => format!(
                        "{} {} {}",
                        entry["method"].as_str().unwrap_or(""),
                        entry["status"].as_u64().map_or_else(
                            || entry["failure"].as_str().unwrap_or("").to_owned(),
                            |s| s.to_string()
                        ),
                        clipped(entry["url"].as_str().unwrap_or(""), 160)
                    ),
                    _ => format!(
                        "{} {}",
                        entry["level"].as_str().unwrap_or(""),
                        clipped(entry["text"].as_str().unwrap_or(""), 200)
                    ),
                })
                .collect();
            view.truncated = entries.len() > 5 || value["truncated"] == true;
        }
        "browser.close" => view.summary = "网页已关闭".into(),
        _ => page(view, value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn a_screenshot_shows_its_page_its_size_and_whether_the_model_saw_it() {
        let mut shot = view();
        let id = aura_domain_common::ArtifactId::new();
        summarize(
            &mut shot,
            "browser.screenshot",
            &json!({"url":"http://127.0.0.1:9/game","title":"Game","width":1280,"height":800,
                "image_artifact_id":id,"shown_to_model":true,"note":""}),
        );
        assert_eq!(shot.target.as_deref(), Some("http://127.0.0.1:9/game"));
        assert_eq!(shot.summary, "截图 1280×800 · 已发给模型");
        assert_eq!(shot.image, Some(id));
    }

    #[test]
    fn a_page_result_shows_its_title_and_what_the_page_reported() {
        let mut clicked = view();
        summarize(
            &mut clicked,
            "browser.click",
            &json!({"url":"http://127.0.0.1:9/game","title":"Minesweeper","snapshot":"",
                "snapshot_truncated":false,"notice":null,
                "dialogs":["alert: Game over（Aura 已确认）"],
                "console":[{"level":"log","text":"tick","source":null},
                    {"level":"exception","text":"TypeError: x is undefined","source":null}]}),
        );
        assert_eq!(clicked.summary, "Minesweeper");
        assert_eq!(
            clicked.details,
            ["弹框 alert: Game over（Aura 已确认）", "页面报错 TypeError: x is undefined"]
        );
        let mut asked = view();
        summarize(&mut asked, "browser.request_user", &json!({"outcome":"declined","page":null}));
        assert_eq!(asked.summary, "你已取消");
        assert_eq!(
            details("browser.request_user", Some(&json!({"reason":"请登录"}))),
            ["请求：请登录"]
        );
        assert_eq!(title("browser.evaluate"), Some("网页脚本"));
    }
}
