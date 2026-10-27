//! How the user's Chrome reads in the conversation. A Chrome result has the built-in browser's
//! shape, so its facts - the page, a screenshot, the user's answer - read the same way; only the
//! words for what was done say Chrome.
use aura_tool_projection::AgentToolDisplay;
use serde_json::Value;

pub(super) fn title(name: &str) -> Option<&'static str> {
    Some(match name {
        "chrome.tabs" => "Chrome 标签页",
        "chrome.open" => "在 Chrome 打开网页",
        "chrome.navigate" => "Chrome 标签页跳转",
        "chrome.snapshot" => "读取 Chrome 标签页",
        "chrome.screenshot" => "Chrome 截图",
        "chrome.click" => "在 Chrome 中点击",
        "chrome.type" => "在 Chrome 中输入",
        "chrome.press" => "在 Chrome 中按键",
        "chrome.scroll" => "滚动 Chrome 标签页",
        "chrome.hover" => "在 Chrome 中悬停",
        "chrome.select" => "在 Chrome 中选择",
        "chrome.wait" => "等待 Chrome 标签页",
        "chrome.console" => "Chrome 控制台",
        "chrome.close" => "关闭 Chrome 标签页",
        "chrome.request_user" => "请你在 Chrome 中操作",
        _ => return None,
    })
}

/// The built-in browser's tool with the same result.
fn browser_name(name: &str) -> String {
    name.replacen("chrome.", "browser.", 1)
}

pub(super) fn target(name: &str, arguments: Option<&Value>) -> Option<String> {
    let tab = arguments.and_then(|value| value["tab"].as_str()).map(str::to_owned);
    super::browser::target(&browser_name(name), arguments).or(tab)
}

pub(super) fn details(name: &str, arguments: Option<&Value>) -> Vec<String> {
    super::browser::details(&browser_name(name), arguments)
}

pub(super) fn summarize(view: &mut AgentToolDisplay, name: &str, value: &Value) {
    if name == "chrome.tabs" {
        let tabs = value["tabs"].as_array().cloned().unwrap_or_default();
        view.summary = format!("{} 个共享的标签页", tabs.len());
        view.details = tabs
            .iter()
            .map(|tab| {
                format!(
                    "{} {}",
                    tab["tab"].as_str().unwrap_or(""),
                    tab["title"]
                        .as_str()
                        .filter(|title| !title.is_empty())
                        .unwrap_or(tab["url"].as_str().unwrap_or(""),)
                )
            })
            .collect();
        return;
    }
    super::browser::summarize(view, &browser_name(name), value);
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
    fn every_chrome_tool_has_its_words() {
        let release = aura_tool_registry::agent_release::release().expect("release");
        for entry in &release.manifest().entries {
            let name = entry.handler.name();
            if name.starts_with("chrome.") {
                assert!(title(name).is_some(), "{name}");
            }
        }
    }

    #[test]
    fn a_screenshot_and_a_handover_read_like_the_browsers() {
        let mut shot = view();
        let id = aura_domain_common::ArtifactId::new();
        summarize(
            &mut shot,
            "chrome.screenshot",
            &json!({"tab":"t1","url":"https://shop.example/cart","title":"Cart","width":800,
                "height":600,"image_artifact_id":id,"shown_to_model":false,"note":""}),
        );
        assert_eq!(shot.target.as_deref(), Some("https://shop.example/cart"));
        assert_eq!(shot.summary, "截图 800×600 · 模型未收到图片");
        assert_eq!(shot.image, Some(id));
        let mut handed = view();
        summarize(&mut handed, "chrome.request_user", &json!({"outcome":"done","page":null}));
        assert_eq!(handed.summary, "你已完成");
    }

    #[test]
    fn the_shared_tabs_are_listed_by_reference() {
        let mut listed = view();
        summarize(
            &mut listed,
            "chrome.tabs",
            &json!({"tabs":[{"tab":"t1","title":"Cart","url":"https://shop.example/cart",
                "current":true,"opened_by_aura":false},{"tab":"t2","title":"",
                "url":"https://pay.example/","current":false,"opened_by_aura":true}]}),
        );
        assert_eq!(listed.summary, "2 个共享的标签页");
        assert_eq!(listed.details, ["t1 Cart", "t2 https://pay.example/"]);
        assert_eq!(
            target("chrome.click", Some(&json!({"tab":"t2","ref":"e3"}))).as_deref(),
            Some("t2")
        );
        assert_eq!(
            target("chrome.open", Some(&json!({"url":"https://shop.example/"}))).as_deref(),
            Some("https://shop.example/")
        );
    }
}
