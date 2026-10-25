use std::{fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("revision must be a canonical unsigned decimal string")]
pub struct RevisionParseError;

fn parse_decimal(value: &str) -> Result<u64, RevisionParseError> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(RevisionParseError);
    }

    value.parse().map_err(|_| RevisionParseError)
}

macro_rules! define_revision {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd)]
        pub struct $name(#[schemars(with = "String")] u64);

        impl $name {
            #[must_use]
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn value(self) -> u64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = RevisionParseError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                parse_decimal(value).map(Self)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                value.parse().map_err(D::Error::custom)
            }
        }
    };
}

define_revision!(Revision);
define_revision!(ExpectedRevision);
define_revision!(SessionSequence);

impl Revision {
    pub const INITIAL: Self = Self(1);

    #[must_use]
    pub const fn expected(self) -> ExpectedRevision {
        ExpectedRevision::new(self.0)
    }
}
