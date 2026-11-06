//! A plugin's panel acting in the conversation it was opened in: reading its public state and
//! what happened since a cursor, sending a message marked as the plugin's or running one of the
//! plugin's own commands as one, stopping its turn; and reading and setting the plugin's own text
//! settings. Each call is checked against what the plugin's trusted manifest declares before
//! anything is read or done; Core builds this from its Agent application and plugin runtime for
//! each call. Main also asks here what it needs to show the panel, and keeps the panel's problems
//! in the plugin's log. A plugin's background service asks the same of Aura through the host that
//! keeps it running (`background`), within the conversations the user put in its scope, and
//! passes in what its channels receive (`channels`), where a private chat may decide the
//! approvals its turns wait for once the user lets it.
mod background;
mod channels;
mod checks;
mod own;
mod panel;
mod service;

pub use background::{
    AgentAccess, LISTED_SESSIONS, NO_SCOPE_REPLY, PluginServices, SERVICE_NOTICE_WINDOW,
    SERVICE_NOTICES, ServiceBridge, UI_NOTIFICATION, service_identity,
};
pub use channels::{
    ChatApproval, Choice, Handed, Incoming, Known, PENDING_REPLIES_MAX, PENDING_REPLY_TIME,
    Pending, PendingReplies, REMOTE_APPROVALS_MAX, REMOTE_DECISION_DELAY, Refused, Remote,
    RemoteApprovals, RemoteCommand, RemoteDecision, SEEN_COMMAND_TIME, SEEN_COMMANDS_MAX,
    SeenCommands, Typed, Waiting, check_decision, check_widening, choose, expanded, handed_text,
    help_text, in_scope, incoming, menu, outgoing, plain_text, remote_command, remote_decisions,
    reply_for, split_text, typed,
};

pub use checks::{
    Standing, checked_key, checked_notice, checked_status, checked_text, message_command,
    permitted, unusable,
};
pub use own::own_prompt;
pub use panel::panel_site;
pub use service::{PluginSessions, starts_work};
