//! User input routing is independent of execution configuration. Only the
//! authenticated desktop can create these messages; tools cannot choose a lane.
use crate::{AgentError, OriginalUserMessage, UserMessageOrigin};
use aura_domain_common::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "lane", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentInputLane {
    NextTurn,
    Addition { turn_task_id: TurnTaskId },
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentPendingMessage {
    pub message_id: EventId,
    #[serde(default = "initial_message_revision")]
    pub revision: Revision,
    pub text: String,
    pub destination: AgentInputLane,
    pub created_at: UtcTimestamp,
    /// The Skills chosen with this message, as checked when it was sent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<crate::LoadedSkillEntry>,
    /// The thinking level chosen with this message for the turn it starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<crate::TurnReasoningEffort>,
    /// The pictures and files added to this message, kept with it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<crate::AgentMessageAttachment>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentInboxProjection {
    pub session_id: SessionId,
    pub revision: Revision,
    pub paused: bool,
    #[serde(default)]
    pub pause_reason: Option<String>,
    pub messages: Vec<AgentPendingMessage>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentSessionRequest {
    pub session_id: SessionId,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentSubmitRequest {
    pub session_id: SessionId,
    pub text: String,
    pub destination: AgentInputLane,
    /// Skills the user chose for the turn this message starts. Only a message that starts
    /// a new turn may carry them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skill_ids: Vec<SkillInstanceId>,
    /// The thinking level the conversation uses with its model, for the turn this message
    /// starts; none is the model's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<crate::TurnReasoningEffort>,
    /// Pictures and files added to the message in the composer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<crate::AgentAttachmentDraft>,
}

impl AgentSubmitRequest {
    /// A message with no chosen Skills.
    #[must_use]
    pub fn new(
        session_id: SessionId,
        text: impl Into<String>,
        destination: AgentInputLane,
    ) -> Self {
        Self {
            session_id,
            text: text.into(),
            destination,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }
    }

    /// The request as a repeated command is compared and kept: digests in place of what its
    /// pictures and text files hold.
    #[must_use]
    pub fn receipt(&self) -> Self {
        Self {
            attachments: self
                .attachments
                .iter()
                .map(crate::AgentAttachmentDraft::receipt)
                .collect(),
            ..self.clone()
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentInboxAction {
    Edit { message_id: EventId, text: String },
    Remove { message_id: EventId },
    Append { message_id: EventId, turn_task_id: TurnTaskId },
    Revoke { message_id: EventId },
    Pause,
    Resume,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentInboxMutation {
    pub session_id: SessionId,
    pub expected_revision: Revision,
    pub action: AgentInboxAction,
}

/// References remain separate even when several messages start one Turn.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentConsumedInput {
    pub turn_task_id: TurnTaskId,
    pub user_event_ids: Vec<EventId>,
    #[serde(default)]
    pub message_revisions: Vec<(EventId, Revision)>,
    pub destination: AgentInputLane,
    pub consumed_at: UtcTimestamp,
}

fn initial_message_revision() -> Revision {
    Revision::INITIAL
}

impl AgentPendingMessage {
    pub fn freeze(
        &self,
        session: SessionId,
        at: UtcTimestamp,
    ) -> Result<OriginalUserMessage, AgentError> {
        self.freeze_as(session, self.text.clone(), at)
    }

    /// The message as it becomes part of the conversation with `text`, what the user typed
    /// with its attachments as the model reads them ([`crate::message_with_attachments`]).
    pub fn freeze_as(
        &self,
        session: SessionId,
        text: String,
        at: UtcTimestamp,
    ) -> Result<OriginalUserMessage, AgentError> {
        OriginalUserMessage::new(
            self.message_id,
            session,
            text,
            match self.destination {
                AgentInputLane::NextTurn => UserMessageOrigin::UserInput,
                AgentInputLane::Addition { .. } => UserMessageOrigin::Steer,
            },
            at,
        )
    }
}

pub fn validate_agent_input(text: &str) -> Result<(), AgentError> {
    validate_agent_message(text, false)
}

/// A message with pictures or files may have no words of its own.
pub fn validate_agent_message(text: &str, attached: bool) -> Result<(), AgentError> {
    if (text.trim().is_empty() && !attached) || text.len() > 256 * 1024 {
        Err(AgentError::InvalidInput)
    } else {
        Ok(())
    }
}
