//! What a Chrome call touches, decided by Core before the permission engine sees it.
//!
//! A call acts on one share of this conversation and names the tab's site; a site confirmed
//! neither for the share nor for the conversation is asked about, and Chrome's own pages are never
//! touched. `chrome.open` on a site the conversation already has a tab of uses that tab again.
//! What an input action would touch is inspected in the tab before admission, and what the first
//! resolution read is kept by call, so every later resolution of the call reads the same; the
//! extension checks again as it acts.
use super::super::*;
use crate::chrome_control::{CallSites, ResolvedTab};
use anyhow::{anyhow, bail, ensure};
use aura_chrome_domain::commands::{ChromeCommand, ChromeIntent};
use aura_chrome_domain::*;
use aura_tool_registry::agent_release::AgentBuiltinHandler as H;
use serde_json::Value;
use std::time::Duration;

/// Core's refusal of a Chrome call, said to the model in Core's words.
pub(in crate::agent_execution) fn admission_feedback(error: &anyhow::Error) -> Option<String> {
    let text = error.to_string();
    text.starts_with("chrome.").then_some(text)
}

pub(crate) struct ChromeResolution {
    pub resources: Vec<ResourceDescriptor>,
}

const NOT_CONNECTED: &str = "chrome.not_connected: 用户的 Chrome 现在没有连接 Aura。请用户在 Chrome 里打开 Aura 侧边栏，确认已连接，并把要用的标签页共享给本会话。";

/// An http(s) address, as the user would type it.
pub(super) fn web_address(text: &str) -> anyhow::Result<String> {
    let trimmed = text.trim();
    let address =
        if trimmed.contains("://") { trimmed.to_owned() } else { format!("https://{trimmed}") };
    let parsed = url::Url::parse(&address)
        .map_err(|_| anyhow!("chrome.address: “{text}”不是网页地址；请给 http(s) 地址"))?;
    ensure!(
        matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some(),
        "chrome.address: “{text}”不是网页地址；请给 http(s) 地址"
    );
    ensure!(
        !aura_security_contract::chrome_origin_blocked(parsed.as_str()),
        "chrome.blocked: Chrome 应用商店和浏览器自己的页面不能由 Aura 打开或操作。"
    );
    Ok(parsed.to_string())
}

pub(super) fn element(text: Option<String>) -> anyhow::Result<Option<String>> {
    if let Some(text) = &text {
        let digits = text.strip_prefix('e').unwrap_or("");
        ensure!(
            !digits.is_empty() && digits.len() <= 8 && digits.bytes().all(|b| b.is_ascii_digit()),
            "chrome.ref: “{text}”不是元素引用；用最近一次 chrome.snapshot 里的 ref，例如 e12"
        );
    }
    Ok(text)
}

fn bounded(value: Option<u32>, default: u32, low: u32, high: u32) -> u32 {
    value.unwrap_or(default).clamp(low, high)
}

/// The input action a call is, when it is one, with the element it names (its `ref`).
fn intent(handler: H, input: &Value) -> Option<(ChromeIntent, Option<String>)> {
    let element = input["ref"].as_str().map(str::to_owned);
    Some(match handler {
        H::ChromeClick => (ChromeIntent::Click, element),
        H::ChromeType => (ChromeIntent::Type, element),
        H::ChromePress => (ChromeIntent::Press, None),
        H::ChromeSelect => (ChromeIntent::Select, element),
        H::ChromeHover => (ChromeIntent::Hover, element),
        H::ChromeScroll => (ChromeIntent::Scroll, element),
        _ => return None,
    })
}

fn handler_named(name: &str) -> Option<H> {
    Some(match name {
        "chrome.click" => H::ChromeClick,
        "chrome.type" => H::ChromeType,
        "chrome.press" => H::ChromePress,
        "chrome.select" => H::ChromeSelect,
        "chrome.hover" => H::ChromeHover,
        "chrome.scroll" => H::ChromeScroll,
        _ => return None,
    })
}

/// `chrome.tabs`: the conversation's shares from Core's own record.
pub(in crate::agent_execution) fn tabs_result(
    service: &AgentExecution,
    session: SessionId,
) -> anyhow::Result<Value> {
    let tabs = service.chrome.get().map(|chrome| chrome.tabs(session)).unwrap_or_default();
    Ok(serde_json::to_value(ChromeTabsResult { tabs })?)
}

/// Looks, in the tab, at what each of the step's input actions would touch, before admission
/// decides on them. A call whose tab cannot be inspected is left alone: admission refuses it.
pub(crate) async fn inspect_before_admission(
    service: &AgentExecution,
    session: SessionId,
    calls: Vec<aura_persistence_v2::hooks::PendingToolCall>,
) {
    let Some(chrome) = service.chrome.get() else { return };
    for call in calls {
        let Some(handler) = handler_named(&call.tool_name) else { continue };
        let Some((intent, element)) = intent(handler, &call.arguments) else { continue };
        let Ok(tab) = chrome.resolve_tab(session, call.arguments["tab"].as_str()) else { continue };
        let (x, y) = (call.arguments["x"].as_f64(), call.arguments["y"].as_f64());
        let point = (intent == ChromeIntent::Click && element.is_none()).then_some((x, y));
        let (x, y) = point.unwrap_or((None, None));
        let command = ChromeCommand::Inspect { tab_id: tab.tab_id, element, x, y, intent };
        let Ok(command) = serde_json::to_value(command) else { continue };
        if let Ok(found) = chrome.request(command, None, Duration::from_secs(5)).await
            && let Ok(inspection) = serde_json::from_value(found)
        {
            chrome.remember_inspection(call.tool_call_id, inspection);
        }
    }
}

impl AgentExecution {
    pub(in crate::agent_execution) fn resolve_chrome(
        &self,
        handler: H,
        tool_call_id: aura_domain_common::ToolCallId,
        action: &mut Value,
        capabilities: &mut Vec<ResolvedCapability>,
        snapshot: &PermissionSnapshot,
    ) -> anyhow::Result<ChromeResolution> {
        let chrome = self.chrome.get().ok_or_else(|| anyhow!(NOT_CONNECTED))?;
        ensure!(chrome.connected(), NOT_CONNECTED);
        let session = snapshot.session_id;
        let input = action.clone();
        let opening = if handler == H::ChromeOpen {
            let open: ChromeOpenInput = serde_json::from_value(input.clone())?;
            let url = web_address(&open.url)?;
            Some((crate::chrome_control::shares::origin_of(&url), url))
        } else {
            None
        };
        let sites = match chrome.pinned(tool_call_id) {
            Some(sites) => sites,
            None => {
                let confirmed = aura_persistence_v2::chrome_sites::confirmed(
                    self.journal.lock()?.database.connection(),
                    session,
                )?;
                chrome.set_session_sites(session, confirmed.clone());
                let reuse = opening.as_ref().and_then(|(origin, url)| {
                    chrome.opened_on(session, origin).map(|tab| (tab.tab_id, tab.url == *url))
                });
                let sites = CallSites { session: confirmed, reuse };
                chrome.pin(tool_call_id, sites.clone());
                sites
            }
        };
        // A site is confirmed for the call when the tab's share or the conversation confirmed it.
        let confirmed = |site: &str, tab: Option<&ResolvedTab>| {
            tab.is_some_and(|tab| tab.origin == site && tab.origin_confirmed)
                || sites.session.contains(site)
        };
        if let Some((origin, url)) = opening {
            let Some((tab_id, same_address)) = sites.reuse else {
                *action = serde_json::to_value(ChromeCommand::Open { url })?;
                return Ok(ChromeResolution {
                    resources: vec![
                        ResourceDescriptor::ChromeShare { share_id: format!("open:{session}") },
                        ResourceDescriptor::ChromeTab { tab_ref: "new".into() },
                        ResourceDescriptor::ChromeOrigin {
                            confirmed: confirmed(&origin, None),
                            origin,
                        },
                    ],
                });
            };
            // One tab per site: the conversation's tab Aura opened there goes to the address in
            // the background, or stays when it is there already.
            let tab = chrome.session_tab(session, tab_id).ok_or_else(|| {
                anyhow!("chrome.no_tab: 本会话在这个站点的标签页已经关闭；请重试 chrome.open。")
            })?;
            let command = if same_address {
                ChromeCommand::Snapshot { tab_id, max_chars: 12_000 }
            } else {
                ChromeCommand::Navigate { tab_id, url: Some(url), action: None }
            };
            *action = serde_json::to_value(command)?;
            return Ok(ChromeResolution {
                resources: vec![
                    ResourceDescriptor::ChromeShare { share_id: tab.share_id.clone() },
                    ResourceDescriptor::ChromeTab { tab_ref: tab.reference.clone() },
                    ResourceDescriptor::ChromeOrigin {
                        confirmed: confirmed(&origin, Some(&tab)),
                        origin,
                    },
                ],
            });
        }
        let tab = chrome
            .resolve_tab(session, input["tab"].as_str())
            .map_err(|message| anyhow!(message))?;
        ensure!(
            !aura_security_contract::chrome_origin_blocked(&tab.url),
            "chrome.blocked: 这个标签页现在是 Chrome 自己的页面，Aura 不操作它。"
        );
        let mut origin = (tab.origin.clone(), confirmed(&tab.origin, Some(&tab)));
        let tab_id = tab.tab_id;
        let command = match handler {
            H::ChromeNavigate => {
                let input: ChromeNavigateInput = serde_json::from_value(input)?;
                let url = match (&input.url, input.action) {
                    (Some(url), None) => Some(web_address(url)?),
                    (None, Some(_)) => None,
                    _ => bail!("chrome.navigate: 给出 url 或 action 中的一个"),
                };
                if let Some(url) = &url {
                    let target = crate::chrome_control::shares::origin_of(url);
                    origin = (target.clone(), confirmed(&target, Some(&tab)));
                }
                ChromeCommand::Navigate { tab_id, url, action: input.action }
            }
            H::ChromeSnapshot => {
                let input: ChromeSnapshotInput = serde_json::from_value(input)?;
                ChromeCommand::Snapshot {
                    tab_id,
                    max_chars: bounded(input.max_chars, 12_000, 2_000, 60_000),
                }
            }
            H::ChromeScreenshot => {
                let input: ChromeScreenshotInput = serde_json::from_value(input)?;
                ChromeCommand::Screenshot {
                    tab_id,
                    element: element(input.element)?,
                    full_page: input.full_page,
                }
            }
            H::ChromeWait => {
                let input: ChromeWaitInput = serde_json::from_value(input)?;
                ChromeCommand::Wait {
                    tab_id,
                    text: input.text,
                    text_gone: input.text_gone,
                    time_ms: input.time_ms.map(|ms| ms.min(60_000)),
                    timeout_ms: bounded(input.timeout_ms, 10_000, 100, 60_000),
                }
            }
            H::ChromeConsole => {
                let input: ChromeConsoleInput = serde_json::from_value(input)?;
                ChromeCommand::Console {
                    tab_id,
                    level: input.level,
                    max_entries: bounded(input.max_entries, 50, 1, 500),
                }
            }
            H::ChromeClose => {
                ensure!(
                    tab.opened_by_aura,
                    "chrome.not_opened_by_aura: {} 是用户共享的标签页，只有用户能停止共享；chrome.close 只关闭 chrome.open 打开的标签页。",
                    tab.reference
                );
                ChromeCommand::Close { tab_id }
            }
            H::ChromeRequestUser => {
                let input: ChromeRequestUserInput = serde_json::from_value(input)?;
                ensure!(!input.reason.trim().is_empty(), "chrome.request_user: 说明要用户做什么");
                ChromeCommand::Assist {
                    tab_id,
                    reason: input.reason.chars().take(500).collect(),
                    timeout_seconds: bounded(input.timeout_seconds, 300, 10, 1_800),
                }
            }
            _ => {
                let inspection = chrome.inspection(tool_call_id).ok_or_else(|| {
                    anyhow!(
                        "chrome.not_inspected: Chrome 没有及时回应，这一步没有执行；请稍后重试。"
                    )
                })?;
                if let Some(refused) = inspection.refused {
                    bail!(
                        "chrome.refused: {refused}。这类内容只能由用户自己操作：用 chrome.request_user 把标签页交给用户。"
                    );
                }
                if inspection.sensitive
                    && !capabilities.contains(&ResolvedCapability::ChromeSensitiveAction)
                {
                    capabilities.push(ResolvedCapability::ChromeSensitiveAction);
                }
                origin = (inspection.origin.clone(), confirmed(&inspection.origin, Some(&tab)));
                input_command(handler, tab_id, input)?
            }
        };
        *action = serde_json::to_value(command)?;
        Ok(ChromeResolution {
            resources: vec![
                ResourceDescriptor::ChromeShare { share_id: tab.share_id },
                ResourceDescriptor::ChromeTab { tab_ref: tab.reference },
                ResourceDescriptor::ChromeOrigin { origin: origin.0, confirmed: origin.1 },
            ],
        })
    }
}

fn input_command(handler: H, tab_id: u32, input: Value) -> anyhow::Result<ChromeCommand> {
    Ok(match handler {
        H::ChromeClick => {
            let input: ChromeClickInput = serde_json::from_value(input)?;
            if let (Some(x), Some(y)) = (input.x, input.y) {
                ensure!(
                    x.is_finite()
                        && y.is_finite()
                        && (0.0..=20_000.0).contains(&x)
                        && (0.0..=20_000.0).contains(&y),
                    "chrome.click: 坐标要在视口内"
                );
            }
            ensure!(
                input.element.is_some() || (input.x.is_some() && input.y.is_some()),
                "chrome.click: 给出元素 ref，或 x 与 y"
            );
            ChromeCommand::Click {
                tab_id,
                element: element(input.element)?,
                x: input.x,
                y: input.y,
                button: input.button,
                double: input.double,
            }
        }
        H::ChromeType => {
            let input: ChromeTypeInput = serde_json::from_value(input)?;
            ensure!(input.text.chars().count() <= 10_000, "chrome.type: 一次最多输入 10000 个字符");
            ChromeCommand::Type {
                tab_id,
                element: element(input.element)?,
                text: input.text,
                clear: input.clear,
                submit: input.submit,
            }
        }
        H::ChromePress => {
            let input: ChromePressInput = serde_json::from_value(input)?;
            ChromeCommand::Press {
                tab_id,
                key: input.key,
                repeat: bounded(input.repeat, 1, 1, 50),
                hold_ms: bounded(input.hold_ms, 0, 0, 5_000),
            }
        }
        H::ChromeScroll => {
            let input: ChromeScrollInput = serde_json::from_value(input)?;
            ChromeCommand::Scroll {
                tab_id,
                direction: input.direction,
                element: element(input.element)?,
                amount: bounded(input.amount, 1, 1, 20),
            }
        }
        H::ChromeHover => {
            let input: ChromeHoverInput = serde_json::from_value(input)?;
            ChromeCommand::Hover {
                tab_id,
                element: element(Some(input.element))?.unwrap_or_default(),
            }
        }
        H::ChromeSelect => {
            let input: ChromeSelectInput = serde_json::from_value(input)?;
            ensure!(!input.values.is_empty(), "chrome.select: 至少给一个选项值");
            ChromeCommand::Select {
                tab_id,
                element: element(Some(input.element))?.unwrap_or_default(),
                values: input.values,
            }
        }
        _ => bail!("chrome.unsupported_tool"),
    })
}

/// How long Core waits for the extension; the user's own time for `chrome.request_user`.
pub(crate) fn timeout(command: &ChromeCommand) -> Duration {
    Duration::from_millis(match command {
        ChromeCommand::Open { .. } | ChromeCommand::Navigate { .. } => 45_000,
        ChromeCommand::Wait { timeout_ms, time_ms, .. } => {
            u64::from((*timeout_ms).max(time_ms.unwrap_or(0))) + 5_000
        }
        ChromeCommand::Press { repeat, hold_ms, .. } => 20_000 + u64::from(repeat * hold_ms),
        ChromeCommand::Assist { timeout_seconds, .. } => {
            u64::from(*timeout_seconds) * 1_000 + 15_000
        }
        _ => 20_000,
    })
}
