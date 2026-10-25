use std::{borrow::Cow, fmt, str::FromStr};

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;

const MAX_SLUG_BYTES: usize = 64;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("model configuration slug must be 1..=64 bytes of lowercase ASCII kebab-case")]
pub struct ModelConfigSlugError;

macro_rules! define_slug {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ModelConfigSlugError> {
                let value = value.into();
                validate_slug(&value)?;
                Ok(Self(value))
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

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = ModelConfigSlugError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                String::deserialize(deserializer)?.parse().map_err(D::Error::custom)
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                Cow::Borrowed(stringify!($name))
            }

            fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
                json_schema!({
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 64,
                    "pattern": r"^[a-z0-9]+(?:-[a-z0-9]+)*$"
                })
            }
        }
    };
}

define_slug!(ProviderSlug);
define_slug!(EndpointSlug);

fn validate_slug(value: &str) -> Result<(), ModelConfigSlugError> {
    if value.is_empty()
        || value.len() > MAX_SLUG_BYTES
        || !value.is_ascii()
        || value.starts_with('-')
        || value.ends_with('-')
        || value.as_bytes().windows(2).any(|pair| pair == b"--")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(ModelConfigSlugError);
    }
    Ok(())
}

/// Produces the frozen ASCII-only slug stem. Uniqueness is applied by the
/// configuration owner because its scope differs for Provider and Endpoint.
#[must_use]
pub fn normalize_slug_stem(value: &str, fallback: &str) -> String {
    let mut stem = String::with_capacity(value.len().min(MAX_SLUG_BYTES));
    let mut pending_separator = false;
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() {
            if pending_separator && !stem.is_empty() {
                stem.push('-');
            }
            stem.push(byte.to_ascii_lowercase() as char);
            pending_separator = false;
        } else {
            pending_separator = true;
        }
    }
    stem.truncate(MAX_SLUG_BYTES);
    while stem.ends_with('-') {
        stem.pop();
    }
    if stem.is_empty() { fallback.to_owned() } else { stem }
}

/// Appends the required deterministic eight-character lowercase ULID suffix
/// while keeping the resulting slug within 64 ASCII bytes.
#[must_use]
pub fn suffix_slug_for_collision(stem: &str, ulid_payload: &str) -> String {
    const SUFFIX_BYTES: usize = 9;
    let suffix = &ulid_payload.to_ascii_lowercase()[ulid_payload.len() - 8..];
    let mut bounded = stem[..stem.len().min(MAX_SLUG_BYTES - SUFFIX_BYTES)].to_owned();
    while bounded.ends_with('-') {
        bounded.pop();
    }
    format!("{bounded}-{suffix}")
}

#[cfg(test)]
mod tests {
    use super::{EndpointSlug, ProviderSlug, normalize_slug_stem, suffix_slug_for_collision};

    #[test]
    fn slugs_enforce_the_frozen_wire_grammar() {
        for valid in ["openai", "chat-completions", "p1"] {
            assert!(valid.parse::<ProviderSlug>().is_ok());
            assert!(valid.parse::<EndpointSlug>().is_ok());
        }
        for invalid in ["", "OpenAI", "open_ai", "-openai", "openai-", "open--ai"] {
            assert!(invalid.parse::<ProviderSlug>().is_err(), "{invalid}");
        }
    }

    #[test]
    fn normalization_and_collision_suffix_are_ascii_and_bounded() {
        assert_eq!(normalize_slug_stem(" Moonshot AI / 月之暗面 ", "provider"), "moonshot-ai");
        assert_eq!(normalize_slug_stem("月之暗面", "provider"), "provider");
        let slug = suffix_slug_for_collision(&"a".repeat(64), "01ARZ3NDEKTSV4RRFFQ69G5FAV");
        assert_eq!(slug.len(), 64);
        assert!(slug.ends_with("-q69g5fav"));
        assert!(slug.parse::<ProviderSlug>().is_ok());
    }
}
