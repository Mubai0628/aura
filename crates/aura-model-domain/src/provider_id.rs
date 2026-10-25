use std::{borrow::Cow, fmt, str::FromStr};

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;

use aura_domain_common::ToolCallId;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("provider tool call ID must be a non-blank string")]
pub struct ProviderToolCallIdError;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("Aura provider wire tool call ID must use aura_ plus a lowercase 26-character ULID")]
pub struct ProviderWireToolCallIdError;

/// The provider-issued tool call identity, preserved byte-for-byte as UTF-8 text.
///
/// This is intentionally not an Aura ULID and must never be used as an internal
/// database primary key, permission identity, or correlation identity.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProviderToolCallId(String);

impl ProviderToolCallId {
    pub fn new(value: impl Into<String>) -> Result<Self, ProviderToolCallIdError> {
        let value = value.into();
        (!value.trim().is_empty()).then_some(Self(value)).ok_or(ProviderToolCallIdError)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for ProviderToolCallId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for ProviderToolCallId {
    type Err = ProviderToolCallIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl Serialize for ProviderToolCallId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ProviderToolCallId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

impl JsonSchema for ProviderToolCallId {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ProviderToolCallId")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": r"\S"
        })
    }
}

/// An Aura-generated tool call identity used only on a Provider wire when an
/// opaque Provider-issued identity cannot be reused.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProviderWireToolCallId(String);

impl ProviderWireToolCallId {
    #[must_use]
    pub fn from_tool_call_id(tool_call_id: ToolCallId) -> Self {
        Self(format!("aura_{}", tool_call_id.ulid().to_string().to_ascii_lowercase()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProviderWireToolCallId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for ProviderWireToolCallId {
    type Err = ProviderWireToolCallIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let payload = value.strip_prefix("aura_").ok_or(ProviderWireToolCallIdError)?;
        if payload.len() != 26
            || !payload.bytes().all(|byte| {
                byte.is_ascii_digit()
                    || matches!(
                        byte,
                        b'a'..=b'h' | b'j'..=b'k' | b'm'..=b'n' | b'p'..=b't' | b'v'..=b'z'
                    )
            })
        {
            return Err(ProviderWireToolCallIdError);
        }
        Ok(Self(value.to_owned()))
    }
}

impl Serialize for ProviderWireToolCallId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ProviderWireToolCallId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(D::Error::custom)
    }
}

impl JsonSchema for ProviderWireToolCallId {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ProviderWireToolCallId")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": r"^aura_[0-9abcdefghjkmnpqrstvwxyz]{26}$"
        })
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use aura_domain_common::ToolCallId;

    use super::{ProviderToolCallId, ProviderWireToolCallId};

    #[test]
    fn preserves_provider_identity_exactly() {
        let value = "  Call_MixedCase-01  ";
        let id = ProviderToolCallId::new(value).expect("provider ID");
        assert_eq!(id.as_str(), value);
    }

    #[test]
    fn rejects_empty_and_blank_values() {
        assert!(ProviderToolCallId::new("").is_err());
        assert!(ProviderToolCallId::new(" \r\n\t ").is_err());
    }

    #[test]
    fn aura_wire_id_is_distinct_and_deterministic() {
        let tool_call_id =
            ToolCallId::from_str("tcl_01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("canonical tool call ID");
        let wire_id = ProviderWireToolCallId::from_tool_call_id(tool_call_id);
        assert_eq!(wire_id.as_str(), "aura_01arz3ndektsv4rrffq69g5fav");
        assert_eq!(wire_id.as_str().parse::<ProviderWireToolCallId>(), Ok(wire_id));
        assert!("Call_Provider".parse::<ProviderWireToolCallId>().is_err());
    }
}
