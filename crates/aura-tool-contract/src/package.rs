use std::{borrow::Cow, fmt, str::FromStr};

use aura_domain_common::{ContentHash, ContentHashAlgorithm, UtcTimestamp};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::PackageRelativePath;
use aura_domain_common::CanonicalSemVer;
use aura_domain_common::jcs::{CanonicalBlake3HashV1, CanonicalSha256HashV1};
use aura_domain_common::{ToolId, ToolPackageId};

pub const TOOL_PACKAGE_MANIFEST_NAME: &str = "aura-tool-package.json";
pub const PACKAGE_RUNTIME_CLOSURE_POLICY_VERSION_V1: u32 = 1;
/// `max_entries` includes the root `aura-tool-package.json`, so an authored
/// `files[]` can contain at most one fewer entry.
pub const MAX_TOOL_PACKAGE_DECLARED_FILES_V1: usize = 4_095;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolPackageImportPolicyV1 {
    pub max_zip_input_bytes: u64,
    pub max_expanded_bytes: u64,
    pub max_entries: u32,
    pub max_file_bytes: u64,
    pub max_single_entry_compression_ratio: u32,
    pub max_aggregate_compression_ratio: u32,
    pub max_relative_depth: u32,
    pub max_manifest_bytes: u64,
}

pub const TOOL_PACKAGE_IMPORT_POLICY_V1: ToolPackageImportPolicyV1 = ToolPackageImportPolicyV1 {
    max_zip_input_bytes: 64 * 1024 * 1024,
    max_expanded_bytes: 256 * 1024 * 1024,
    max_entries: 4_096,
    max_file_bytes: 64 * 1024 * 1024,
    max_single_entry_compression_ratio: 100,
    max_aggregate_compression_ratio: 100,
    max_relative_depth: 32,
    max_manifest_bytes: 1024 * 1024,
};

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolPackageContractError {
    #[error("package_key must be 1..=128 ASCII bytes and use the frozen syntax")]
    InvalidPackageKey,
    #[error("tool_key must be 1..=64 ASCII bytes and use the frozen syntax")]
    InvalidToolKey,
    #[error("package file entries must be strictly ordered by canonical path")]
    InvalidFileOrder,
    #[error("package file hash must use SHA-256")]
    InvalidFileHash,
    #[error("package manifest must not list itself")]
    ManifestListedAsFile,
    #[error("package file list exceeds the fixed import policy")]
    FilePolicyExceeded,
    #[error("tool conformance case name must be non-blank safe single-line text")]
    InvalidConformanceCaseName,
}

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PackageKey(String);

impl PackageKey {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolPackageContractError> {
        let value = value.into();
        valid_key(&value, 128, b'.')
            .then_some(Self(value))
            .ok_or(ToolPackageContractError::InvalidPackageKey)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl JsonSchema for PackageKey {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PackageKey")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": 128,
            "pattern": "^[a-z0-9]+(?:[.-][a-z0-9]+)*$"
        })
    }
}

impl fmt::Debug for PackageKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("PackageKey").field(&self.0).finish()
    }
}

impl fmt::Display for PackageKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for PackageKey {
    type Err = ToolPackageContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for PackageKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ToolKey(String);

impl ToolKey {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolPackageContractError> {
        let value = value.into();
        valid_key(&value, 64, b'_')
            .then_some(Self(value))
            .ok_or(ToolPackageContractError::InvalidToolKey)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl JsonSchema for ToolKey {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ToolKey")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": 64,
            "pattern": "^[a-z0-9]+(?:[-_][a-z0-9]+)*$"
        })
    }
}

impl fmt::Debug for ToolKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("ToolKey").field(&self.0).finish()
    }
}

impl fmt::Display for ToolKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for ToolKey {
    type Err = ToolPackageContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for ToolKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Stable, model-independent name of one Package-owned Conformance case.
///
/// This lives in the low-level Tool Contract crate because P04 must bind an
/// uninstalled Draft Conformance run without depending on the higher-level
/// Package domain crate.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ToolPackageConformanceCaseNameV1(String);

impl ToolPackageConformanceCaseNameV1 {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolPackageContractError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 256
            && !value.trim().is_empty()
            && !value.chars().any(|character| character <= '\u{001f}' || character == '\u{007f}');
        valid.then_some(Self(value)).ok_or(ToolPackageContractError::InvalidConformanceCaseName)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl JsonSchema for ToolPackageConformanceCaseNameV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ToolPackageConformanceCaseNameV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": 256,
            "pattern": "^[^\\u0000-\\u001f\\u007f]+$"
        })
    }
}

impl fmt::Debug for ToolPackageConformanceCaseNameV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("ToolPackageConformanceCaseNameV1").field(&self.0).finish()
    }
}

impl fmt::Display for ToolPackageConformanceCaseNameV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for ToolPackageConformanceCaseNameV1 {
    type Err = ToolPackageContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for ToolPackageConformanceCaseNameV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

fn valid_key(value: &str, maximum: usize, second_separator: u8) -> bool {
    if value.is_empty() || value.len() > maximum || !value.is_ascii() {
        return false;
    }
    let bytes = value.as_bytes();
    if !bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit() {
        return false;
    }
    if !bytes[bytes.len() - 1].is_ascii_lowercase() && !bytes[bytes.len() - 1].is_ascii_digit() {
        return false;
    }
    bytes.windows(2).all(|pair| {
        let separator = |byte| byte == b'-' || byte == second_separator;
        !(separator(pair[0]) && separator(pair[1]))
    }) && bytes.iter().all(|byte| {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || *byte == b'-'
            || *byte == second_separator
    })
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolPackageFileRoleV1 {
    RuntimeExecutable,
    RuntimeScript,
    RuntimeLibrary,
    RuntimeResource,
    ConformanceFixture,
    Documentation,
    License,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageFileSha256V1 {
    algorithm: ContentHashAlgorithm,
    hex: String,
}

impl ToolPackageFileSha256V1 {
    pub fn new(hex: impl Into<String>) -> Result<Self, ToolPackageContractError> {
        let hex = hex.into();
        let valid = hex.len() == 64
            && hex.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'));
        valid
            .then_some(Self { algorithm: ContentHashAlgorithm::Sha256, hex })
            .ok_or(ToolPackageContractError::InvalidFileHash)
    }

    #[must_use]
    pub fn sha256(bytes: &[u8]) -> Self {
        Self {
            algorithm: ContentHashAlgorithm::Sha256,
            hex: format!("{:x}", Sha256::digest(bytes)),
        }
    }

    #[must_use]
    pub const fn algorithm(&self) -> ContentHashAlgorithm {
        self.algorithm
    }

    #[must_use]
    pub fn hex(&self) -> &str {
        &self.hex
    }
}

impl PartialEq<ContentHash> for ToolPackageFileSha256V1 {
    fn eq(&self, other: &ContentHash) -> bool {
        other.algorithm == ContentHashAlgorithm::Sha256 && self.hex() == other.hex
    }
}

impl PartialEq<ToolPackageFileSha256V1> for ContentHash {
    fn eq(&self, other: &ToolPackageFileSha256V1) -> bool {
        other == self
    }
}

impl<'de> Deserialize<'de> for ToolPackageFileSha256V1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireHash {
            algorithm: ContentHashAlgorithm,
            hex: String,
        }

        let wire = WireHash::deserialize(deserializer)?;
        if wire.algorithm != ContentHashAlgorithm::Sha256 {
            return Err(D::Error::custom("package file hash algorithm must be sha256"));
        }
        Self::new(wire.hex).map_err(D::Error::custom)
    }
}

impl JsonSchema for ToolPackageFileSha256V1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ToolPackageFileSha256V1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "algorithm": { "type": "string", "const": "sha256" },
                "hex": {
                    "type": "string",
                    "minLength": 64,
                    "maxLength": 64,
                    "pattern": "^[0-9a-f]{64}$"
                }
            },
            "required": ["algorithm", "hex"]
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageFileEntryV1 {
    pub path: PackageRelativePath,
    #[schemars(range(min = 0, max = 67_108_864))]
    pub byte_length: u64,
    pub sha256: ToolPackageFileSha256V1,
    pub role: ToolPackageFileRoleV1,
}

pub fn validate_closed_file_manifest(
    files: &[ToolPackageFileEntryV1],
) -> Result<(), ToolPackageContractError> {
    if !files
        .windows(2)
        .all(|pair| pair[0].path.as_str().as_bytes() < pair[1].path.as_str().as_bytes())
    {
        return Err(ToolPackageContractError::InvalidFileOrder);
    }
    let mut total = 0_u64;
    for file in files {
        if file.path.as_str().eq_ignore_ascii_case(TOOL_PACKAGE_MANIFEST_NAME) {
            return Err(ToolPackageContractError::ManifestListedAsFile);
        }
        if file.byte_length > TOOL_PACKAGE_IMPORT_POLICY_V1.max_file_bytes {
            return Err(ToolPackageContractError::FilePolicyExceeded);
        }
        total = total
            .checked_add(file.byte_length)
            .ok_or(ToolPackageContractError::FilePolicyExceeded)?;
    }
    if total > TOOL_PACKAGE_IMPORT_POLICY_V1.max_expanded_bytes
        || files.len() > MAX_TOOL_PACKAGE_DECLARED_FILES_V1
    {
        return Err(ToolPackageContractError::FilePolicyExceeded);
    }
    Ok(())
}

/// Core-owned V1 runtime visibility policy. This is intentionally not a
/// Package Manifest field: every V1 runtime sees the complete immutable
/// Runtime Root for its exact PackageVersion.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackageRuntimeVisibilityPolicyV1;

impl Serialize for PackageRuntimeVisibilityPolicyV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str("full_package")
    }
}

impl<'de> Deserialize<'de> for PackageRuntimeVisibilityPolicyV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value != "full_package" {
            return Err(D::Error::custom("PackageRuntimeVisibilityPolicyV1 must be full_package"));
        }
        Ok(Self)
    }
}

impl JsonSchema for PackageRuntimeVisibilityPolicyV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PackageRuntimeVisibilityPolicyV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "const": "full_package"
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackageRuntimeClosurePolicyVersionV1;

impl Serialize for PackageRuntimeClosurePolicyVersionV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u32(PACKAGE_RUNTIME_CLOSURE_POLICY_VERSION_V1)
    }
}

impl<'de> Deserialize<'de> for PackageRuntimeClosurePolicyVersionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        if value != PACKAGE_RUNTIME_CLOSURE_POLICY_VERSION_V1 {
            return Err(D::Error::custom("PackageRuntimeClosurePolicyVersionV1 must be 1"));
        }
        Ok(Self)
    }
}

impl JsonSchema for PackageRuntimeClosurePolicyVersionV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PackageRuntimeClosurePolicyVersionV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "integer",
            "const": 1
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PackageRuntimeClosureHashV1(pub CanonicalSha256HashV1);

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PackageFingerprintV1(pub CanonicalSha256HashV1);

/// Installation-independent hash of one Package-authored Tool contract. Under
/// V1 `full_package`, every Tool fingerprint references the same authoritative
/// `PackageRuntimeClosureHashV1` produced by the W2 staging evidence chain.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolContractFingerprintV1 {
    pub package_runtime_closure_hash: PackageRuntimeClosureHashV1,
    pub fingerprint: CanonicalSha256HashV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PackageSecurityFingerprintV1(pub CanonicalBlake3HashV1);

impl PackageSecurityFingerprintV1 {
    #[must_use]
    pub fn uses_blake3(&self) -> bool {
        self.0.algorithm() == ContentHashAlgorithm::Blake3
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageVersionRefV1 {
    pub tool_package_id: ToolPackageId,
    pub package_key: PackageKey,
    pub package_version: CanonicalSemVer,
    pub package_fingerprint: PackageFingerprintV1,
    pub security_fingerprint: PackageSecurityFingerprintV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageToolIdentityV1 {
    pub tool_key: ToolKey,
    pub tool_id: ToolId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolTombstoneV1 {
    pub tool_id: ToolId,
    pub deleted_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageTombstoneV1 {
    pub tool_package_id: ToolPackageId,
    pub package_key: PackageKey,
    pub deleted_at: UtcTimestamp,
}

#[cfg(test)]
mod tests {
    use aura_domain_common::ContentHashAlgorithm;
    use serde_json::json;

    use super::{
        MAX_TOOL_PACKAGE_DECLARED_FILES_V1, PackageKey, PackageRuntimeClosurePolicyVersionV1,
        PackageRuntimeVisibilityPolicyV1, TOOL_PACKAGE_IMPORT_POLICY_V1, ToolKey,
        ToolPackageContractError, ToolPackageFileEntryV1, ToolPackageFileRoleV1,
        ToolPackageFileSha256V1, validate_closed_file_manifest,
    };

    #[test]
    fn package_and_tool_keys_follow_distinct_frozen_grammars() {
        assert!("org.example-tool".parse::<PackageKey>().is_ok());
        assert!("read_file-v2".parse::<ToolKey>().is_ok());
        assert!("org_example".parse::<PackageKey>().is_err());
        assert!("read.file".parse::<ToolKey>().is_err());
    }

    #[test]
    fn manifest_is_closed_ordered_and_sha256_only() {
        let file_hash = ToolPackageFileSha256V1::sha256(b"x");
        assert_eq!(file_hash.algorithm(), ContentHashAlgorithm::Sha256);
        assert_eq!(file_hash.hex().len(), 64);
        let entry = |path: &str| ToolPackageFileEntryV1 {
            path: path.parse().expect("path"),
            byte_length: 1,
            sha256: ToolPackageFileSha256V1::sha256(b"x"),
            role: ToolPackageFileRoleV1::RuntimeResource,
        };
        validate_closed_file_manifest(&[entry("a"), entry("b")]).expect("valid files");
        assert_eq!(
            validate_closed_file_manifest(&[entry("b"), entry("a")]),
            Err(ToolPackageContractError::InvalidFileOrder)
        );
        assert_eq!(TOOL_PACKAGE_IMPORT_POLICY_V1.max_entries, 4_096);
        assert_eq!(MAX_TOOL_PACKAGE_DECLARED_FILES_V1, 4_095);
        assert!(
            serde_json::from_value::<ToolPackageFileSha256V1>(json!({
                "algorithm": "blake3",
                "hex": "00".repeat(32)
            }))
            .is_err()
        );
    }

    #[test]
    fn runtime_visibility_freezes_full_package_v1() {
        assert_eq!(
            serde_json::to_value(PackageRuntimeVisibilityPolicyV1).expect("visibility wire"),
            json!("full_package")
        );
        assert!(
            serde_json::from_value::<PackageRuntimeVisibilityPolicyV1>(json!("restricted_view"))
                .is_err()
        );
        assert_eq!(
            serde_json::to_value(PackageRuntimeClosurePolicyVersionV1).expect("policy wire"),
            json!(1)
        );
    }
}
