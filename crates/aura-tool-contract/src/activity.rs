use std::{borrow::Cow, fmt};

use aura_domain_common::{SessionId, SessionSequence};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;

use aura_domain_common::jcs::CanonicalSha256HashV1;
use aura_domain_common::{ToolCallId, ToolWireVersionV1};

pub const DEFAULT_TOOL_ACTIVITY_PAGE_SIZE_V1: u16 = 50;
pub const MAX_TOOL_ACTIVITY_PAGE_SIZE_V1: u16 = 100;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolActivityContractErrorV1 {
    #[error("tool activity cursor is invalid")]
    InvalidCursor,
    #[error("tool activity page size must be in 1..=100")]
    InvalidPageSize,
}

#[derive(Clone, Eq, Hash, JsonSchema, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ToolActivityCursorV1(#[schemars(with = "String")] String);

impl ToolActivityCursorV1 {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolActivityContractErrorV1> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 4_096
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
        valid.then_some(Self(value)).ok_or(ToolActivityContractErrorV1::InvalidCursor)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ToolActivityCursorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("ToolActivityCursorV1").field(&"<opaque>").finish()
    }
}

impl<'de> Deserialize<'de> for ToolActivityCursorV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ToolActivityCursorKindV1;

impl Serialize for ToolActivityCursorKindV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str("aura.p07.tool_activity_cursor")
    }
}

impl<'de> Deserialize<'de> for ToolActivityCursorKindV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        (value == "aura.p07.tool_activity_cursor")
            .then_some(Self)
            .ok_or_else(|| D::Error::custom("invalid Tool Activity cursor domain"))
    }
}

impl JsonSchema for ToolActivityCursorKindV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ToolActivityCursorKindV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "string", "const": "aura.p07.tool_activity_cursor" })
    }
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolActivityCursorBindingV1 {
    pub kind: ToolActivityCursorKindV1,
    pub cursor_version: ToolWireVersionV1,
    pub session_id: SessionId,
    pub visibility_context_hash: CanonicalSha256HashV1,
    pub cutoff_session_sequence: SessionSequence,
    pub page_size: u16,
    pub last_session_sequence: SessionSequence,
    pub last_tool_call_id: ToolCallId,
    pub ordering_algorithm_version: ToolWireVersionV1,
}

impl ToolActivityCursorBindingV1 {
    pub fn validate(&self) -> Result<(), ToolActivityContractErrorV1> {
        (self.page_size > 0 && self.page_size <= MAX_TOOL_ACTIVITY_PAGE_SIZE_V1)
            .then_some(())
            .ok_or(ToolActivityContractErrorV1::InvalidPageSize)
    }
}

impl<'de> Deserialize<'de> for ToolActivityCursorBindingV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            kind: ToolActivityCursorKindV1,
            cursor_version: ToolWireVersionV1,
            session_id: SessionId,
            visibility_context_hash: CanonicalSha256HashV1,
            cutoff_session_sequence: SessionSequence,
            page_size: u16,
            last_session_sequence: SessionSequence,
            last_tool_call_id: ToolCallId,
            ordering_algorithm_version: ToolWireVersionV1,
        }
        let wire = Wire::deserialize(deserializer)?;
        let value = Self {
            kind: wire.kind,
            cursor_version: wire.cursor_version,
            session_id: wire.session_id,
            visibility_context_hash: wire.visibility_context_hash,
            cutoff_session_sequence: wire.cutoff_session_sequence,
            page_size: wire.page_size,
            last_session_sequence: wire.last_session_sequence,
            last_tool_call_id: wire.last_tool_call_id,
            ordering_algorithm_version: wire.ordering_algorithm_version,
        };
        value.validate().map_err(D::Error::custom)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolActivityPageRequestV1 {
    pub cursor: ToolActivityCursorV1,
}
