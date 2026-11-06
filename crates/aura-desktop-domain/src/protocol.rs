//! Lines of JSON between Core and Aura's desktop host, one message per line on the host's
//! standard input and output. Core starts the host and sends [`HostRequest`]s; the host answers
//! each with a [`HostMessage::Reply`] carrying the same id. Requests carry what they need: the
//! host keeps no catalog and no permissions of its own.
use crate::{
    ActStep, AppEntry, AppIcon, Element, ElementKey, InputStep, ManualApp, Monitor, Rect,
    ScreenMap, WindowAction, WindowInfo,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The protocol both ends speak; the host says it first, and Core stops a host that speaks
/// another.
pub const DESKTOP_PROTOCOL_VERSION: u32 = 6;
/// How long the host keeps the desktop for a hold with none of its commands and no
/// [`HostCommand::Keep`], in milliseconds: one model step can take minutes.
pub const HOLD_IDLE_MS: u64 = 300_000;
/// The longest line either end accepts.
pub const MAX_LINE_BYTES: usize = 64 * 1024 * 1024;
/// The largest icon one [`HostCommand::Icons`] may ask for.
pub const MAX_ICON_SIZE: u32 = 256;
/// The most programs one [`HostCommand::Icons`] may name.
pub const MAX_ICON_APPS: usize = 400;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostRequest {
    pub id: u64,
    pub command: HostCommand,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostCommand {
    /// An action carried out while the host keeps the desktop: the banner shows, and a mouse
    /// button, the wheel, Escape or the stop keys of the user's take the desktop over and stop
    /// the action. In a hold, the host keeps the desktop between the hold's commands too, until
    /// [`HostCommand::Release`], the user takes it over, or [`HOLD_IDLE_MS`] pass without a
    /// command of it or a [`HostCommand::Keep`]; without one, for the command. Only action
    /// commands can be wrapped.
    Guarded {
        command: Box<HostCommand>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hold: Option<String>,
        /// The stop shortcut, as [`crate::pip::stop_chord`] reads it; none or unreadable: Escape
        /// alone stops.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stop_keys: Option<String>,
    },
    /// Gives the desktop of hold `hold` back, when the host still keeps it.
    Release { hold: String },
    /// The turn of hold `hold` goes on (its model thinks, or it reads): when the host keeps the
    /// hold, it keeps it another [`HOLD_IDLE_MS`] from now.
    Keep { hold: String },
    /// Checks the current targets without sending input or changing focus.
    Inspect { command: Box<HostCommand> },
    /// The programs of the Start menu's All apps, then the ones the user added.
    Catalog {
        #[serde(default)]
        manual: Vec<ManualApp>,
    },
    /// The visible top-level windows, front to back, each matched to one of `apps` when it can be.
    Windows { apps: Vec<AppEntry> },
    /// Opens a program, waits up to `timeout_ms` for a window of it that was not there before
    /// and brings it to the front. A program that already has a window is not opened again: that
    /// window comes to the front instead.
    Launch { app: AppEntry, timeout_ms: u32 },
    /// The monitors.
    Displays,
    /// The icons of `apps` as the Start menu shows them, `size` pixels square.
    Icons { apps: Vec<AppEntry>, size: u32 },
    /// `region` of the virtual screen, or without one the area around the windows of `show`
    /// (the whole screen when none is on it), with every window not of `show` painted over when
    /// `paint` is on and `marks` drawn, made no longer than `max_long_edge` pixels.
    Screenshot {
        show: Vec<AppEntry>,
        paint: bool,
        max_long_edge: u32,
        #[serde(default)]
        region: Option<Rect>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        marks: Vec<Mark>,
    },
    /// Simulated input for one program, step by step. Pointer steps must land on a window of
    /// `app`, and keys go only to it; `window`, when given, must still be where `expected`
    /// says.
    Input {
        app: AppEntry,
        steps: Vec<InputStep>,
        #[serde(default)]
        window: Option<WindowCheck>,
        /// Chords the system answers (Win+…, Alt+Tab) are refused unless this is on.
        #[serde(default)]
        system_keys: bool,
        /// Long text may be pasted through the clipboard, replacing what the user had on it.
        #[serde(default)]
        clipboard: bool,
    },
    /// The elements of one of `app`'s windows (`window`, or the front one) as UI Automation
    /// shows them, or of the part under `root`: only what is on the screen and can be used or
    /// read, at most `max_elements` of them, no more than `max_depth` levels down, read within
    /// `timeout_ms`.
    Elements {
        app: AppEntry,
        #[serde(default)]
        window: Option<String>,
        #[serde(default)]
        root: Option<ElementKey>,
        max_elements: u32,
        max_depth: u32,
        timeout_ms: u32,
    },
    /// Acts on elements of one of `app`'s windows step by step in the background: no pointer,
    /// no keys, and the window stays where it is (only `focus` brings it to the front). Stops at
    /// the first step that fails.
    Act { app: AppEntry, window: String, steps: Vec<ActStep>, timeout_ms: u32 },
    /// Does `action` to one of `app`'s windows (`window`, or the front one) and waits a little
    /// for the window to change.
    Window {
        app: AppEntry,
        #[serde(default)]
        window: Option<String>,
        action: WindowAction,
    },
}

impl HostCommand {
    /// The program an action is for: the commands that can change something on the desktop.
    /// `None` for what only reads.
    #[must_use]
    pub fn acts_on(&self) -> Option<&AppEntry> {
        match self {
            Self::Launch { app, .. }
            | Self::Input { app, .. }
            | Self::Act { app, .. }
            | Self::Window { app, .. } => Some(app),
            _ => None,
        }
    }

    /// Whether the action takes the foreground from the user: it moves the pointer or types,
    /// or brings a window to the front, as opening a program does.
    #[must_use]
    pub fn takes_foreground(&self) -> bool {
        match self {
            Self::Input { .. } | Self::Launch { .. } => true,
            Self::Act { steps, .. } => {
                steps.iter().any(|step| matches!(step, ActStep::Focus { .. }))
            }
            Self::Window { action, .. } => {
                matches!(action, WindowAction::Front | WindowAction::Maximize)
            }
            _ => false,
        }
    }
}

/// A numbered box drawn over an element on a screenshot: `number` is the element's ref
/// (12 for e12).
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Mark {
    pub number: u32,
    pub rect: Rect,
}

/// Where a window was when the screenshot that a step's points come from was taken.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WindowCheck {
    pub window: String,
    pub expected: Rect,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostMessage {
    /// The steps of request `id` done so far.
    Control {
        id: u64,
        app_id: String,
        steps_done: u32,
    },
    /// The host's first line.
    Ready {
        protocol: u32,
    },
    Reply {
        id: u64,
        outcome: HostOutcome,
    },
    /// The host gave the desktop of hold `hold` back by itself, between requests or before
    /// the reply it writes next.
    HoldEnded {
        hold: String,
        reason: HoldEnd,
        /// What of the user's took the desktop over, as [`taken_over_message`] words it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cause: Option<String>,
    },
}

/// What the host and Core tell the model when the user took the desktop over; `cause` (for
/// example 点了鼠标) says how, when known.
#[must_use]
pub fn taken_over_message(cause: Option<&str>) -> String {
    match cause {
        Some(cause) => format!("用户已接管电脑（{cause}），用户交还操作权之前不要再操作电脑"),
        None => "用户已接管电脑，用户交还操作权之前不要再操作电脑".into(),
    }
}

/// Why the host gave a hold's desktop back by itself.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldEnd {
    /// The user took the desktop over: a mouse button, the wheel, Escape or the stop keys.
    /// The hold's later actions are refused.
    TakenOver,
    /// No command of the hold, and no [`HostCommand::Keep`], for [`HOLD_IDLE_MS`].
    Idle,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HostOutcome {
    Done(HostResult),
    Failed(HostError),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostResult {
    Inspected {
        safety: String,
    },
    Catalog {
        apps: Vec<AppEntry>,
    },
    Windows {
        windows: Vec<WindowInfo>,
    },
    Launched {
        windows: Vec<WindowInfo>,
        already_running: bool,
    },
    Displays {
        monitors: Vec<Monitor>,
    },
    Icons {
        /// One for each program asked that Windows has an icon for, in the order asked.
        icons: Vec<AppIcon>,
    },
    Screenshot {
        /// A PNG, base64.
        png: String,
        map: ScreenMap,
        monitors: Vec<Monitor>,
        /// The windows of `show` the screenshot has, front to back.
        windows: Vec<WindowInfo>,
    },
    Input {
        steps_done: u32,
    },
    Elements {
        window: WindowInfo,
        elements: Vec<Element>,
        /// Everything on the screen was read. When not, what is missing is inside the elements
        /// marked `more`, or, when none is, anywhere in the window.
        complete: bool,
    },
    Acted {
        /// Each step's element as it is after the step; `None` when it is gone, such as when
        /// the step closed its window.
        after: Vec<Option<Element>>,
    },
    Window {
        /// The window the action was for.
        window: String,
        /// False when the window already was as asked, so nothing was sent.
        changed: bool,
        /// The program's windows afterwards, front to back.
        windows: Vec<WindowInfo>,
    },
    /// The hold is not kept any more.
    Released,
    /// Whether the host still keeps the hold [`HostCommand::Keep`] named.
    Kept {
        kept: bool,
    },
}

/// Whether what failed may have changed anything on the desktop.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    /// Nothing was sent to the program: trying again is safe.
    NotDelivered,
    /// It may have reached the program: read the program again before deciding.
    Uncertain,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostErrorCode {
    /// The user took the desktop over; the hold's actions are refused from now on.
    TakenOver,
    /// Another hold keeps the desktop.
    Busy,
    InvalidRequest,
    AppNotFound,
    WindowNotFound,
    LaunchFailed,
    /// A window moved, changed or covered the point since the screenshot.
    Moved,
    /// The program's window could not be brought to the front.
    NotForeground,
    /// The input desktop is not the user's (locked, or a UAC prompt).
    DesktopLocked,
    /// Beyond what the host may do: a program running as administrator, a system chord.
    Refused,
    /// The program did not answer in time.
    Timeout,
    /// An element of an earlier read is gone or changed: read the window again.
    Stale,
    /// The element cannot do what the step asks, or not now (it is disabled).
    Unsupported,
    Internal,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostError {
    pub code: HostErrorCode,
    pub message: String,
    pub delivery: Delivery,
    /// The steps of an input or an act carried out before the step that failed; absent when the
    /// request failed before its first step began (it was refused, or the window did not answer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps_done: Option<u32>,
}

impl HostError {
    #[must_use]
    pub fn new(code: HostErrorCode, message: impl Into<String>, delivery: Delivery) -> Self {
        Self { code, message: message.into(), delivery, steps_done: None }
    }
}
