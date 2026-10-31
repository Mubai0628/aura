//! What a browser call touches, decided by Core before the permission engine sees it.
//!
//! The browser must be on and its settings readable; a blocked site is refused here, before any
//! approval could be asked for it. The page's site is the one the host last reported; the host
//! checks again before it acts, so an approval never carries over to a page that has moved.
//! Page debugging (a script in the page, its network log) needs the user's own switch in the
//! settings, which no tool can change; a script on a site other than this computer's own pages
//! is a sensitive action, asked every time.
use super::super::*;
use anyhow::{anyhow, bail, ensure};
use aura_browser_domain::*;
use aura_tool_registry::agent_release::AgentBuiltinHandler as H;
use serde_json::{Value, json};

/// Core's refusal of a browser call, said to the model in Core's words.
pub(in crate::agent_execution) fn admission_feedback(error: &anyhow::Error) -> Option<String> {
    let text = error.to_string();
    text.starts_with("browser.").then_some(text)
}

/// The page every model call acts on: one per session.
pub(crate) const MODEL_PAGE: &str = "model";

pub(crate) struct BrowserResolution {
    pub resources: Vec<ResourceDescriptor>,
    pub network: OperationNetwork,
    pub risks: Vec<OperationRiskFlag>,
}

const DEBUGGING_OFF: &str = "browser.debugging_off: 网页调试没有开启。只有用户能在 设置 → 网络与浏览器 打开“允许模型调试网页”；在此之前请用 browser.snapshot、browser.console 和截图。";

fn element(text: Option<String>) -> anyhow::Result<Option<String>> {
    if let Some(text) = &text {
        let digits = text.strip_prefix('e').unwrap_or("");
        ensure!(
            !digits.is_empty() && digits.len() <= 8 && digits.bytes().all(|b| b.is_ascii_digit()),
            "browser.ref: “{text}”不是元素引用；用最近一次快照里的 ref，例如 e12"
        );
    }
    Ok(text)
}

fn required(text: String) -> anyhow::Result<String> {
    element(Some(text)).map(|value| value.unwrap_or_default())
}

fn bounded(value: Option<u32>, default: u32, low: u32, high: u32) -> u32 {
    value.unwrap_or(default).clamp(low, high)
}

impl AgentExecution {
    pub(in crate::agent_execution) fn resolve_browser(
        &self,
        handler: H,
        action: &mut Value,
        capabilities: &mut Vec<ResolvedCapability>,
        snapshot: &PermissionSnapshot,
    ) -> anyhow::Result<BrowserResolution> {
        use ResolvedCapability as C;
        let settings = self
            .browser
            .settings()
            .ok_or_else(|| anyhow!("browser.unavailable: 浏览器设置尚未就绪，请稍后重试。"))?;
        ensure!(
            settings.issue.is_none() && settings.settings.enabled,
            "browser.disabled: 内置浏览器已在设置中关闭（设置 → 网络与浏览器）。"
        );
        let allowed = settings.settings.allowed_rules();
        let blocked = settings.settings.blocked_rules();
        let session = snapshot.session_id;
        let page = ResourceDescriptor::BrowserPage { page_ref: format!("{session}/{MODEL_PAGE}") };
        let site = |site: &BrowserSiteV1| ResourceDescriptor::BrowserSite {
            host: site.host.clone(),
            allowed_by_rule: allowed
                .iter()
                .find(|rule| rule.matches(site))
                .map(ToString::to_string),
        };
        // The page as the host last reported it.
        let current = self.browser.page_url(session).as_deref().and_then(site_of_url);
        let mut on_page = vec![page.clone()];
        on_page.extend(current.as_ref().map(site));
        let input = action.clone();
        let page_action = |caps: &[C], network: OperationNetwork, host: BrowserHostActionV1| {
            (caps.to_vec(), on_page.clone(), network, host)
        };
        let mut risks = vec![];
        let (caps, resources, network, host) = match handler {
            H::BrowserOpen => {
                let open: BrowserOpenInput = serde_json::from_value(input.clone())?;
                match (open.url, open.path) {
                    (Some(url), None) => {
                        let address = browser_address(&url).ok_or_else(|| {
                            anyhow!("browser.open: “{url}”不是网页地址；请给 http(s) 地址，或 localhost:端口")
                        })?;
                        match site_of_url(&address) {
                            None => page_action(
                                &[C::BrowserRead],
                                OperationNetwork::None,
                                BrowserHostActionV1::Open { url: address },
                            ),
                            Some(target) => {
                                if BrowserSiteDecision::of(&target, &allowed, &blocked)
                                    == BrowserSiteDecision::Blocked
                                {
                                    bail!(
                                        "browser.blocked: {} 已在浏览器设置中被阻止，不能打开。",
                                        target.host
                                    );
                                }
                                (
                                    vec![C::BrowserRead],
                                    vec![
                                        page.clone(),
                                        site(&target),
                                        ResourceDescriptor::NetworkOrigin {
                                            scheme: target.scheme.clone(),
                                            host: target.host.clone(),
                                            port: target.port,
                                        },
                                    ],
                                    OperationNetwork::BrowserSitePolicy,
                                    BrowserHostActionV1::Open { url: address },
                                )
                            }
                        }
                    }
                    (None, Some(path)) => {
                        let missing = |error: &dyn std::fmt::Display| {
                            anyhow!(
                                "browser.open: 打不开工作区文件 {path}（{error}）；路径相对于工作区根目录"
                            )
                        };
                        let relative = WorkspacePath::parse(&path).map_err(|e| missing(&e))?;
                        let resolver = self.workspace.resolver(snapshot.workspace_id)?;
                        let resolved =
                            resolver.resolve_existing(&relative).map_err(|e| missing(&e))?;
                        ensure!(
                            !resolved.info.is_directory,
                            "browser.open: {path} 是文件夹；请给一个文件，例如 index.html"
                        );
                        let url =
                            url::Url::from_file_path(&resolved.absolute_path).map_err(|()| {
                                anyhow!("browser.open: 无法把 {path} 作为本地文件打开")
                            })?;
                        (
                            vec![C::BrowserRead, C::WorkspaceFileRead],
                            vec![
                                page.clone(),
                                ResourceDescriptor::WorkspacePath {
                                    workspace_id: snapshot.workspace_id,
                                    canonical_relative_path: relative,
                                    file_target: None,
                                },
                            ],
                            OperationNetwork::BrowserSitePolicy,
                            BrowserHostActionV1::Open { url: url.to_string() },
                        )
                    }
                    _ => bail!("browser.open: 给出 url 或 path 中的一个"),
                }
            }
            H::BrowserNavigate => {
                let input: BrowserNavigateInput = serde_json::from_value(input.clone())?;
                page_action(
                    &[C::BrowserRead],
                    OperationNetwork::BrowserSitePolicy,
                    BrowserHostActionV1::Navigate { action: input.action },
                )
            }
            H::BrowserSnapshot => {
                let input: BrowserSnapshotInput = serde_json::from_value(input.clone())?;
                page_action(
                    &[C::BrowserRead],
                    OperationNetwork::None,
                    BrowserHostActionV1::Snapshot {
                        max_chars: bounded(input.max_chars, 12_000, 2_000, 60_000),
                    },
                )
            }
            H::BrowserScreenshot => {
                let input: BrowserScreenshotInput = serde_json::from_value(input.clone())?;
                page_action(
                    &[C::BrowserRead],
                    OperationNetwork::None,
                    BrowserHostActionV1::Screenshot {
                        element: element(input.element)?,
                        full_page: input.full_page,
                    },
                )
            }
            H::BrowserClick => {
                let input: BrowserClickInput = serde_json::from_value(input.clone())?;
                let point = match (input.x, input.y) {
                    (Some(x), Some(y)) => {
                        ensure!(
                            x.is_finite()
                                && y.is_finite()
                                && (0.0..=20_000.0).contains(&x)
                                && (0.0..=20_000.0).contains(&y),
                            "browser.click: 坐标应在页面之内"
                        );
                        true
                    }
                    (None, None) => false,
                    _ => bail!("browser.click: x 与 y 要一起给"),
                };
                ensure!(
                    point != input.element.is_some(),
                    "browser.click: 给出 ref，或者 x 与 y，二选一"
                );
                page_action(
                    &[C::BrowserInteraction],
                    OperationNetwork::BrowserSitePolicy,
                    BrowserHostActionV1::Click {
                        element: element(input.element)?,
                        x: input.x,
                        y: input.y,
                        button: input.button,
                        double: input.double,
                    },
                )
            }
            H::BrowserType => {
                let input: BrowserTypeInput = serde_json::from_value(input.clone())?;
                ensure!(
                    input.text.chars().count() <= 20_000,
                    "browser.type: 一次最多输入 20000 个字符"
                );
                page_action(
                    &[C::BrowserInteraction],
                    OperationNetwork::BrowserSitePolicy,
                    BrowserHostActionV1::Type {
                        element: element(input.element)?,
                        text: input.text,
                        clear: input.clear,
                        submit: input.submit,
                    },
                )
            }
            H::BrowserPress => {
                let input: BrowserPressInput = serde_json::from_value(input.clone())?;
                let key = input.key.trim().to_owned();
                ensure!(
                    !key.is_empty()
                        && key.chars().count() <= 40
                        && !key.chars().any(char::is_control),
                    "browser.press: 写一个按键，例如 Enter、ArrowLeft、a 或 Control+a"
                );
                let repeat = bounded(input.repeat, 1, 1, 20);
                let hold_ms = bounded(input.hold_ms, 0, 0, 3_000);
                ensure!(repeat * hold_ms <= 10_000, "browser.press: 按住的总时长最多 10 秒");
                page_action(
                    &[C::BrowserInteraction],
                    OperationNetwork::BrowserSitePolicy,
                    BrowserHostActionV1::Press { key, repeat, hold_ms },
                )
            }
            H::BrowserScroll => {
                let input: BrowserScrollInput = serde_json::from_value(input.clone())?;
                page_action(
                    &[C::BrowserInteraction],
                    OperationNetwork::BrowserSitePolicy,
                    BrowserHostActionV1::Scroll {
                        element: element(input.element)?,
                        direction: input.direction,
                        amount: bounded(input.amount, 600, 1, 20_000),
                    },
                )
            }
            H::BrowserHover => {
                let input: BrowserHoverInput = serde_json::from_value(input.clone())?;
                page_action(
                    &[C::BrowserInteraction],
                    OperationNetwork::BrowserSitePolicy,
                    BrowserHostActionV1::Hover { element: required(input.element)? },
                )
            }
            H::BrowserSelect => {
                let input: BrowserSelectInput = serde_json::from_value(input.clone())?;
                ensure!(
                    (1..=50).contains(&input.values.len()),
                    "browser.select: 选 1 到 50 个选项"
                );
                page_action(
                    &[C::BrowserInteraction],
                    OperationNetwork::BrowserSitePolicy,
                    BrowserHostActionV1::Select {
                        element: required(input.element)?,
                        values: input.values,
                    },
                )
            }
            H::BrowserWait => {
                let input: BrowserWaitInput = serde_json::from_value(input.clone())?;
                ensure!(
                    input.text.is_some() || input.text_gone.is_some() || input.time_ms.is_some(),
                    "browser.wait: 给出 text、text_gone 或 time_ms"
                );
                page_action(
                    &[C::BrowserRead],
                    OperationNetwork::None,
                    BrowserHostActionV1::Wait {
                        text: input.text,
                        text_gone: input.text_gone,
                        time_ms: input.time_ms.map(|ms| ms.min(10_000)),
                        timeout_ms: bounded(input.timeout_ms, 10_000, 100, 30_000),
                    },
                )
            }
            H::BrowserConsole => {
                let input: BrowserConsoleInput = serde_json::from_value(input.clone())?;
                page_action(
                    &[C::BrowserRead],
                    OperationNetwork::None,
                    BrowserHostActionV1::Console {
                        level: input.level.unwrap_or_default(),
                        max_entries: bounded(input.max_entries, 50, 1, 200),
                    },
                )
            }
            H::BrowserClose => {
                let _: BrowserCloseInput = serde_json::from_value(input.clone())?;
                page_action(&[C::BrowserRead], OperationNetwork::None, BrowserHostActionV1::Close)
            }
            H::BrowserRequestUser => {
                let input: BrowserRequestUserInput = serde_json::from_value(input.clone())?;
                let reason = input.reason.trim().to_owned();
                ensure!(
                    !reason.is_empty() && reason.chars().count() <= 300,
                    "browser.request_user: 用一句话（最多 300 字）说明用户要在页面上做什么"
                );
                page_action(
                    &[C::BrowserRead],
                    OperationNetwork::None,
                    BrowserHostActionV1::RequestUser {
                        reason,
                        timeout_ms: bounded(input.timeout_seconds, 180, 30, 300) * 1_000,
                    },
                )
            }
            H::BrowserEvaluate => {
                ensure!(settings.settings.debugging, DEBUGGING_OFF);
                let input: BrowserEvaluateInput = serde_json::from_value(input.clone())?;
                ensure!(
                    !input.expression.trim().is_empty() && input.expression.len() <= 20_000,
                    "browser.evaluate: 给出一段 JavaScript 表达式（最多 20000 字节）"
                );
                let action = BrowserHostActionV1::Evaluate { expression: input.expression };
                // This computer's own pages (a dev server, a workspace file) are the model's to
                // debug; anywhere else a script reads and acts as the signed-in user.
                if current.as_ref().is_none_or(|site| this_computer(&site.host)) {
                    page_action(
                        &[C::BrowserInteraction],
                        OperationNetwork::BrowserSitePolicy,
                        action,
                    )
                } else {
                    risks.push(OperationRiskFlag::SensitiveBrowserAction);
                    page_action(
                        &[C::BrowserSensitiveAction],
                        OperationNetwork::BrowserSitePolicy,
                        action,
                    )
                }
            }
            H::BrowserNetwork => {
                ensure!(settings.settings.debugging, DEBUGGING_OFF);
                let input: BrowserNetworkInput = serde_json::from_value(input.clone())?;
                page_action(
                    &[C::BrowserRead],
                    OperationNetwork::None,
                    BrowserHostActionV1::Network {
                        failed_only: input.failed_only,
                        max_entries: bounded(input.max_entries, 50, 1, 200),
                    },
                )
            }
            // Read through DevTools; no script runs in the page, nothing changes on it.
            H::BrowserInspect => {
                ensure!(settings.settings.debugging, DEBUGGING_OFF);
                let input: BrowserInspectInput = serde_json::from_value(input.clone())?;
                ensure!(
                    !input.element.trim().is_empty() && input.element.len() <= 64,
                    "browser.inspect: 给出最新快照里的元素 ref（例如 e3）"
                );
                page_action(
                    &[C::BrowserRead],
                    OperationNetwork::None,
                    BrowserHostActionV1::Inspect { element: input.element },
                )
            }
            H::BrowserPerformance => {
                ensure!(settings.settings.debugging, DEBUGGING_OFF);
                let _: BrowserPerformanceInput = serde_json::from_value(input.clone())?;
                page_action(
                    &[C::BrowserRead],
                    OperationNetwork::None,
                    BrowserHostActionV1::Performance,
                )
            }
            other => bail!("browser.unsupported: {}", other.name()),
        };
        *capabilities = caps;
        *action = json!(host);
        Ok(BrowserResolution { resources, network, risks })
    }
}

/// How long Core waits for the host, by action.
pub(crate) fn timeout_ms(action: &BrowserHostActionV1) -> u32 {
    match action {
        BrowserHostActionV1::Open { .. } | BrowserHostActionV1::Navigate { .. } => 45_000,
        BrowserHostActionV1::Wait { timeout_ms, time_ms, .. } => {
            timeout_ms.max(&time_ms.unwrap_or(0)) + 5_000
        }
        BrowserHostActionV1::Press { repeat, hold_ms, .. } => 20_000 + repeat * hold_ms,
        // The host answers when the user does or its own timer runs out; this is the backstop.
        BrowserHostActionV1::RequestUser { timeout_ms, .. } => timeout_ms + 15_000,
        _ => 20_000,
    }
}
