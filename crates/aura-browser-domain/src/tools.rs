//! The `browser.*` tools the model calls. They act on the session's model page in Aura's
//! built-in browser; a page the user opened in another tab is out of their reach.
use aura_domain_common::ArtifactId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserOpenInput {
    /// An http(s) address, or a host such as localhost:5173. Give url or path, not both.
    #[serde(default)]
    pub url: Option<String>,
    /// A workspace file to open instead, such as index.html or dist/game.html.
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserHistoryAction {
    Back,
    Forward,
    Reload,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserNavigateInput {
    pub action: BrowserHistoryAction,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserSnapshotInput {
    /// Longest snapshot to return, in characters (default 12000, at most 60000).
    #[serde(default)]
    pub max_chars: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserScreenshotInput {
    /// Only this element (a ref from the latest snapshot, such as e7).
    #[serde(default, rename = "ref")]
    pub element: Option<String>,
    /// The whole page instead of what is on screen.
    #[serde(default)]
    pub full_page: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserMouseButton {
    #[default]
    Left,
    Right,
    Middle,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserClickInput {
    /// The element to click: a ref from the latest snapshot, such as e12.
    #[serde(default, rename = "ref")]
    pub element: Option<String>,
    /// Or a point on the page, in CSS pixels of its 1280x800 viewport (x with y).
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    #[serde(default)]
    pub button: BrowserMouseButton,
    #[serde(default)]
    pub double: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserTypeInput {
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
pub struct BrowserPressInput {
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
pub enum BrowserScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserScrollInput {
    pub direction: BrowserScrollDirection,
    /// Scroll inside this element (a ref) instead of the page.
    #[serde(default, rename = "ref")]
    pub element: Option<String>,
    /// How far, in CSS pixels (default 600).
    #[serde(default)]
    pub amount: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserHoverInput {
    #[serde(rename = "ref")]
    pub element: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserSelectInput {
    /// The select element (a ref).
    #[serde(rename = "ref")]
    pub element: String,
    /// The options to choose, by value or by visible text.
    pub values: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserWaitInput {
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
pub enum BrowserConsoleLevel {
    Error,
    Warning,
    #[default]
    All,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserConsoleInput {
    /// Only errors, errors and warnings, or everything (default all).
    #[serde(default)]
    pub level: Option<BrowserConsoleLevel>,
    /// The latest this many entries (default 50, at most 200).
    #[serde(default)]
    pub max_entries: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserCloseInput {}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserRequestUserInput {
    /// What the user should do on the page, in a sentence (sign in, solve a check, choose).
    pub reason: String,
    /// How long to wait for them, in seconds (default 180, at most 300).
    #[serde(default)]
    pub timeout_seconds: Option<u32>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserAssistOutcome {
    Done,
    Declined,
    TimedOut,
    Closed,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserAssistResult {
    /// What the user answered. Aura does not check whether what they did worked.
    pub outcome: BrowserAssistOutcome,
    /// The page afterwards; refs from before are no longer valid.
    pub page: Option<BrowserPageResult>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserEvaluateInput {
    /// A JavaScript expression, run in the page; a promise is awaited (at most 10 seconds).
    pub expression: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserEvaluateResult {
    pub url: String,
    /// The value as JSON text (at most 20000 characters), when it ran.
    pub value: Option<String>,
    pub value_truncated: bool,
    /// The error it threw, when it threw.
    pub exception: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserNetworkInput {
    /// Only requests that failed or answered 400 and above.
    #[serde(default)]
    pub failed_only: bool,
    /// The latest this many requests (default 50, at most 200).
    #[serde(default)]
    pub max_entries: Option<u32>,
}

/// One request the page made. Cookie and authorization header values are not shown.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserNetworkEntryV1 {
    pub method: String,
    pub url: String,
    /// document, script, image, fetch, xhr, websocket…
    pub kind: String,
    pub status: Option<u16>,
    pub mime_type: Option<String>,
    pub failure: Option<String>,
    pub request_headers: Vec<String>,
    pub response_headers: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserNetworkResult {
    pub url: String,
    pub entries: Vec<BrowserNetworkEntryV1>,
    pub truncated: bool,
}

/// One element, as DevTools' Elements panel shows it. Read through DevTools, no page script runs.
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserInspectInput {
    /// The element's ref in the latest snapshot (e3).
    pub element: String,
}

/// Where an element is drawn, in CSS pixels of the page's viewport, rounded.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserElementBoxV1 {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserInspectResult {
    pub url: String,
    /// The element's HTML as the page has it now (at most 8000 characters).
    pub html: String,
    pub html_truncated: bool,
    /// Its border box; none when it is not drawn (display: none, detached).
    pub box_model: Option<BrowserElementBoxV1>,
    /// Its computed style for layout and look, one property a line ("display: flex").
    pub styles: Vec<String>,
}

/// The page's performance counters, as Chromium keeps them.
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserPerformanceInput {}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserPerformanceResult {
    pub url: String,
    /// One counter a line, with its unit ("JSHeapUsedSize: 12.4 MB", "LayoutCount: 42").
    pub metrics: Vec<String>,
}

/// One thing the page reported: a console message, an uncaught error or a load that failed.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserConsoleEntryV1 {
    /// log, info, warning, error, exception or failed_load.
    pub level: String,
    pub text: String,
    /// Where it came from (address and line), when the page said.
    pub source: Option<String>,
}

/// What the page looks like after an action.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserPageResult {
    pub url: String,
    pub title: String,
    /// The page's accessibility snapshot. Elements carry refs (e3) for click, type and the rest.
    pub snapshot: String,
    pub snapshot_truncated: bool,
    /// Console messages, page errors and failed loads since the previous browser result.
    pub console: Vec<BrowserConsoleEntryV1>,
    /// Dialogs the page opened during the action, and how Aura answered them.
    pub dialogs: Vec<String>,
    /// What stopped or changed the action, such as a navigation the settings refused.
    pub notice: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserScreenshotResult {
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
pub struct BrowserConsoleResult {
    pub url: String,
    pub entries: Vec<BrowserConsoleEntryV1>,
    /// Older entries were dropped to stay within the limit.
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserCloseResult {
    pub closed: bool,
}
