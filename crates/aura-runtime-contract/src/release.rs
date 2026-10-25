use std::{fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use sha2::{Digest, Sha256};
use thiserror::Error;

use aura_domain_common::{ContentHash, ReleaseBundleId};

pub const PRODUCTION_BUNDLE_NAMESPACE: &str = "aura.desktop.production";
const DEVELOPMENT_PREFIX: &str = "aura.desktop.development.";
const TEST_PREFIX: &str = "aura.desktop.test.";
const MAX_NAMESPACE_LENGTH: usize = 128;
pub const NAMESPACE_OBJECT_HASH_BYTES: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseTrustDomain {
    Production,
    Development,
    Test,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum BundleNamespaceParseError {
    #[error("bundle namespace must match ^[a-z0-9][a-z0-9.-]{{0,127}}$")]
    InvalidSyntax,
}

#[derive(Clone, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd)]
#[schemars(with = "String")]
pub struct BundleNamespace(String);

impl BundleNamespace {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn sha256(&self) -> [u8; 32] {
        Sha256::digest(self.0.as_bytes()).into()
    }

    /// Stable Windows object/data partition suffix: the first 128 bits of
    /// SHA-256(bundle_namespace), encoded as 32 lowercase hexadecimal bytes.
    #[must_use]
    pub fn object_name_hash(&self) -> String {
        self.sha256()[..NAMESPACE_OBJECT_HASH_BYTES]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    #[must_use]
    pub fn trust_domain(&self) -> Option<ReleaseTrustDomain> {
        if self.0 == PRODUCTION_BUNDLE_NAMESPACE {
            return Some(ReleaseTrustDomain::Production);
        }
        if let Some(instance_id) = self.0.strip_prefix(DEVELOPMENT_PREFIX)
            && instance_id.len() == 26
            && instance_id.parse::<ulid::Ulid>().is_ok()
        {
            return Some(ReleaseTrustDomain::Development);
        }
        if self.0.strip_prefix(TEST_PREFIX).is_some_and(|run_id| !run_id.is_empty()) {
            return Some(ReleaseTrustDomain::Test);
        }
        None
    }
}

impl fmt::Debug for BundleNamespace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("BundleNamespace").field(&self.0).finish()
    }
}

impl fmt::Display for BundleNamespace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for BundleNamespace {
    type Err = BundleNamespaceParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty()
            || value.len() > MAX_NAMESPACE_LENGTH
            || !value
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
            })
        {
            return Err(BundleNamespaceParseError::InvalidSyntax);
        }
        Ok(Self(value.to_owned()))
    }
}

impl Serialize for BundleNamespace {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for BundleNamespace {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?.parse().map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseIdentity {
    pub release_bundle_id: ReleaseBundleId,
    pub component_manifest_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseDescriptor {
    pub bundle_namespace: BundleNamespace,
    pub identity: ReleaseIdentity,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_syntax_and_domains_are_strict() {
        let production: BundleNamespace = PRODUCTION_BUNDLE_NAMESPACE.parse().expect("production");
        assert_eq!(production.trust_domain(), Some(ReleaseTrustDomain::Production));
        assert_eq!(production.object_name_hash(), "a47182cd8b9177c29a6be19b0c9c32a4");

        let development: BundleNamespace =
            "aura.desktop.development.01arz3ndektsv4rrffq69g5fav".parse().expect("development");
        assert_eq!(development.trust_domain(), Some(ReleaseTrustDomain::Development));

        let test: BundleNamespace = "aura.desktop.test.p04-cms".parse().expect("test");
        assert_eq!(test.trust_domain(), Some(ReleaseTrustDomain::Test));

        for invalid in ["", "Aura.desktop.production", ".aura", "aura/desktop", &"a".repeat(129)] {
            assert!(invalid.parse::<BundleNamespace>().is_err());
        }
        let unsupported: BundleNamespace = "aura.desktop.preview".parse().expect("valid syntax");
        assert_eq!(unsupported.trust_domain(), None);
    }
}
