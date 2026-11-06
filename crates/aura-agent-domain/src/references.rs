//! What a user message refers to with "@". The composer names it; Core resolves it once when the
//! message is sent and keeps the result with the message, and the result is written after the
//! message's text the same way every time, so every later step sends the same bytes. A reference
//! carries no contents of a file: the model reads what it names with its tools when the task needs
//! it. Another conversation comes with a short recap of where it was when the message was sent,
//! and what is kept names it by its [`ConversationHandle`], never by its id.
use aura_domain_common::{ConversationHandle, SessionId, WorkspacePath};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The most references one message carries.
pub const MAX_MESSAGE_REFERENCES: usize = 20;

/// One thing a message refers to, as the composer sends it.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentMessageReference {
    /// A file or folder of the session's workspace, by its path relative to the workspace root.
    WorkspacePath { path: WorkspacePath },
    /// Another conversation of the same workspace that is not archived, as the composer names it.
    Session { session_id: SessionId },
    /// The same by its handle, as a command's receipt keeps it.
    Conversation { conversation: ConversationHandle },
}

impl AgentMessageReference {
    /// How a command's receipt keeps it: another conversation by its handle.
    #[must_use]
    pub fn receipt(&self) -> Self {
        match self {
            Self::Session { session_id } => {
                Self::Conversation { conversation: ConversationHandle::of(*session_id) }
            }
            other => other.clone(),
        }
    }
}

/// A reference as it was when its message was sent, kept with the message.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResolvedMessageReference {
    /// A file of the workspace.
    WorkspaceFile { path: WorkspacePath },
    /// A folder of the workspace; `.` is its root.
    WorkspaceFolder { path: WorkspacePath },
    /// Another conversation: its handle, its title, and what it was about then, one line each.
    Session { conversation: ConversationHandle, title: String, recap: String },
}

/// What kind of thing a message referred to.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MessageReferenceKind {
    File,
    Folder,
    Session,
}

/// A message's reference as the conversation and the queue show it.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields)]
pub struct MessageReferenceView {
    pub kind: MessageReferenceKind,
    pub label: String,
}

impl ResolvedMessageReference {
    /// How the conversation shows it with its message.
    #[must_use]
    pub fn view(&self) -> MessageReferenceView {
        let (kind, label) = match self {
            Self::WorkspaceFile { path } => (MessageReferenceKind::File, path.to_string()),
            Self::WorkspaceFolder { path } => (MessageReferenceKind::Folder, path.to_string()),
            Self::Session { title, .. } => (MessageReferenceKind::Session, title.clone()),
        };
        MessageReferenceView { kind, label }
    }

    /// The conversation it names, when it names one.
    #[must_use]
    pub fn conversation(&self) -> Option<&ConversationHandle> {
        match self {
            Self::Session { conversation, .. } => Some(conversation),
            _ => None,
        }
    }

    /// The line the model reads for it.
    fn line(&self) -> String {
        match self {
            Self::WorkspaceFile { path } => format!(
                "workspace file `{path}`, not included: read it from the workspace if the task \
                 needs it"
            ),
            Self::WorkspaceFolder { path } if path.is_root() => format!(
                "the workspace root folder `{path}`, not included: list or search it if the task \
                 needs it"
            ),
            Self::WorkspaceFolder { path } => format!(
                "workspace folder `{path}`, not included: list or search it if the task needs it"
            ),
            Self::Session { conversation, title, recap } if recap.trim().is_empty() => {
                format!("conversation \"{title}\" (`{conversation}`), which has no messages yet")
            }
            Self::Session { conversation, title, recap } => {
                let mut line = format!(
                    "conversation \"{title}\" (`{conversation}`), not included: read more of it \
                     with history.list_turns, history.search and history.read, passing \
                     \"conversation\": \"{conversation}\", if the task needs more than its recent \
                     state:"
                );
                for part in recap.lines() {
                    line.push_str("\n  > ");
                    line.push_str(part);
                }
                line
            }
        }
    }
}

/// The message as the model reads it with what it refers to: its text (what the user typed, with
/// its attachments), then one line per reference in the order the message has them. With none it
/// is the text.
#[must_use]
pub fn message_with_references(text: &str, references: &[ResolvedMessageReference]) -> String {
    if references.is_empty() {
        return text.to_owned();
    }
    let mut whole = String::new();
    if !text.trim().is_empty() {
        whole.push_str(text);
        whole.push_str("\n\n");
    }
    whole.push_str("Referred to in this message:");
    for reference in references {
        whole.push_str("\n- ");
        whole.push_str(&reference.line());
    }
    whole
}
