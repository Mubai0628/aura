//! The `computer.*` tools the model calls. They operate programs on the user's own desktop that
//! the user allowed: a program is named by its id from `computer.apps`, a window by its ref
//! (w1312), an element by its ref from a read of the program (e12), a screenshot by its ref (p3).
use crate::{AppKind, MouseButton, WindowAction, WindowInfo};
use aura_domain_common::ArtifactId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerAppsInput {
    /// Only the programs whose name or id contains this text, ignoring case.
    #[serde(default)]
    pub query: Option<String>,
}

/// One program the tools can operate.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerAppV1 {
    /// The program's id for the other tools.
    pub app: String,
    pub name: String,
    pub kind: AppKind,
    /// It has a window on the desktop now.
    pub running: bool,
    /// This conversation may read and operate it without asking the user again.
    pub allowed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerAppsResult {
    pub apps: Vec<ComputerAppV1>,
    /// How many more programs matched than are listed.
    pub more: u32,
    /// Why the list could not be read, or is not complete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerOpenInput {
    /// The program's id from computer.apps.
    pub app: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerOpenResult {
    pub app: String,
    /// The program's windows, front to back, with refs (w1312) for the other tools.
    pub windows: Vec<WindowInfo>,
    /// It had a window already, so it was not opened again.
    pub already_running: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerSnapshotInput {
    /// The program's id from computer.apps.
    pub app: String,
    /// One of its windows (a ref such as w1312); the one in front when omitted.
    #[serde(default)]
    pub window: Option<String>,
    /// Read only what is inside this element (a ref of an earlier read, such as e12), for one
    /// marked more.
    #[serde(default)]
    pub root: Option<String>,
    /// The most elements to list (default 400, at most 2000).
    #[serde(default)]
    pub max_elements: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerSnapshotResult {
    pub app: String,
    pub window: String,
    pub title: String,
    /// One element a line, indented under the element it is in: its ref, role, name, value,
    /// states and what can be done to it, such as `e12 button "保存" [click]`.
    pub elements: String,
    pub listed: u32,
    /// Everything the window shows was read. When not, read again from an element marked more.
    pub complete: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerScreenshotInput {
    /// The program's id from computer.apps.
    pub app: String,
    /// Only this part of an earlier screenshot, taken again at full detail.
    #[serde(default)]
    pub region: Option<ComputerRegionInput>,
    /// Number the elements of the program's latest computer.snapshot on the image: a box's
    /// number n is the element en.
    #[serde(default)]
    pub marks: bool,
}

/// The part of an earlier screenshot to take again, in that screenshot's pixels.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerRegionInput {
    /// The earlier screenshot's ref, such as p3.
    pub screenshot: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// A window of the program as a screenshot shows it, in the screenshot's pixels.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerShotWindowV1 {
    pub window: String,
    pub title: String,
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
    pub foreground: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerScreenshotResult {
    pub app: String,
    /// The screenshot's ref, for computer.input's points and for a region of it.
    pub screenshot: String,
    pub width: u32,
    pub height: u32,
    /// The program's windows the screenshot shows, front to back.
    pub windows: Vec<ComputerShotWindowV1>,
    /// The image, kept with this call for the user to see.
    pub image_artifact_id: ArtifactId,
    /// Whether the image was attached for the model to look at.
    pub shown_to_model: bool,
    /// How many elements are numbered on the image, when marks were asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marks: Option<u32>,
    pub note: String,
}

/// One step of computer.act, on an element of an earlier read.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ComputerActStepV1 {
    /// Invokes it (a button, a link, a menu item); one that cannot be invoked is toggled,
    /// selected, or expanded or collapsed, whichever it can.
    Click {
        #[serde(rename = "ref")]
        element: String,
    },
    /// Replaces its text, as an edit holds it.
    SetValue {
        #[serde(rename = "ref")]
        element: String,
        value: String,
    },
    Check {
        #[serde(rename = "ref")]
        element: String,
    },
    Uncheck {
        #[serde(rename = "ref")]
        element: String,
    },
    Expand {
        #[serde(rename = "ref")]
        element: String,
    },
    Collapse {
        #[serde(rename = "ref")]
        element: String,
    },
    Select {
        #[serde(rename = "ref")]
        element: String,
    },
    /// Gives it the keyboard focus, which brings its window to the front.
    Focus {
        #[serde(rename = "ref")]
        element: String,
    },
    ScrollIntoView {
        #[serde(rename = "ref")]
        element: String,
    },
    /// Scrolls what it holds: positive down scrolls down, positive right to the right, one unit
    /// a small step such as a line.
    Scroll {
        #[serde(rename = "ref")]
        element: String,
        #[serde(default)]
        down: i32,
        #[serde(default)]
        right: i32,
    },
}

impl ComputerActStepV1 {
    #[must_use]
    pub fn element(&self) -> &str {
        match self {
            Self::Click { element }
            | Self::SetValue { element, .. }
            | Self::Check { element }
            | Self::Uncheck { element }
            | Self::Expand { element }
            | Self::Collapse { element }
            | Self::Select { element }
            | Self::Focus { element }
            | Self::ScrollIntoView { element }
            | Self::Scroll { element, .. } => element,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerActInput {
    /// The program's id from computer.apps.
    pub app: String,
    /// The steps, in order; they stop at the first that fails. Their refs are elements of one
    /// window.
    pub steps: Vec<ComputerActStepV1>,
    /// Take a screenshot of the program afterwards.
    #[serde(default)]
    pub screenshot: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerActResult {
    pub app: String,
    pub window: String,
    pub steps_done: u32,
    /// Each step's element as it is afterwards, one line a step, or that it is gone.
    pub after: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<ComputerScreenshotResult>,
    /// Why the screenshot asked for was not taken.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot_error: Option<String>,
}

/// A point of the screenshot named by the call.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerPointV1 {
    pub x: u32,
    pub y: u32,
}

/// One step of computer.input.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ComputerInputStepV1 {
    Click {
        /// A point of the screenshot the call names (image).
        #[serde(default)]
        x: Option<u32>,
        #[serde(default)]
        y: Option<u32>,
        /// Or the middle of an element of an earlier read, such as e12.
        #[serde(default, rename = "ref")]
        element: Option<String>,
        #[serde(default)]
        button: MouseButton,
        /// 2 is a double click.
        #[serde(default)]
        count: Option<u8>,
    },
    Move {
        /// A point of the screenshot the call names (image).
        #[serde(default)]
        x: Option<u32>,
        #[serde(default)]
        y: Option<u32>,
        /// Or the middle of an element of an earlier read, such as e12.
        #[serde(default, rename = "ref")]
        element: Option<String>,
    },
    /// Presses at the first point, moves through the others and lets go at the last.
    Drag {
        path: Vec<ComputerPointV1>,
        #[serde(default)]
        button: MouseButton,
    },
    /// Turns the wheel over a point: positive down scrolls down, positive right to the right,
    /// one unit a notch.
    Scroll {
        /// A point of the screenshot the call names (image).
        #[serde(default)]
        x: Option<u32>,
        #[serde(default)]
        y: Option<u32>,
        /// Or the middle of an element of an earlier read, such as e12.
        #[serde(default, rename = "ref")]
        element: Option<String>,
        #[serde(default)]
        down: i32,
        #[serde(default)]
        right: i32,
    },
    Type {
        text: String,
    },
    /// A key or a chord: Enter, Escape, Tab, Control+s, Alt+F4.
    Press {
        keys: String,
        #[serde(default)]
        repeat: Option<u8>,
    },
    Wait {
        ms: u32,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerInputInput {
    /// The program's id from computer.apps.
    pub app: String,
    /// The screenshot (a ref such as p3) whose pixels the steps' x and y are; needed when a step
    /// gives a point.
    #[serde(default)]
    pub image: Option<String>,
    pub steps: Vec<ComputerInputStepV1>,
    /// Take a screenshot of the program afterwards.
    #[serde(default)]
    pub screenshot: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerInputResult {
    pub app: String,
    pub steps_done: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<ComputerScreenshotResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerWindowInput {
    /// The program's id from computer.apps.
    pub app: String,
    /// One of its windows (a ref such as w1312); the one in front when omitted.
    #[serde(default)]
    pub window: Option<String>,
    pub action: WindowAction,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerWindowResult {
    pub app: String,
    pub window: String,
    pub action: WindowAction,
    /// False when the window already was as asked.
    pub changed: bool,
    /// The program's windows afterwards, front to back.
    pub windows: Vec<WindowInfo>,
    /// Said when a window asked to close is still open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A conversation, for the routes of the activity list.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerSessionRequest {
    pub session_id: aura_domain_common::SessionId,
}

/// The user's Stop for one program of a conversation, or for all of them.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerStopRequest {
    pub session_id: aura_domain_common::SessionId,
    /// The program; every program of the conversation when omitted.
    #[serde(default)]
    pub app_id: Option<String>,
}

/// A program a conversation is controlling now.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerControlledV1 {
    pub app_id: String,
    pub name: String,
    /// It moves the pointer or types now.
    pub foreground: bool,
    /// The user took the desktop over from it: the turn's actions are refused until the user
    /// gives the desktop back. A turn that ended so stays listed until then, or until the
    /// conversation's next turn runs.
    #[serde(default)]
    pub taken_over: bool,
    #[serde(default)]
    pub mode: crate::ComputerMode,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerControllingResult {
    pub apps: Vec<ComputerControlledV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pip: Option<crate::ComputerPipStatus>,
}

/// The approval card's 总是允许: the program goes on the always-allowed list.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerAlwaysAllowRequest {
    pub app_id: String,
    pub name: String,
}

/// The programs for the composer's 「@」 list.
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerCatalogRequest {
    #[serde(default)]
    pub query: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerCatalogEntryV1 {
    pub app_id: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerCatalogResult {
    /// Off, or the settings cannot be read: the list is empty.
    pub enabled: bool,
    pub apps: Vec<ComputerCatalogEntryV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
}

/// The icons of programs of the catalog, for the composer's 「@」 list, its program blocks and
/// the approval card.
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerIconsRequest {
    /// The programs' IDs, as the catalog gives them.
    pub app_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerIconV1 {
    pub app_id: String,
    /// A PNG, base64.
    pub png: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerIconsResult {
    /// One for each program asked that is in the catalog and has an icon; none while control is
    /// off.
    pub icons: Vec<ComputerIconV1>,
}
