//! What Core asks the browser host (Electron Main) to do on a session's model page, and what
//! comes back. Core decides and records; the host acts, and checks again before it acts that the
//! page is still where Core authorized the action.
use crate::{
    BrowserConsoleLevel, BrowserHistoryAction, BrowserMouseButton, BrowserScrollDirection,
};
use aura_domain_common::{ArtifactId, RuntimeInstanceId, SessionId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How the session's model page may navigate while it carries out the command.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserPageAccessV1 {
    /// A ReadOnly session: the page navigates only to allowed sites and approved hosts.
    pub read_only: bool,
    /// Hosts the user approved for this session, and the one approved for this command.
    pub approved_hosts: Vec<String>,
    /// The workspace folder (a `file:` address ending in `/`) a workspace page may load from.
    pub file_root: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BrowserHostActionV1 {
    Open {
        url: String,
    },
    Navigate {
        action: BrowserHistoryAction,
    },
    Snapshot {
        max_chars: u32,
    },
    Screenshot {
        element: Option<String>,
        full_page: bool,
    },
    Click {
        element: Option<String>,
        x: Option<f64>,
        y: Option<f64>,
        button: BrowserMouseButton,
        double: bool,
    },
    Type {
        element: Option<String>,
        text: String,
        clear: bool,
        submit: bool,
    },
    Press {
        key: String,
        repeat: u32,
        hold_ms: u32,
    },
    Scroll {
        element: Option<String>,
        direction: BrowserScrollDirection,
        amount: u32,
    },
    Hover {
        element: String,
    },
    Select {
        element: String,
        values: Vec<String>,
    },
    Wait {
        text: Option<String>,
        text_gone: Option<String>,
        time_ms: Option<u32>,
        timeout_ms: u32,
    },
    Console {
        level: BrowserConsoleLevel,
        max_entries: u32,
    },
    Close,
    RequestUser {
        reason: String,
        timeout_ms: u32,
    },
    Evaluate {
        expression: String,
    },
    Network {
        failed_only: bool,
        max_entries: u32,
    },
    /// One element's HTML, box and computed style, read through DevTools.
    Inspect {
        element: String,
    },
    /// The page's performance counters.
    Performance,
}

impl BrowserHostActionV1 {
    /// Whether the action is the model's input on the page: refused while the user has taken the
    /// page over, and of unknown effect when it does not answer.
    #[must_use]
    pub fn is_input(&self) -> bool {
        matches!(
            self,
            Self::Click { .. }
                | Self::Type { .. }
                | Self::Press { .. }
                | Self::Scroll { .. }
                | Self::Hover { .. }
                | Self::Select { .. }
                | Self::Evaluate { .. }
        )
    }
}

/// `browser.command.v1`, Core to Main.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserHostCommandV1 {
    /// Unguessable, one per command; the result names it.
    pub command_id: String,
    pub runtime_instance_id: RuntimeInstanceId,
    pub session_id: SessionId,
    /// The host of the page when Core authorized the action (none: a blank or workspace page).
    /// An input action on a page that has since moved elsewhere is refused, not carried out.
    pub expected_host: Option<String>,
    pub access: BrowserPageAccessV1,
    pub action: BrowserHostActionV1,
    /// How long Core waits for the result.
    pub timeout_ms: u32,
}

/// `browser.cancel.v1`, Core to Main: Core no longer waits for this command (the turn was
/// stopped, or Core's timer ran out). A command not started yet is dropped; a wait for the user
/// ends. An input already under way is not undone.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserHostCancelV1 {
    pub command_id: String,
    pub runtime_instance_id: RuntimeInstanceId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserImageV1 {
    pub media_type: String,
    pub width: u32,
    pub height: u32,
    pub data_base64: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum BrowserHostOutcomeV1 {
    /// Done. `result` is the tool's result as the host saw it; Core checks it against the tool.
    Completed { result: serde_json::Value, image: Option<BrowserImageV1> },
    /// Not carried out: the page was not open, had moved, the user had it, a ref was stale, a
    /// field was sensitive. Nothing happened on the page.
    Refused { message: String },
    /// Started and failed. `may_have_acted` says whether the page could have changed.
    Failed { message: String, may_have_acted: bool },
}

/// `browser.host.result`, Main to Core.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserHostResultV1 {
    pub command_id: String,
    /// The page's address afterwards (about:blank when there is no page).
    pub page_url: String,
    pub outcome: BrowserHostOutcomeV1,
}

/// `browser.screenshot.read`: a screenshot a browser call of this conversation took, for the
/// conversation to show.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserScreenshotReadRequest {
    pub session_id: SessionId,
    pub artifact_id: ArtifactId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserScreenshotImageV1 {
    pub media_type: String,
    pub data_base64: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserHostResultAck {
    /// False when no command was waiting under that id (it had timed out or never existed).
    pub accepted: bool,
}
