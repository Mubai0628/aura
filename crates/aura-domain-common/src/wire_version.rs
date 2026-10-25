use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use std::borrow::Cow;

/// Numeric marker used where a frozen P07 wire requires the literal value 1.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ToolWireVersionV1;

impl Serialize for ToolWireVersionV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(1)
    }
}

impl<'de> Deserialize<'de> for ToolWireVersionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match u8::deserialize(deserializer)? {
            1 => Ok(Self),
            _ => Err(D::Error::custom("the P07 wire version must be exactly 1")),
        }
    }
}

impl JsonSchema for ToolWireVersionV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ToolWireVersionV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "integer", "const": 1 })
    }
}
