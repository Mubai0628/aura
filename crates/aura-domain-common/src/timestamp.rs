use std::{fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;
use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("timestamp must be valid RFC 3339: {message}")]
pub struct TimestampParseError {
    message: String,
}

#[derive(Clone, Copy, Eq, JsonSchema, Ord, PartialEq, PartialOrd)]
pub struct UtcTimestamp(#[schemars(with = "String")] OffsetDateTime);

impl UtcTimestamp {
    #[must_use]
    pub fn now() -> Self {
        Self(OffsetDateTime::now_utc())
    }

    #[must_use]
    pub const fn value(self) -> OffsetDateTime {
        self.0
    }

    #[must_use]
    pub const fn from_value(value: OffsetDateTime) -> Self {
        Self(value)
    }
}

impl fmt::Debug for UtcTimestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("UtcTimestamp").field(&self.to_string()).finish()
    }
}

impl fmt::Display for UtcTimestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let utc = self.0.to_offset(UtcOffset::UTC);
        let formatted = utc.format(&Rfc3339).map_err(|_| fmt::Error)?;
        formatter.write_str(&formatted)
    }
}

impl FromStr for UtcTimestamp {
    type Err = TimestampParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        OffsetDateTime::parse(value, &Rfc3339)
            .map(|timestamp| Self(timestamp.to_offset(UtcOffset::UTC)))
            .map_err(|error| TimestampParseError { message: error.to_string() })
    }
}

impl Serialize for UtcTimestamp {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for UtcTimestamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(D::Error::custom)
    }
}
