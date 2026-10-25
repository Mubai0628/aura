use std::{fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;

use crate::{CredentialId, MutationJournalId, Revision};

const PREFIX: &str = "wincred://aura/";
const MAX_REFERENCE_LENGTH: usize = 512;
const MAX_SEGMENTS: usize = 8;
const MAX_SEGMENT_LENGTH: usize = 64;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CredentialRefParseError {
    #[error("CredentialRef must begin with '{PREFIX}'")]
    InvalidScheme,
    #[error("CredentialRef must contain between two and {MAX_SEGMENTS} path segments")]
    InvalidSegmentCount,
    #[error(
        "CredentialRef path segments must be canonical lowercase ASCII identifiers of at most {MAX_SEGMENT_LENGTH} characters"
    )]
    InvalidSegment,
    #[error("CredentialRef exceeds the maximum supported length")]
    TooLong,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CredentialRevisionError {
    #[error("credential revision must be at least 1")]
    Zero,
    #[error("credential revision overflow")]
    Overflow,
}

/// Secret-generation revision for one stable CredentialId.
///
/// This is intentionally distinct from a row revision. Clear and metadata-only
/// mutations may change row concurrency without creating a new secret
/// generation.
#[derive(Clone, Copy, Debug, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd)]
pub struct CredentialRevision(Revision);

impl CredentialRevision {
    pub const INITIAL: Self = Self(Revision::INITIAL);

    pub fn new(revision: Revision) -> Result<Self, CredentialRevisionError> {
        if revision.value() == 0 {
            return Err(CredentialRevisionError::Zero);
        }
        Ok(Self(revision))
    }

    #[must_use]
    pub const fn revision(self) -> Revision {
        self.0
    }

    #[must_use]
    pub const fn value(self) -> u64 {
        self.0.value()
    }

    pub fn next(self) -> Result<Self, CredentialRevisionError> {
        self.value()
            .checked_add(1)
            .map(Revision::new)
            .map(Self)
            .ok_or(CredentialRevisionError::Overflow)
    }
}

impl fmt::Display for CredentialRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Serialize for CredentialRevision {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CredentialRevision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let revision = Revision::deserialize(deserializer)?;
        Self::new(revision).map_err(D::Error::custom)
    }
}

/// An opaque reference to a secret stored in Windows Credential Manager.
///
/// The reference is safe to persist and log. The secret value it names is not.
#[derive(Clone, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd)]
#[schemars(with = "String")]
pub struct CredentialRef(String);

impl CredentialRef {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn path(&self) -> &str {
        self.0.strip_prefix(PREFIX).unwrap_or_default()
    }

    #[must_use]
    pub fn windows_target_name(&self) -> String {
        format!("Aura/{}", self.path())
    }

    pub fn for_test(run_namespace: &str, name: &str) -> Result<Self, CredentialRefParseError> {
        format!("{PREFIX}test/{run_namespace}/{name}").parse()
    }
}

impl fmt::Debug for CredentialRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("CredentialRef").field(&self.0).finish()
    }
}

impl fmt::Display for CredentialRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for CredentialRef {
    type Err = CredentialRefParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() > MAX_REFERENCE_LENGTH {
            return Err(CredentialRefParseError::TooLong);
        }
        let path = value.strip_prefix(PREFIX).ok_or(CredentialRefParseError::InvalidScheme)?;
        let segments = path.split('/').collect::<Vec<_>>();
        if !(2..=MAX_SEGMENTS).contains(&segments.len()) {
            return Err(CredentialRefParseError::InvalidSegmentCount);
        }
        if !segments.iter().all(|segment| {
            !segment.is_empty()
                && !matches!(*segment, "." | "..")
                && segment.len() <= MAX_SEGMENT_LENGTH
                && (segment.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'-' | b'_' | b'.')
                }) || segment.parse::<CredentialId>().is_ok()
                    || segment.parse::<MutationJournalId>().is_ok())
        }) {
            return Err(CredentialRefParseError::InvalidSegment);
        }
        Ok(Self(value.to_owned()))
    }
}

impl Serialize for CredentialRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CredentialRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?.parse().map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use crate::{CredentialId, Revision};

    use super::{CredentialRef, CredentialRevision};

    #[test]
    fn accepts_a_canonical_reference() {
        let reference: CredentialRef = "wincred://aura/mcp/mcp_github".parse().expect("valid ref");
        assert_eq!(reference.windows_target_name(), "Aura/mcp/mcp_github");
    }

    #[test]
    fn rejects_noncanonical_or_ambiguous_references() {
        assert!("wincred://aura/MCP/github".parse::<CredentialRef>().is_err());
        assert!("wincred://aura/mcp/./github".parse::<CredentialRef>().is_err());
        assert!("wincred://aura/mcp/../github".parse::<CredentialRef>().is_err());
        assert!("file://aura/mcp/github".parse::<CredentialRef>().is_err());
    }

    #[test]
    fn preserves_a_canonical_credential_id_target_segment() {
        let credential_id = CredentialId::new();
        let reference: CredentialRef =
            format!("wincred://aura/0123456789abcdef/credential/{credential_id}")
                .parse()
                .expect("credential identity segment");
        assert_eq!(
            reference.windows_target_name(),
            format!("Aura/0123456789abcdef/credential/{credential_id}")
        );
    }

    #[test]
    fn credential_revision_is_a_non_zero_revision_wire_string() {
        let revision = CredentialRevision::new(Revision::new(4)).expect("credential revision");
        assert_eq!(revision.to_string(), "4");
        assert_eq!(serde_json::to_string(&revision).expect("serialize"), "\"4\"");
        assert_eq!(
            serde_json::from_str::<CredentialRevision>("\"4\"").expect("deserialize"),
            revision
        );
        assert!(CredentialRevision::new(Revision::new(0)).is_err());
        assert!(serde_json::from_str::<CredentialRevision>("\"0\"").is_err());
        assert_eq!(revision.next().expect("next").value(), 5);
    }
}
