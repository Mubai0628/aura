//! A plugin's panel acting in the conversation it was opened in. Main makes these calls for the
//! panel's page and names the plugin and the conversation from the panel that called; the window
//! has no way to them. Core checks each against what the plugin's trusted manifest declares.
use super::*;

pub(super) const ROUTES: &[RouteDescriptor] = &[
    route!(
        "agent.plugin_session.state",
        Agent,
        Core,
        AuthenticatedMain,
        "PluginSessionRequest",
        "PluginSessionState",
        ReadOnly,
        None
    ),
    route!(
        "agent.plugin_session.events",
        Agent,
        Core,
        AuthenticatedMain,
        "PluginSessionEventsRequest",
        "PluginSessionEvents",
        ReadOnly,
        None
    ),
    route!(
        "agent.plugin_session.send",
        Agent,
        Core,
        AuthenticatedMain,
        "PluginSessionSendRequest",
        "PluginSessionSent",
        CommandInvocation,
        None
    ),
    // One of the plugin's own prompt commands, sent as its message as `send` sends one.
    route!(
        "agent.plugin_session.run",
        Agent,
        Core,
        AuthenticatedMain,
        "PluginCommandRunRequest",
        "PluginSessionSent",
        CommandInvocation,
        None
    ),
    route!(
        "agent.plugin_session.stop",
        Agent,
        Core,
        AuthenticatedMain,
        "PluginSessionRequest",
        "PluginSessionStopped",
        CommandInvocation,
        None
    ),
    // The plugin's own text settings, read and set as the plugin page sets them. They belong to
    // no conversation.
    route!(
        "agent.plugin_session.settings",
        Agent,
        Core,
        AuthenticatedMain,
        "PluginSettingsRequest",
        "PluginSettingValues",
        ReadOnly,
        None
    ),
    route!(
        "agent.plugin_session.set_setting",
        Agent,
        Core,
        AuthenticatedMain,
        "PluginSettingSetRequest",
        "PluginSettingValues",
        CommandInvocation,
        None
    ),
    // What Main needs to show the panel, while the plugin and its panel can both be used.
    route!(
        "agent.plugin_session.panel",
        Agent,
        Core,
        AuthenticatedMain,
        "PluginSessionRequest",
        "PluginSessionPanel",
        ReadOnly,
        None
    ),
    // A problem Main saw in the panel, for the plugin's log: kept once, never replayed, with no
    // result to ask for later.
    route!(
        "agent.plugin_session.log",
        Agent,
        Core,
        AuthenticatedMain,
        "PluginPanelLogRequest",
        "PluginPanelLogged",
        TransientInput,
        None
    ),
    // A notice, or the plugin's status entry, checked before Main shows it: shown once, never
    // replayed. Neither belongs to a conversation.
    route!(
        "agent.plugin_session.notify",
        Agent,
        Core,
        AuthenticatedMain,
        "PluginNoticeRequest",
        "PluginNotice",
        TransientInput,
        None
    ),
    route!(
        "agent.plugin_session.status",
        Agent,
        Core,
        AuthenticatedMain,
        "PluginStatusRequest",
        "PluginStatus",
        TransientInput,
        None
    ),
];
