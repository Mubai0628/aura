//! A field that must be present and may be null.
//!
//! Serde reads an omitted `Option` field as `None`. When a record's null is a recorded fact, not a
//! default, the field names both halves of this module, so an omitted field is refused in Rust and
//! in the generated schema instead of being read as absent:
//!
//! ```ignore
//! #[serde(deserialize_with = "aura_domain_common::nullable::required")]
//! #[schemars(with = "aura_domain_common::nullable::Required<ToolId>", required)]
//! pub tool_id: Option<ToolId>,
//! ```
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer};
use std::{borrow::Cow, marker::PhantomData};

/// Reads `T` or null. Naming a `deserialize_with` function turns off serde's omission default.
pub fn required<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

/// The schema of such a field: `T` or null, inlined where the field is declared.
pub struct Required<T>(PhantomData<T>);

impl<T: JsonSchema> JsonSchema for Required<T> {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        format!("Required_{}_OrNull", T::schema_name()).into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let inner = generator.subschema_for::<T>();
        json_schema!({ "anyOf": [inner, { "type": "null" }] })
    }
}
