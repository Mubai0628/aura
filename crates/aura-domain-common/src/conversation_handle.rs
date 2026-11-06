//! How one conversation names another in what it keeps and shows the model: `conv_` and the
//! first 16 hexadecimal digits of a SHA-256 digest of the other's id. Deleting a Session finds
//! its records by its id, so a record of one conversation that named another by id would go
//! with the other; one that names it by this handle stays with its own conversation.
use std::{borrow::Cow, fmt};

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use crate::{ContentHash, SessionId};

const PREFIX: &str = "conv_";
const DIGITS: usize = 16;

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ConversationHandle(String);

impl ConversationHandle {
    /// The handle of the conversation `session`; the same every time.
    #[must_use]
    pub fn of(session: SessionId) -> Self {
        let digest =
            ContentHash::sha256(format!("aura.conversation-handle.v1:{session}").as_bytes());
        Self(format!("{PREFIX}{}", &digest.hex[..DIGITS]))
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let digits = value.strip_prefix(PREFIX)?;
        (digits.len() == DIGITS
            && digits.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')))
        .then(|| Self(value.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ConversationHandle {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).ok_or_else(|| {
            D::Error::custom("a conversation handle is conv_ and 16 lowercase hexadecimal digits")
        })
    }
}

impl JsonSchema for ConversationHandle {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ConversationHandle")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": 21,
            "maxLength": 21,
            "pattern": "^conv_[0-9a-f]{16}$"
        })
    }
}

impl fmt::Debug for ConversationHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("ConversationHandle").field(&self.0).finish()
    }
}

impl fmt::Display for ConversationHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::ConversationHandle;
    use crate::SessionId;

    #[test]
    fn a_handle_is_the_same_for_a_session_and_never_holds_its_id() {
        let session = SessionId::new();
        let handle = ConversationHandle::of(session);
        assert_eq!(handle, ConversationHandle::of(session));
        assert_ne!(handle, ConversationHandle::of(SessionId::new()));
        let text = handle.to_string();
        assert!(text.starts_with("conv_") && text.len() == 21, "{text}");
        let id = session.to_string();
        assert!(!text.contains(&id) && !text.contains(&id["ses_".len()..]));
        let read: ConversationHandle =
            serde_json::from_value(serde_json::to_value(&handle).expect("json")).expect("handle");
        assert_eq!(read, handle);
        for wrong in ["conv_0123", "conv_0123456789ABCDEF", "ses_0123456789abcdef", &id] {
            assert!(serde_json::from_value::<ConversationHandle>(wrong.into()).is_err(), "{wrong}");
        }
    }
}
