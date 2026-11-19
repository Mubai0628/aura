use super::bridge::ObservedPage;
use super::{AgentExecution, BackendError, ResourceDescriptor, SecuredOperation, rejected};
use aura_browser_domain::{
    BrowserDebugBindingV1, BrowserHostActionV1, BrowserSiteDecision, site_of_url,
};
use aura_config::BrowserSettingsSnapshot;
use aura_domain_common::SessionId;

const BLOCKED: &str =
    "browser.debugging_blocked: 网页调试已在设置中禁止，本次未执行。可改用页面快照、控制台或截图。";
const STALE: &str = "browser.debugging_changed: 审批对应的网页或浏览器权限已变化，本次未执行。请先刷新页面快照，再按当前状态决定是否需要调试。";

pub(super) fn origin(url: &str) -> Option<String> {
    let origin = url::Url::parse(url).ok()?.origin().ascii_serialization();
    (origin != "null").then_some(origin)
}

pub(super) fn binding(
    settings: &BrowserSettingsSnapshot,
    page: Option<&ObservedPage>,
) -> anyhow::Result<BrowserDebugBindingV1> {
    anyhow::ensure!(settings.settings.debugging_blocked != Some(true), BLOCKED);
    let document_id = page.and_then(|p| p.document_id.clone()).ok_or_else(|| {
        anyhow::anyhow!("browser.debugging_needs_page: 先打开页面或刷新页面快照，再请求调试。")
    })?;
    let settings_revision = settings.revision.ok_or_else(|| anyhow::anyhow!(STALE))?;
    Ok(BrowserDebugBindingV1 {
        origin: page.and_then(|p| origin(&p.url)),
        document_id,
        settings_revision,
    })
}

pub(super) fn action(name: &str) -> bool {
    matches!(
        name,
        "browser.evaluate" | "browser.network" | "browser.inspect" | "browser.performance"
    )
}

impl AgentExecution {
    pub(super) fn verify_browser_debugging(
        &self,
        operation: &SecuredOperation,
    ) -> Result<(), BackendError> {
        let content = &operation.content;
        if !action(&content.action_kind) {
            return Ok(());
        }
        let stale = || rejected(STALE);
        let command: BrowserHostActionV1 =
            serde_json::from_value(content.action.clone()).map_err(|_| stale())?;
        let expected = command.debug_binding().flatten().ok_or_else(stale)?;
        let session = content.identity.session_id.ok_or_else(stale)?;
        let page = content
            .resources
            .iter()
            .find_map(|resource| match resource {
                ResourceDescriptor::BrowserPage { page_ref } => {
                    page_ref.rsplit_once('/').map(|p| p.1)
                }
                _ => None,
            })
            .ok_or_else(stale)?;
        let settings = self.browser.settings().ok_or_else(stale)?;
        if settings.issue.is_some() || !settings.settings.enabled {
            return Err(stale());
        }
        let observed = self.browser.observation(session, page);
        if binding(&settings, observed.as_ref()).ok().as_ref() != Some(expected) {
            return Err(stale());
        }
        Ok(())
    }

    pub(in crate::agent_execution) fn browser_debugging_status(
        &self,
        session: SessionId,
        tool: &str,
    ) -> Option<&'static str> {
        if !action(tool) {
            return None;
        }
        let Some(settings) =
            self.browser.settings().filter(|s| s.issue.is_none() && s.settings.enabled)
        else {
            return Some("内置浏览器当前不可用。");
        };
        if settings.settings.debugging_blocked == Some(true) {
            return Some("设置禁止网页调试。可使用页面快照、控制台或截图。");
        }
        let observed = self.browser.observation(session, super::resolve::MODEL_PAGE);
        if observed.as_ref().and_then(|page| site_of_url(&page.url)).is_some_and(|site| {
            BrowserSiteDecision::of(
                &site,
                &settings.settings.allowed_rules(),
                &settings.settings.blocked_rules(),
            ) == BrowserSiteDecision::Blocked
        }) {
            return Some("当前站点已被浏览器策略阻止，网页调试不可用。");
        }
        if observed.as_ref().is_none_or(|page| page.document_id.is_none()) {
            return Some("先打开页面或刷新页面快照。调试按当前页面的权限处理。");
        }
        if !settings.settings.debugging {
            return Some("当前页面的调试需要用户批准本次操作。");
        }
        if tool == "browser.evaluate" {
            return Some("当前设置允许调试。对外部网页执行脚本需要另行批准。");
        }
        Some("当前设置允许读取此页面的调试信息。")
    }
}
