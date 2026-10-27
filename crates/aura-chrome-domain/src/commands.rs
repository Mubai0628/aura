//! What Core asks the extension to do in a shared tab, and what the extension reports back. A
//! command names Chrome's own tab id, which Core resolved from the model's tab reference and the
//! session's shares; neither the id nor any other tab reaches the model.
use crate::tools::{
    ChromeAssistOutcome, ChromeConsoleEntryV1, ChromeConsoleLevel, ChromeHistoryAction,
    ChromeMouseButton, ChromeScrollDirection,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChromeCommand {
    /// A new tab in the "Aura" tab group, shared with the calling session.
    Open {
        url: String,
    },
    Navigate {
        tab_id: u32,
        url: Option<String>,
        action: Option<ChromeHistoryAction>,
    },
    Snapshot {
        tab_id: u32,
        max_chars: u32,
    },
    Screenshot {
        tab_id: u32,
        element: Option<String>,
        full_page: bool,
    },
    /// Looks at what an action would touch before Core decides on it: the element, the point a
    /// click goes to, or else the focused element.
    Inspect {
        tab_id: u32,
        element: Option<String>,
        x: Option<f64>,
        y: Option<f64>,
        intent: ChromeIntent,
    },
    Click {
        tab_id: u32,
        element: Option<String>,
        x: Option<f64>,
        y: Option<f64>,
        button: ChromeMouseButton,
        double: bool,
    },
    Type {
        tab_id: u32,
        element: Option<String>,
        text: String,
        clear: bool,
        submit: bool,
    },
    Press {
        tab_id: u32,
        key: String,
        repeat: u32,
        hold_ms: u32,
    },
    Scroll {
        tab_id: u32,
        direction: ChromeScrollDirection,
        element: Option<String>,
        amount: u32,
    },
    Hover {
        tab_id: u32,
        element: String,
    },
    Select {
        tab_id: u32,
        element: String,
        values: Vec<String>,
    },
    Wait {
        tab_id: u32,
        text: Option<String>,
        text_gone: Option<String>,
        time_ms: Option<u32>,
        timeout_ms: u32,
    },
    Console {
        tab_id: u32,
        level: Option<ChromeConsoleLevel>,
        max_entries: u32,
    },
    /// Closes a tab Aura opened.
    Close {
        tab_id: u32,
    },
    /// Shows the user the takeover banner; the outcome is theirs.
    Assist {
        tab_id: u32,
        reason: String,
        timeout_seconds: u32,
    },
    /// Detaches from these tabs; nothing more runs in them until a new command.
    Release {
        tab_ids: Vec<u32>,
    },
}

/// The action an [`ChromeCommand::Inspect`] is about.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChromeIntent {
    Click,
    Type,
    Press,
    Select,
    Hover,
    Scroll,
}

/// What an action would touch: refused outright (a password, one-time-code or payment field, a
/// CAPTCHA, a passkey dialog, a file chooser, a browser or store page), or one the user approves
/// each time (buy, pay, delete, send, publish, transfer; a form with such fields).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeInspection {
    pub refused: Option<String>,
    pub sensitive: bool,
    /// The tab's origin now.
    pub origin: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeOpenedTab {
    pub tab_id: u32,
    pub window_id: u32,
    pub url: String,
    pub title: String,
}

/// The tab after an action; Core adds the model's reference to it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromePageReport {
    pub url: String,
    pub title: String,
    pub snapshot: String,
    pub snapshot_truncated: bool,
    pub console: Vec<ChromeConsoleEntryV1>,
    pub dialogs: Vec<String>,
    pub notice: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeScreenshotReport {
    pub url: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub png_base64: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeConsoleReport {
    pub url: String,
    pub entries: Vec<ChromeConsoleEntryV1>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeAssistReport {
    pub outcome: ChromeAssistOutcome,
    pub page: Option<ChromePageReport>,
}

/// A conversation the user can share a tab with, as the extension lists it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeSessionChoice {
    pub session_id: String,
    pub title: String,
    pub workspace: String,
}

/// How long a share lasts: until the user stops it, or until the conversation's turn ends (the
/// running one, or the next when none runs). A share lives in memory, so a restart of Core or of
/// Chrome ends it either way.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChromeShareScope {
    #[default]
    Session,
    Turn,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChromeShareState {
    Active,
    /// The tab moved to a site the user has not confirmed for this share.
    AwaitingOrigin,
    /// The user has the tab back for a while (`chrome.request_user`).
    WithUser,
}

/// One shared tab, as the extension's side panel shows it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeShareView {
    pub tab_id: u32,
    pub session_id: String,
    pub session_title: String,
    pub origin: String,
    pub state: ChromeShareState,
    pub scope: ChromeShareScope,
}

/// The tab `Open` made, and what it shows.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeOpenReport {
    pub tab: ChromeOpenedTab,
    pub page: ChromePageReport,
}

/// The result of `Close`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeCloseReport {
    pub closed: bool,
}
