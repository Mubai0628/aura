use aura_domain_common::EventId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const SESSION_REPLY: &str = "session.reply";

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionReplyInput {
    #[schemars(length(min = 1, max = 2000))]
    pub text: String,
}

impl SessionReplyInput {
    #[must_use]
    pub fn valid(&self) -> bool {
        !self.text.trim().is_empty() && self.text.chars().count() <= 2000
    }
}

/// The ID of the message stored in the active conversation.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionReplyReceipt {
    pub event_id: EventId,
}
