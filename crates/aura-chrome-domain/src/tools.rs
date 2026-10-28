//! The `chrome.*` tools the model calls. They act on tabs the user shared with the session from
//! their own Chrome, and on tabs the model opened there; every other tab is out of their reach.
//! Their shapes follow `browser.*`, so refs, keys and results read the same in both.
use aura_domain_common::ArtifactId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeTabsInput {}

/// One tab the session may act on.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeTabV1 {
    /// The tab's ref for the other tools, such as t2.
    pub tab: String,
    pub title: String,
    pub url: String,
    /// The tab the tools act on when they are not given one.
    pub current: bool,
    /// Opened by `chrome.open`, so `chrome.close` may close it.
    pub opened_by_aura: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeTabsResult {
    pub tabs: Vec<ChromeTabV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeOpenInput {
    /// An http(s) address, or a host such as localhost:5173. The tab it opens in becomes the
    /// conversation's current tab.
    pub url: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChromeHistoryAction {
    Back,
    Forward,
    Reload,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeNavigateInput {
    /// The tab (a ref from chrome.tabs); the current tab when omitted.
    #[serde(default)]
    pub tab: Option<String>,
    /// An address to go to in that tab. Give url or action, not both.
    #[serde(default)]
    pub url: Option<String>,
    /// Or back, forward or reload.
    #[serde(default)]
    pub action: Option<ChromeHistoryAction>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeSnapshotInput {
    /// The tab (a ref from chrome.tabs); the current tab when omitted.
    #[serde(default)]
    pub tab: Option<String>,
    /// Longest snapshot to return, in characters (default 12000, at most 60000).
    #[serde(default)]
    pub max_chars: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeScreenshotInput {
    /// The tab (a ref from chrome.tabs); the current tab when omitted.
    #[serde(default)]
    pub tab: Option<String>,
    /// Only this element (a ref from the latest snapshot, such as e7).
    #[serde(default, rename = "ref")]
    pub element: Option<String>,
    /// The whole page instead of what is on screen.
    #[serde(default)]
    pub full_page: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChromeMouseButton {
    #[default]
    Left,
    Right,
    Middle,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeClickInput {
    /// The tab (a ref from chrome.tabs); the current tab when omitted.
    #[serde(default)]
    pub tab: Option<String>,
    /// The element to click: a ref from the latest snapshot, such as e12.
    #[serde(default, rename = "ref")]
    pub element: Option<String>,
    /// Or a point in the tab's viewport, in CSS pixels (x with y).
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    #[serde(default)]
    pub button: ChromeMouseButton,
    #[serde(default)]
    pub double: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeTypeInput {
    /// The tab (a ref from chrome.tabs); the current tab when omitted.
    #[serde(default)]
    pub tab: Option<String>,
    /// The field to type into (a ref); without it, the focused element.
    #[serde(default, rename = "ref")]
    pub element: Option<String>,
    pub text: String,
    /// Replace the field's current text instead of adding to it.
    #[serde(default)]
    pub clear: bool,
    /// Press Enter afterwards.
    #[serde(default)]
    pub submit: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromePressInput {
    /// The tab (a ref from chrome.tabs); the current tab when omitted.
    #[serde(default)]
    pub tab: Option<String>,
    /// A key or a chord: Enter, Escape, Tab, ArrowLeft, Space, a, Control+a, Shift+Tab.
    pub key: String,
    /// Press it this many times (default 1, at most 20).
    #[serde(default)]
    pub repeat: Option<u32>,
    /// Keep it held this long, in milliseconds, as a game expects (at most 3000).
    #[serde(default)]
    pub hold_ms: Option<u32>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChromeScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeScrollInput {
    /// The tab (a ref from chrome.tabs); the current tab when omitted.
    #[serde(default)]
    pub tab: Option<String>,
    pub direction: ChromeScrollDirection,
    /// Scroll inside this element (a ref) instead of the page.
    #[serde(default, rename = "ref")]
    pub element: Option<String>,
    /// How far, in CSS pixels (default 600).
    #[serde(default)]
    pub amount: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeHoverInput {
    /// The tab (a ref from chrome.tabs); the current tab when omitted.
    #[serde(default)]
    pub tab: Option<String>,
    #[serde(rename = "ref")]
    pub element: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeSelectInput {
    /// The tab (a ref from chrome.tabs); the current tab when omitted.
    #[serde(default)]
    pub tab: Option<String>,
    /// The select element (a ref).
    #[serde(rename = "ref")]
    pub element: String,
    /// The options to choose, by value or by visible text.
    pub values: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeWaitInput {
    /// The tab (a ref from chrome.tabs); the current tab when omitted.
    #[serde(default)]
    pub tab: Option<String>,
    /// Wait until this text is on the page.
    #[serde(default)]
    pub text: Option<String>,
    /// Wait until this text is gone.
    #[serde(default)]
    pub text_gone: Option<String>,
    /// Or simply wait this long, in milliseconds (at most 10000).
    #[serde(default)]
    pub time_ms: Option<u32>,
    /// Give up after this long, in milliseconds (default 10000, at most 30000).
    #[serde(default)]
    pub timeout_ms: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChromeConsoleLevel {
    Error,
    Warning,
    #[default]
    All,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeConsoleInput {
    /// The tab (a ref from chrome.tabs); the current tab when omitted.
    #[serde(default)]
    pub tab: Option<String>,
    /// Only errors, errors and warnings, or everything (default all).
    #[serde(default)]
    pub level: Option<ChromeConsoleLevel>,
    /// The latest this many entries (default 50, at most 200).
    #[serde(default)]
    pub max_entries: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeCloseInput {
    /// A tab the model opened (a ref from chrome.tabs); the current tab when omitted. A tab the
    /// user shared is not closed: stopping the share is the user's.
    #[serde(default)]
    pub tab: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeRequestUserInput {
    /// The tab (a ref from chrome.tabs); the current tab when omitted.
    #[serde(default)]
    pub tab: Option<String>,
    /// What the user should do in the tab, in a sentence (sign in, enter a code, choose).
    pub reason: String,
    /// How long to wait for them, in seconds (default 300; the settings set the most).
    #[serde(default)]
    pub timeout_seconds: Option<u32>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChromeAssistOutcome {
    Done,
    Declined,
    TimedOut,
    Closed,
}

/// One thing the page reported: a console message, an uncaught error or a load that failed.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeConsoleEntryV1 {
    /// log, info, warning, error, exception or failed_load.
    pub level: String,
    pub text: String,
    /// Where it came from (address and line), when the page said.
    pub source: Option<String>,
}

/// What the tab looks like after an action.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromePageResult {
    pub tab: String,
    pub url: String,
    pub title: String,
    /// The page's accessibility snapshot. Elements carry refs (e3) for click, type and the rest.
    /// Password, one-time-code and payment fields show no value.
    pub snapshot: String,
    pub snapshot_truncated: bool,
    /// Console messages, page errors and failed loads since the previous result for this tab.
    pub console: Vec<ChromeConsoleEntryV1>,
    /// Dialogs the page opened during the action, and how they were answered.
    pub dialogs: Vec<String>,
    /// What stopped or changed the action, such as a new site waiting for the user.
    pub notice: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeAssistResult {
    /// What the user answered. Aura does not check whether what they did worked.
    pub outcome: ChromeAssistOutcome,
    /// The tab afterwards; refs from before are no longer valid.
    pub page: Option<ChromePageResult>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeScreenshotResult {
    pub tab: String,
    pub url: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
    /// The image, kept with this call for the user to see.
    pub image_artifact_id: ArtifactId,
    /// Whether the image was attached for the model to look at.
    pub shown_to_model: bool,
    pub note: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeConsoleResult {
    pub tab: String,
    pub url: String,
    pub entries: Vec<ChromeConsoleEntryV1>,
    /// Older entries were dropped to stay within the limit.
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeCloseResult {
    pub closed: bool,
}
