//! What Aura's settings page reads about Chrome control and asks of it.
use aura_domain_common::UtcTimestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Chrome control as the settings page shows it.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeControlStatus {
    pub enabled: bool,
    /// The settings revision the page changes next; `None` while the settings cannot be read.
    pub revision: Option<u64>,
    /// Why the settings cannot be read, in the user's words.
    pub issue: Option<String>,
    /// Whether Chrome can find Aura's relay.
    pub registered: bool,
    pub paired: Option<ChromePairedExtension>,
    pub connection: ChromeConnectionState,
    /// When the pairing code the page showed stops working, while one is waiting.
    pub pairing_expires_at: Option<UtcTimestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromePairedExtension {
    pub extension_id: String,
    pub paired_at: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChromeConnectionState {
    /// Chrome control is off.
    Off,
    /// On, and no extension is connected.
    Waiting,
    /// An extension is connected and has not paired or proved its pairing yet.
    Pairing,
    Connected,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeSetEnabledRequest {
    pub expected_revision: u64,
    pub enabled: bool,
}

/// A code the user types into the extension once; it works for five minutes and five tries.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromePairingCode {
    pub code: String,
    pub expires_at: UtcTimestamp,
}

/// A conversation, for what Aura shows about its shared tabs.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeSessionRequest {
    pub session_id: aura_domain_common::SessionId,
}

/// What a shared tab is doing now.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChromeSharedTabState {
    Active,
    /// The tab moved to a site the user has not confirmed for this share.
    AwaitingOrigin,
    /// The model handed the tab to the user (`chrome.request_user`).
    WithUser,
}

/// One tab the user shared with the conversation, or that the model opened for it.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeSharedTab {
    /// The conversation's name for the tab (t1, t2…).
    pub tab: String,
    pub title: String,
    pub url: String,
    pub state: ChromeSharedTabState,
    /// The share ends with the conversation's turn.
    pub this_turn_only: bool,
    /// What the model asked the user to do, while the user has the tab.
    pub assist_reason: Option<String>,
}

/// The conversation's shared tabs, for the line above its message box.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeSessionShares {
    /// Whether the extension is connected now.
    pub connected: bool,
    pub tabs: Vec<ChromeSharedTab>,
}

/// The extension folder the user loads in Chrome, and whether the file manager opened it.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeExtensionFolder {
    pub path: String,
    /// False when the folder is missing or could not be opened.
    pub opened: bool,
}
