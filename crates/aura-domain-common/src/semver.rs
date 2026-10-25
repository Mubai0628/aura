use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use std::{borrow::Cow, fmt, str::FromStr};
use thiserror::Error;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("semantic version must be a canonical SemVer 2.0.0 string")]
pub struct CanonicalSemVerError;

#[derive(Clone, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CanonicalSemVer(String);

impl CanonicalSemVer {
    pub fn new(value: impl Into<String>) -> Result<Self, CanonicalSemVerError> {
        let value = value.into();
        if value.len() > 128 || !value.is_ascii() {
            return Err(CanonicalSemVerError);
        }
        let parsed = Version::parse(&value).map_err(|_| CanonicalSemVerError)?;
        if parsed.to_string() != value {
            return Err(CanonicalSemVerError);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl JsonSchema for CanonicalSemVer {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("CanonicalSemVer")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": 5,
            "maxLength": 128,
            "pattern": "^(0|[1-9][0-9]*)[.](0|[1-9][0-9]*)[.](0|[1-9][0-9]*)(?:-(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)(?:[.](?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*))*)?(?:[+][0-9A-Za-z-]+(?:[.][0-9A-Za-z-]+)*)?$"
        })
    }
}

impl fmt::Debug for CanonicalSemVer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("CanonicalSemVer").field(&self.0).finish()
    }
}

impl fmt::Display for CanonicalSemVer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for CanonicalSemVer {
    type Err = CanonicalSemVerError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl TryFrom<String> for CanonicalSemVer {
    type Error = CanonicalSemVerError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for CanonicalSemVer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::CanonicalSemVer;
    use std::str::FromStr;
    #[test]
    fn canonical_semver_requires_exact_semver_two_wire() {
        for value in ["1.2.3", "1.2.3-alpha.1", "1.2.3-alpha.1+build.5"] {
            let parsed = CanonicalSemVer::from_str(value).expect("canonical SemVer");
            assert_eq!(parsed.to_string(), value);
        }
        for value in ["v1.2.3", " 1.2.3", "1.2.3 ", "1.2", "01.2.3", "1.2.3-01"] {
            assert!(CanonicalSemVer::from_str(value).is_err(), "{value} must be rejected");
        }
    }
}
