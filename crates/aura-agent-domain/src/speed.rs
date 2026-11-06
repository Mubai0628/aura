//! The speed mode the user chose in the composer for the turn a message starts, by the name the
//! model's catalog entry gives it, such as `fast`. None is the standard mode. Each conversation
//! remembers its own choice per model and sends it with the message; Core sends the mode only
//! when the model's settings allow speed modes and its catalog entry offers that one.
//!
//! A name is 1 to 64 ASCII letters, digits, `-`, `_` or `.`: the words the model domain takes as
//! a mode's name (`is_speed_mode_word`), which this kernel crate cannot name; Core converts one
//! into the other.
use std::{borrow::Cow, fmt};

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct TurnSpeedMode(String);

impl TurnSpeedMode {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        ((1..=64).contains(&value.len())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
        .then(|| Self(value.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for TurnSpeedMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).ok_or_else(|| {
            D::Error::custom("a speed mode is 1 to 64 ASCII letters, digits, '-', '_' or '.'")
        })
    }
}

impl JsonSchema for TurnSpeedMode {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("TurnSpeedMode")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": 64,
            "pattern": "^[A-Za-z0-9._-]{1,64}$"
        })
    }
}

impl fmt::Debug for TurnSpeedMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("TurnSpeedMode").field(&self.0).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::TurnSpeedMode;

    #[test]
    fn a_mode_is_a_plain_word_and_reads_back_as_written() {
        for word in ["fast", "ultrafast", "fast-mode-2026-02-01", "v1.2_beta", &"a".repeat(64)] {
            let mode = TurnSpeedMode::parse(word).expect(word);
            assert_eq!(mode.as_str(), word);
            let written = serde_json::to_value(&mode).expect("JSON");
            assert_eq!(written, serde_json::Value::from(word));
            assert_eq!(serde_json::from_value::<TurnSpeedMode>(written).expect("read"), mode);
        }
        for word in
            ["", " fast", "fast mode", "fast\r\nx-evil: 1", "\"fast\"", "速", &"a".repeat(65)]
        {
            assert!(TurnSpeedMode::parse(word).is_none(), "{word:?}");
            assert!(
                serde_json::from_value::<TurnSpeedMode>(word.into()).is_err(),
                "{word:?} is refused when read"
            );
        }
    }
}
