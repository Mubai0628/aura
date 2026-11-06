//! A plugin's channels: a message one of them passes in, the command it may start with, the
//! conversation it goes to inside the plugin's scope, the reply Aura sends back once the turn
//! that took it ended, shaped for the channel, and the approvals its private chats decide.
mod approvals;
mod choose;
mod commands;
mod incoming;
mod outgoing;
mod replies;

pub use approvals::{
    ChatApproval, Handed, REMOTE_APPROVALS_MAX, REMOTE_DECISION_DELAY, Refused, RemoteApprovals,
    RemoteDecision, check_decision, check_widening, handed_text, remote_decisions,
};
pub use choose::{Choice, Known, choose, in_scope};
pub use commands::{
    Remote, RemoteCommand, SEEN_COMMAND_TIME, SEEN_COMMANDS_MAX, SeenCommands, Typed, expanded,
    help_text, menu, remote_command, typed,
};
pub use incoming::{Incoming, incoming};
pub use outgoing::{outgoing, plain_text, split_text};
pub use replies::{
    PENDING_REPLIES_MAX, PENDING_REPLY_TIME, Pending, PendingReplies, Waiting, reply_for,
};
