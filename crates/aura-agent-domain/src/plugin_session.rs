//! What a plugin's panel may read and do in the conversation it was opened in. Main names the
//! plugin and the conversation from the panel that called, so a page never chooses them; Core
//! checks each call against what the plugin's trusted manifest declares.
use crate::{AgentSessionWaitingFor, AgentTurnOutcome, TodoItem};
use aura_domain_common::{
    ApprovalRequestId, ContextProjectionSnapshotId, EventId, ModelProfileId, ModelStepId,
    SessionId, ToolCallId, TurnTaskId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The most a plugin may send as one message.
pub const PLUGIN_MESSAGE_MAX_CHARS: usize = 16_000;
/// The longest de-duplication key a plugin may give a message.
pub const PLUGIN_DEDUPE_KEY_MAX_CHARS: usize = 128;

/// One plugin and the conversation it acts in: the conversation's public state needs
/// `session.read`, stopping its turn needs `session.control`.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginSessionRequest {
    pub plugin: String,
    pub session_id: SessionId,
}

/// What happened after the cursor `after`; needs `session.events`. Without `after`, nothing that
/// happened before now: only the cursor to read from.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginSessionEventsRequest {
    pub plugin: String,
    pub session_id: SessionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<u64>,
}

/// A message for the user, marked as the plugin's; needs `session.send`. The same `dedupe_key`
/// again is the same message and never a second turn.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginSessionSendRequest {
    pub plugin: String,
    pub session_id: SessionId,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_key: Option<String>,
}

/// One of the plugin's own prompt commands run in the conversation as the plugin's message; needs
/// `commands`. `arguments` go where its template says, as the text after a typed command does.
/// The same `dedupe_key` again is the same message and never a second turn.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginCommandRunRequest {
    pub plugin: String,
    pub session_id: SessionId,
    /// The command's name, without the `/`.
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub arguments: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_key: Option<String>,
}

/// The values of the plugin's text settings; needs `settings`.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginSettingsRequest {
    pub plugin: String,
}

/// One of the plugin's text settings, set as the plugin page sets it; needs `settings`. An empty
/// value clears it. A secret setting is set only on the plugin page.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginSettingSetRequest {
    pub plugin: String,
    pub key: String,
    pub value: String,
}

/// The plugin's text settings that have a value, by key.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginSettingValues {
    pub values: BTreeMap<String, String>,
}

/// What the conversation is doing now.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginSessionActivity {
    Idle,
    Running,
    Waiting,
}

/// What the conversation's finished requests sent and received, as their Providers reported.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginSessionUsage {
    pub requests: u64,
    pub input_tokens: u64,
    /// Of the input, what the Providers said they read from their cache.
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
}

/// The newest request's context, as the meter beside the composer shows it.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginSessionContextUse {
    pub window_tokens: u64,
    pub used_tokens: u64,
    /// The figure was estimated before the request went out.
    pub estimated: bool,
}

/// The conversation as a plugin may see it: no credentials, no file contents, no tool results.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginSessionState {
    pub session_id: SessionId,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_profile_id: Option<ModelProfileId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    pub activity: PluginSessionActivity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_for: Option<AgentSessionWaitingFor>,
    pub usage: PluginSessionUsage,
    /// The newest turn's to-do list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub todo: Vec<TodoItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<PluginSessionContextUse>,
    /// Where an events read started now begins.
    pub cursor: u64,
}

/// What one request to the model used, as its Provider reported.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginStepUsage {
    pub input_tokens: u64,
    /// Of the input, what the Provider said it read from its cache.
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
}

/// How a compaction of the conversation's history ended.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginCompactionOutcome {
    Completed,
    Failed,
    Cancelled,
    /// Its turn was stopped or lost while it ran.
    Interrupted,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PluginSessionEventKind {
    TurnStarted {
        turn_task_id: TurnTaskId,
    },
    /// The turn left the conversation. Without `outcome` it was taken off for recovery.
    TurnEnded {
        turn_task_id: TurnTaskId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outcome: Option<AgentTurnOutcome>,
        /// The turn's final reply, when it completed with one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        final_reply: Option<String>,
        /// Why it failed or needs recovery, as Aura records it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    ApprovalWaiting {
        turn_task_id: TurnTaskId,
        approval_id: ApprovalRequestId,
    },
    /// A request to the model is about to go out.
    StepStarted {
        turn_task_id: TurnTaskId,
        model_step_id: ModelStepId,
    },
    /// The model answered the request.
    StepEnded {
        turn_task_id: TurnTaskId,
        model_step_id: ModelStepId,
        usage: PluginStepUsage,
    },
    /// A tool the answer calls, one event for each, in the answer's order.
    ToolCall {
        turn_task_id: TurnTaskId,
        model_step_id: ModelStepId,
        tool_call_id: ToolCallId,
        /// The tool's name in Aura (`fs.write`, …).
        tool: String,
    },
    CompactionStarted {
        compaction_id: ContextProjectionSnapshotId,
    },
    /// Once the model's attempts failed, Aura may still finish the compaction with its own
    /// summary: then a `completed` end follows the `failed` one.
    CompactionEnded {
        compaction_id: ContextProjectionSnapshotId,
        outcome: PluginCompactionOutcome,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginSessionEvent {
    /// Its place in the cursor's order.
    pub sequence: u64,
    pub event: PluginSessionEventKind,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginSessionEvents {
    pub events: Vec<PluginSessionEvent>,
    /// Pass it as `after` to read on.
    pub cursor: u64,
    /// There is more after `cursor` already: read again at once.
    pub more: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginSessionSent {
    /// The message, unless this was the same message sent again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<EventId>,
    /// It waits behind the turn under way.
    pub queued: bool,
    /// The same message was sent before; nothing new was queued.
    pub repeated: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginSessionStopped {
    /// A turn was under way and was stopped.
    pub stopped: bool,
}

/// What Main needs to show a plugin's panel: its tab's name, where its files are, and what its
/// page may reach and ask for. Given only while the plugin and its panel can both be used.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginSessionPanel {
    pub title: String,
    /// The plugin's folder. The page is served from its `panel/` folder and nowhere else.
    pub folder: String,
    /// The page that opens first, relative to the folder and inside `panel/`.
    pub entry: String,
    /// The origins (`https://…`, `wss://…`) the page may connect to.
    pub network: Vec<String>,
    /// What the plugin's trusted manifest declares, by capability name.
    pub capabilities: Vec<String>,
}

/// A problem Main saw in a plugin's panel (a page that did not load, a script error, a request
/// the page may not make), kept in the plugin's log.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginPanelLogRequest {
    pub plugin: String,
    pub session_id: SessionId,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginPanelLogged {}

/// The longest notice a plugin may show, and the longest status entry.
pub const PLUGIN_NOTICE_MAX_CHARS: usize = 200;
pub const PLUGIN_STATUS_MAX_CHARS: usize = 40;

/// How a plugin's notice or status entry reads.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginUiTone {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

/// A notice for the user in Aura's window, marked as the plugin's; needs `notify`.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginNoticeRequest {
    pub plugin: String,
    /// One line.
    pub text: String,
    #[serde(default)]
    pub tone: PluginUiTone,
}

/// The notice as it is shown.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginNotice {
    pub plugin: String,
    pub text: String,
    pub tone: PluginUiTone,
}

/// The plugin's one entry in the status bar; needs `status`. No text, or a blank one, clears it.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginStatusRequest {
    pub plugin: String,
    /// One line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default)]
    pub tone: PluginUiTone,
}

/// The entry as it is shown; without text there is none.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginStatus {
    pub plugin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub tone: PluginUiTone,
}

/// What a plugin's background service changed in Aura's window, once Core checked it; Core
/// sends it to Main as the `plugin.ui.v1` notification.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PluginServiceUi {
    Notice {
        notice: PluginNotice,
    },
    /// Its status entry; without text it is cleared.
    Status {
        status: PluginStatus,
    },
    /// The service is gone; its status entry goes with it.
    Ended {
        plugin: String,
    },
    /// Its state on the plugin page changed.
    Service {
        plugin: String,
    },
}

/// Who sent a message that the user did not type.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MessageSender {
    pub plugin: String,
    /// The person on a chat platform, for a message one of the plugin's channels received.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<MessageFrom>,
}

/// A chat on a platform: with one person, or a group.
#[derive(
    Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum PluginChatKind {
    Private,
    Group,
}

/// One chat a plugin's channel is in: the channel, and the platform's own id of the chat.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct PluginChat {
    pub channel: String,
    pub kind: PluginChatKind,
    pub id: String,
}

/// Who on a chat platform sent a message one of a plugin's channels received.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MessageFrom {
    pub channel: String,
    /// The channel's platform, as the plugin declares it (`qq`, `telegram`, …).
    pub platform: String,
    pub chat: PluginChatKind,
    /// The platform's own id of the person.
    pub user_id: String,
    /// The name the platform gave them; may be empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user_name: String,
}
