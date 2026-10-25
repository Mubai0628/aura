//! 设置 → 网络与浏览器: the browser switch, the allowed and blocked sites and the debugging
//! switch. Reading is ReadOnly; saving names the revision the page showed. Neither opens a page,
//! and neither is reachable by a model: a model's configuration write is refused outright.
use super::*;

pub(super) const ROUTES: &[RouteDescriptor] = &[
    route!(
        "browser.settings.get",
        Agent,
        Main,
        TrustedRenderer,
        "EmptyToolInput",
        "BrowserSettingsSnapshot",
        ReadOnly,
        None
    ),
    route!(
        "browser.settings.get",
        Agent,
        Core,
        AuthenticatedMain,
        "EmptyToolInput",
        "BrowserSettingsSnapshot",
        ReadOnly,
        None
    ),
    route!(
        "browser.settings.update",
        Agent,
        Main,
        TrustedRenderer,
        "BrowserSettingsUpdate",
        "BrowserSettingsSnapshot",
        CommandInvocation,
        None
    ),
    route!(
        "browser.settings.update",
        Agent,
        Core,
        AuthenticatedMain,
        "BrowserSettingsUpdate",
        "BrowserSettingsSnapshot",
        CommandInvocation,
        None
    ),
    // A screenshot a browser call of the conversation took, for the conversation to show;
    // only an image the conversation itself may read.
    route!(
        "browser.screenshot.read",
        Agent,
        Main,
        TrustedRenderer,
        "BrowserScreenshotReadRequest",
        "BrowserScreenshotImageV1",
        ReadOnly,
        None
    ),
    route!(
        "browser.screenshot.read",
        Agent,
        Core,
        AuthenticatedMain,
        "BrowserScreenshotReadRequest",
        "BrowserScreenshotImageV1",
        ReadOnly,
        None
    ),
    // The browser host's answer to a `browser.command.v1` notification. Main is the host, so
    // the route exists on Core only; the renderer has no way to it.
    route!(
        "browser.host.result",
        Agent,
        Core,
        AuthenticatedMain,
        "BrowserHostResultV1",
        "BrowserHostResultAck",
        TransientInput,
        None
    ),
];
