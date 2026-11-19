//! What a browser call touches, decided by Core before the permission engine sees it.
//!
//! The browser must be on and its settings readable; a blocked site is refused here, before any
//! approval could be asked for it. The page's site is the one the host last reported; the host
//! checks again before it acts, so an approval never carries over to a page that has moved.
//! Debugging authority is bound to the observed document and the effective settings revision.
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

/// The page a model call acts on unless it names one the user handed over: one per session.
pub(crate) const MODEL_PAGE: &str = "model";

/// The page the call names: one the user handed over (p1, p2…), or none for the model's own.
fn named_page(input: &Value) -> anyhow::Result<Option<String>> {
    let Some(page) = input.get("page").filter(|page| !page.is_null()) else { return Ok(None) };
    let text = page.as_str().unwrap_or_default();
    if text == MODEL_PAGE {
        return Ok(None);
    }
    let digits = text.strip_prefix('p').unwrap_or("");
    ensure!(
        !digits.is_empty()
            && digits.len() <= 4
            && !digits.starts_with('0')
            && digits.bytes().all(|b| b.is_ascii_digit()),
        "browser.page: “{page}”不是用户交给你的页面编号；用用户交给你时给的编号，例如 p1；不给 page 时是你自己的页面"
    );
    Ok(Some(text.to_owned()))
}

pub(crate) struct BrowserResolution {
    pub resources: Vec<ResourceDescriptor>,
    pub network: OperationNetwork,
    pub risks: Vec<OperationRiskFlag>,
}

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

/// The call's arguments in the tool's own input type; arguments that do not fit are refused in
/// Core's words, with what did not fit.
fn arguments<T: serde::de::DeserializeOwned>(handler: H, input: &Value) -> anyhow::Result<T> {
    serde_json::from_value(input.clone()).map_err(|error| {
        anyhow!(
            "browser.input_invalid: {} 的参数不符合要求（{}），什么都没做；按工具说明改正后再调用。",
            handler.name(),
            super::outside_detail(&error.to_string(), 120)
        )
    })
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
        let named = named_page(action)?;
        let page_id = named.as_deref().unwrap_or(MODEL_PAGE);
        let page = ResourceDescriptor::BrowserPage { page_ref: format!("{session}/{page_id}") };
        let site = |site: &BrowserSiteV1| ResourceDescriptor::BrowserSite {
            host: site.host.clone(),
            allowed_by_rule: allowed
                .iter()
                .find(|rule| rule.matches(site))
                .map(ToString::to_string),
        };
        // The page as the host last reported it.
        let observed = self.browser.observation(session, page_id);
        let current = observed.as_ref().and_then(|p| site_of_url(&p.url));
        let mut on_page = vec![page.clone()];
        on_page.extend(current.as_ref().map(site));
        let input = action.clone();
        let page_action = |caps: &[C], network: OperationNetwork, host: BrowserHostActionV1| {
            (caps.to_vec(), on_page.clone(), network, host)
        };
        let mut risks = vec![];
        let debugging = if super::debugging::action(handler.name()) {
            ensure!(
                current.as_ref().is_none_or(|s| BrowserSiteDecision::of(s, &allowed, &blocked)
                    != BrowserSiteDecision::Blocked),
                "browser.blocked: 当前页面已被浏览器策略阻止，本次调试没有执行。"
            );
            if !settings.settings.debugging {
                risks.push(OperationRiskFlag::SensitiveBrowserAction);
            }
            Some(super::debugging::binding(&settings, observed.as_ref())?)
        } else {
            None
        };
        let (caps, resources, network, host) = match handler {
            H::BrowserOpen => {
                let open: BrowserOpenInput = arguments(handler, &input)?;
                ensure!(
                    open.agent.is_none() || open.url.is_none(),
                    "browser.open: agent 只和 path 一起用，打开那个子 Agent 工作副本里的文件"
                );
                match (open.url, open.path) {
                    (Some(url), None) => {
                        let address =
                            browser_address(&url).ok_or_else(|| not_a_web_address(&url))?;
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
                        // A sub-agent's worktree is a workspace of its own: the file is found
                        // there, and the page only reads it.
                        let copy = open
                            .agent
                            .as_deref()
                            .map(|handle| self.sub_agent_copy(session, handle))
                            .transpose()?;
                        let root = match &open.agent {
                            Some(handle) => format!("{handle} 的工作副本"),
                            None => "这个工作区".to_owned(),
                        };
                        let resolver =
                            self.workspace.resolver(copy.unwrap_or(snapshot.workspace_id))?;
                        // A file written in full is opened when it lies inside the root.
                        let below = match super::local_path::given(resolver.canonical_root(), &path)
                        {
                            super::local_path::Given::Relative => path.clone(),
                            super::local_path::Given::Inside(below) => below,
                            super::local_path::Given::Outside => bail!(
                                "browser.open: {path} 不在{root}里，没有打开；这个工具只打开{root}里的文件。path 写相对于{root}根目录的路径，例如 web/index.html"
                            ),
                        };
                        let relative = WorkspacePath::parse(&below).map_err(|_| {
                            anyhow!(
                                "browser.open: “{path}”不是{root}里的文件路径，没有打开；path 写相对于{root}根目录的路径，例如 web/index.html，不能有 ..、盘符或设备名"
                            )
                        })?;
                        let resolved = resolver.resolve_existing(&relative).map_err(|error| {
                            match error {
                                aura_workspace::WorkspaceError::NotFound { .. } => anyhow!(
                                    "browser.open: {root}里没有 {below}，没有打开；path 相对于{root}根目录，可以先用 fs.find 找到它"
                                ),
                                _ => anyhow!(
                                    "browser.open: 读不到{root}里的 {below}，没有打开；请确认它还在后重试"
                                ),
                            }
                        })?;
                        ensure!(
                            !resolved.info.is_directory,
                            "browser.open: {path} 是文件夹；请给一个文件，例如 index.html"
                        );
                        let url =
                            url::Url::from_file_path(&resolved.absolute_path).map_err(|()| {
                                anyhow!("browser.open: 无法把 {path} 作为本地文件打开")
                            })?;
                        let (caps, resources) = match copy {
                            Some(_) => (vec![C::BrowserRead], vec![page.clone()]),
                            None => (
                                vec![C::BrowserRead, C::WorkspaceFileRead],
                                vec![
                                    page.clone(),
                                    ResourceDescriptor::WorkspacePath {
                                        workspace_id: snapshot.workspace_id,
                                        canonical_relative_path: relative,
                                        file_target: None,
                                    },
                                ],
                            ),
                        };
                        (
                            caps,
                            resources,
                            OperationNetwork::BrowserSitePolicy,
                            BrowserHostActionV1::Open { url: url.to_string() },
                        )
                    }
                    _ => bail!("browser.open: 给出 url 或 path 中的一个"),
                }
            }
            H::BrowserNavigate => {
                let input: BrowserNavigateInput = arguments(handler, &input)?;
                page_action(
                    &[C::BrowserRead],
                    OperationNetwork::BrowserSitePolicy,
                    BrowserHostActionV1::Navigate { action: input.action },
                )
            }
            H::BrowserSnapshot => {
                let input: BrowserSnapshotInput = arguments(handler, &input)?;
                page_action(
                    &[C::BrowserRead],
                    OperationNetwork::None,
                    BrowserHostActionV1::Snapshot {
                        max_chars: bounded(input.max_chars, 12_000, 2_000, 60_000),
                    },
                )
            }
            H::BrowserScreenshot => {
                let input: BrowserScreenshotInput = arguments(handler, &input)?;
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
                let input: BrowserClickInput = arguments(handler, &input)?;
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
                let input: BrowserTypeInput = arguments(handler, &input)?;
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
                let input: BrowserPressInput = arguments(handler, &input)?;
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
                let input: BrowserScrollInput = arguments(handler, &input)?;
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
                let input: BrowserHoverInput = arguments(handler, &input)?;
                page_action(
                    &[C::BrowserInteraction],
                    OperationNetwork::BrowserSitePolicy,
                    BrowserHostActionV1::Hover { element: required(input.element)? },
                )
            }
            H::BrowserSelect => {
                let input: BrowserSelectInput = arguments(handler, &input)?;
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
                let input: BrowserWaitInput = arguments(handler, &input)?;
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
                let input: BrowserConsoleInput = arguments(handler, &input)?;
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
                let _: BrowserCloseInput = arguments(handler, &input)?;
                page_action(&[C::BrowserRead], OperationNetwork::None, BrowserHostActionV1::Close)
            }
            H::BrowserRequestUser => {
                let input: BrowserRequestUserInput = arguments(handler, &input)?;
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
                let input: BrowserEvaluateInput = arguments(handler, &input)?;
                ensure!(
                    !input.expression.trim().is_empty() && input.expression.len() <= 20_000,
                    "browser.evaluate: 给出一段 JavaScript 表达式（最多 20000 字节）"
                );
                let action = BrowserHostActionV1::Evaluate {
                    expression: input.expression,
                    binding: debugging,
                };
                // This computer's own pages (a dev server, a workspace file) are the model's to
                // debug; anywhere else a script reads and acts as the signed-in user.
                if current.as_ref().is_none_or(|site| this_computer(&site.host)) {
                    page_action(
                        &[C::BrowserInteraction],
                        OperationNetwork::BrowserSitePolicy,
                        action,
                    )
                } else {
                    if risks.is_empty() {
                        risks.push(OperationRiskFlag::SensitiveBrowserAction);
                    }
                    page_action(
                        &[C::BrowserSensitiveAction],
                        OperationNetwork::BrowserSitePolicy,
                        action,
                    )
                }
            }
            H::BrowserNetwork => {
                let input: BrowserNetworkInput = arguments(handler, &input)?;
                page_action(
                    &[C::BrowserRead],
                    OperationNetwork::None,
                    BrowserHostActionV1::Network {
                        failed_only: input.failed_only,
                        max_entries: bounded(input.max_entries, 50, 1, 200),
                        binding: debugging,
                    },
                )
            }
            // Read through DevTools; no script runs in the page, nothing changes on it.
            H::BrowserInspect => {
                let input: BrowserInspectInput = arguments(handler, &input)?;
                ensure!(
                    !input.element.trim().is_empty() && input.element.len() <= 64,
                    "browser.inspect: 给出最新快照里的元素 ref（例如 e3）"
                );
                page_action(
                    &[C::BrowserRead],
                    OperationNetwork::None,
                    BrowserHostActionV1::Inspect { element: input.element, binding: debugging },
                )
            }
            H::BrowserPerformance => {
                let _: BrowserPerformanceInput = arguments(handler, &input)?;
                page_action(
                    &[C::BrowserRead],
                    OperationNetwork::None,
                    BrowserHostActionV1::Performance { binding: debugging },
                )
            }
            other => bail!(
                "browser.unsupported: 这个版本的 Aura 不能执行 {}，什么都没做。",
                other.name()
            ),
        };
        *capabilities = caps;
        *action = json!(host);
        Ok(BrowserResolution { resources, network, risks })
    }
}

/// Why a `url` does not open. A file of this computer is opened by its path, which the
/// workspace or a sub-agent's copy confines.
fn not_a_web_address(url: &str) -> anyhow::Error {
    let text = url.trim();
    let bytes = text.as_bytes();
    let drive = bytes.len() > 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    let file = drive
        || text.starts_with("\\\\")
        || text.get(..5).is_some_and(|scheme| scheme.eq_ignore_ascii_case("file:"));
    if file {
        return anyhow!(
            "browser.open: “{url}”是本机文件，不用网址打开：工作区里的文件给 path，子 Agent 工作副本里的文件给 path 和 agent。按 path 打开不能带 ?参数或 #片段；页面需要时，在本机起一个服务，用 http://localhost:端口 打开。"
        );
    }
    anyhow!("browser.open: “{url}”不是网页地址；请给 http(s) 地址，或 localhost:端口")
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
