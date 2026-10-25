//! 设置 → 环境: the interface settings — whether the session shows its debugging information
//! and which terminal a new terminal tab opens. Reading is ReadOnly; saving names the revision
//! the page showed. Neither starts anything.
use super::*;

pub(super) const ROUTES: &[RouteDescriptor] = &[
    route!(
        "ui.settings.get",
        Agent,
        Main,
        TrustedRenderer,
        "EmptyToolInput",
        "UiSettingsSnapshot",
        ReadOnly,
        None
    ),
    route!(
        "ui.settings.get",
        Agent,
        Core,
        AuthenticatedMain,
        "EmptyToolInput",
        "UiSettingsSnapshot",
        ReadOnly,
        None
    ),
    route!(
        "ui.settings.update",
        Agent,
        Main,
        TrustedRenderer,
        "UiSettingsUpdate",
        "UiSettingsSnapshot",
        CommandInvocation,
        None
    ),
    route!(
        "ui.settings.update",
        Agent,
        Core,
        AuthenticatedMain,
        "UiSettingsUpdate",
        "UiSettingsSnapshot",
        CommandInvocation,
        None
    ),
];
