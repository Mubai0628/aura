use std::io::{self, Read};

use aura_domain_common::ContentHash;
use aura_domain_common::jcs::{
    CanonicalJsonError, LocatedCanonicalJsonError, canonical_json_jcs, parse_json_strict,
    parse_json_strict_located,
};
use aura_tool_contract::{
    TOOL_PACKAGE_IMPORT_POLICY_V1, ToolPackageErrorCode, ToolPackageImportPolicyV1,
};
use aura_tool_domain::{
    CanonicalToolPackageManifestV1, RawToolPackageManifestV1, ToolPackageManifestContractError,
};
use thiserror::Error;

/// A Manifest that completed W1 or was reconstructed from an already stored
/// canonical manifest. Both paths strictly self-validate; restored packages must
/// additionally re-read their complete payload and match the persisted fingerprint.
/// Private fields prevent callers manufacturing parsing evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedToolPackageManifestV1 {
    source_byte_length: u64,
    source_sha256: ContentHash,
    canonical_manifest: CanonicalToolPackageManifestV1,
    canonical_manifest_bytes: Vec<u8>,
}

impl ParsedToolPackageManifestV1 {
    pub(crate) fn from_stored_canonical(
        manifest: &CanonicalToolPackageManifestV1,
    ) -> Result<Self, ToolPackageManifestParseErrorV1> {
        manifest.validate().map_err(ToolPackageManifestParseErrorV1::CanonicalConversion)?;
        let bytes = canonical_json_jcs(manifest)
            .map_err(ToolPackageManifestParseErrorV1::CanonicalSerialization)?;
        if bytes.len() as u64 > TOOL_PACKAGE_IMPORT_POLICY_V1.max_manifest_bytes {
            return Err(ToolPackageManifestParseErrorV1::ManifestTooLarge);
        }
        let value = parse_json_strict(&bytes)
            .map_err(ToolPackageManifestParseErrorV1::CanonicalSelfValidationJson)?;
        let decoded: CanonicalToolPackageManifestV1 = serde_json::from_value(value)
            .map_err(ToolPackageManifestParseErrorV1::CanonicalSelfValidationContract)?;
        if decoded != *manifest {
            return Err(ToolPackageManifestParseErrorV1::CanonicalSelfValidationMismatch);
        }
        Ok(Self {
            source_byte_length: bytes.len() as u64,
            source_sha256: ContentHash::sha256(&bytes),
            canonical_manifest: decoded,
            canonical_manifest_bytes: bytes,
        })
    }
    #[must_use]
    pub const fn source_byte_length(&self) -> u64 {
        self.source_byte_length
    }

    #[must_use]
    pub const fn source_sha256(&self) -> &ContentHash {
        &self.source_sha256
    }

    #[must_use]
    pub const fn canonical_manifest(&self) -> &CanonicalToolPackageManifestV1 {
        &self.canonical_manifest
    }

    #[must_use]
    pub fn canonical_manifest_bytes(&self) -> &[u8] {
        &self.canonical_manifest_bytes
    }
}

#[derive(Debug, Error)]
pub enum ToolPackageManifestParseErrorV1 {
    #[error("tool package manifest could not be read")]
    SourceRead(#[source] io::Error),
    #[error("tool package manifest exceeds the fixed V1 byte limit")]
    ManifestTooLarge,
    #[error("tool package manifest is not strict JSON")]
    StrictJson(#[source] LocatedCanonicalJsonError),
    #[error("tool package manifest does not match the Raw V1 contract")]
    RawContract(#[source] serde_json::Error),
    #[error("tool package manifest violates Raw V1 semantic validation")]
    RawSemantic(#[source] ToolPackageManifestContractError),
    #[error("tool package manifest could not be converted to the Canonical V1 contract")]
    CanonicalConversion(#[source] ToolPackageManifestContractError),
    #[error("canonical tool package manifest serialization failed")]
    CanonicalSerialization(#[source] CanonicalJsonError),
    #[error("canonical tool package manifest failed strict self-validation")]
    CanonicalSelfValidationJson(#[source] CanonicalJsonError),
    #[error("canonical tool package manifest failed typed self-validation")]
    CanonicalSelfValidationContract(#[source] serde_json::Error),
    #[error("canonical tool package manifest changed during self-validation")]
    CanonicalSelfValidationMismatch,
}

impl ToolPackageManifestParseErrorV1 {
    /// Maps internal parser detail to the already frozen public P07 error
    /// catalog without exposing JSON, paths, or source text.
    #[must_use]
    pub const fn public_code(&self) -> ToolPackageErrorCode {
        match self {
            Self::ManifestTooLarge => ToolPackageErrorCode::ManifestTooLarge,
            Self::SourceRead(_)
            | Self::StrictJson(_)
            | Self::RawContract(_)
            | Self::RawSemantic(_)
            | Self::CanonicalConversion(_)
            | Self::CanonicalSerialization(_)
            | Self::CanonicalSelfValidationJson(_)
            | Self::CanonicalSelfValidationContract(_)
            | Self::CanonicalSelfValidationMismatch => ToolPackageErrorCode::ManifestInvalid,
        }
    }
}

/// Reads and validates one untrusted `aura-tool-package.json` using the fixed
/// production policy. No downstream Package gate is invoked by this function.
pub fn parse_tool_package_manifest_v1(
    source: impl Read,
) -> Result<ParsedToolPackageManifestV1, ToolPackageManifestParseErrorV1> {
    parse_tool_package_manifest_with_policy_v1(source, TOOL_PACKAGE_IMPORT_POLICY_V1)
}

pub(crate) fn parse_tool_package_manifest_with_policy_v1(
    source: impl Read,
    policy: ToolPackageImportPolicyV1,
) -> Result<ParsedToolPackageManifestV1, ToolPackageManifestParseErrorV1> {
    let source_bytes = read_manifest_source_bounded(source, policy.max_manifest_bytes)?;

    // This order is security-sensitive and mirrors ADR-0003 exactly. Never
    // replace the strict Value gate with direct typed deserialization.
    parse_json_strict_located(&source_bytes)
        .map_err(ToolPackageManifestParseErrorV1::StrictJson)?;
    // Keep the authored line/column for diagnostics. The strict duplicate/number
    // gate above still runs first; typed decoding cannot bypass that boundary.
    let raw_manifest = serde_json::from_slice::<RawToolPackageManifestV1>(&source_bytes)
        .map_err(ToolPackageManifestParseErrorV1::RawContract)?;
    raw_manifest
        .validate_with_source_byte_length(source_bytes.len())
        .map_err(ToolPackageManifestParseErrorV1::RawSemantic)?;
    let canonical_manifest = raw_manifest
        .into_canonical_contract()
        .map_err(ToolPackageManifestParseErrorV1::CanonicalConversion)?;
    let canonical_manifest_bytes = canonical_json_jcs(&canonical_manifest)
        .map_err(ToolPackageManifestParseErrorV1::CanonicalSerialization)?;

    let canonical_value = parse_json_strict(&canonical_manifest_bytes)
        .map_err(ToolPackageManifestParseErrorV1::CanonicalSelfValidationJson)?;
    let verified_manifest =
        serde_json::from_value::<CanonicalToolPackageManifestV1>(canonical_value)
            .map_err(ToolPackageManifestParseErrorV1::CanonicalSelfValidationContract)?;
    if verified_manifest != canonical_manifest {
        return Err(ToolPackageManifestParseErrorV1::CanonicalSelfValidationMismatch);
    }

    Ok(ParsedToolPackageManifestV1 {
        source_byte_length: u64::try_from(source_bytes.len())
            .map_err(|_| ToolPackageManifestParseErrorV1::ManifestTooLarge)?,
        source_sha256: ContentHash::sha256(&source_bytes),
        canonical_manifest: verified_manifest,
        canonical_manifest_bytes,
    })
}

fn read_manifest_source_bounded(
    source: impl Read,
    maximum_bytes: u64,
) -> Result<Vec<u8>, ToolPackageManifestParseErrorV1> {
    let read_limit =
        maximum_bytes.checked_add(1).ok_or(ToolPackageManifestParseErrorV1::ManifestTooLarge)?;
    let initial_capacity = usize::try_from(read_limit)
        .map_err(|_| ToolPackageManifestParseErrorV1::ManifestTooLarge)?;
    let mut bytes = Vec::with_capacity(initial_capacity);
    source
        .take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(ToolPackageManifestParseErrorV1::SourceRead)?;
    if u64::try_from(bytes.len()).map_or(true, |length| length > maximum_bytes) {
        return Err(ToolPackageManifestParseErrorV1::ManifestTooLarge);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::io::{self, Cursor, Read};

    use aura_domain_common::jcs::{canonical_json_jcs, parse_json_strict};
    use aura_tool_contract::{TOOL_PACKAGE_IMPORT_POLICY_V1, ToolPackageErrorCode};
    use serde_json::{Value, json};

    use super::{ToolPackageManifestParseErrorV1, parse_tool_package_manifest_v1};

    const STDIO_MANIFEST: &[u8] =
        include_bytes!("../../../fixtures/tool-package/manifest-v1/valid/aura-stdio.json");
    const HTTP_MANIFEST: &str =
        include_str!("../../../fixtures/tool-package/manifest-v1/valid/declarative-http.json");
    const COMMAND_MANIFEST: &str =
        include_str!("../../../fixtures/tool-package/manifest-v1/valid/declarative-command.json");

    #[test]
    fn production_parser_returns_only_self_validated_canonical_evidence() {
        let parsed = parse_tool_package_manifest_v1(Cursor::new(STDIO_MANIFEST))
            .expect("parse valid Manifest");

        assert_eq!(
            parsed.source_byte_length(),
            u64::try_from(STDIO_MANIFEST.len()).expect("fixture length")
        );
        assert_eq!(parsed.canonical_manifest().package_key.as_str(), "org.aura.fixture.stdio");
        assert_eq!(
            parsed.canonical_manifest_bytes(),
            canonical_json_jcs(parsed.canonical_manifest()).expect("canonical JCS")
        );
        let value =
            parse_json_strict(parsed.canonical_manifest_bytes()).expect("strict canonical bytes");
        assert_eq!(
            value["tools"][0]["model"]["canonical_name"],
            "user.org.aura.fixture.stdio.fixture"
        );
        assert_eq!(value["tools"][0]["audience"], "subagent_configurable");
        assert!(value["tools"][0].get("semantic_version").is_some());
    }

    #[test]
    fn manifest_size_gate_runs_before_json_parsing() {
        let too_large = vec![
            b'{';
            usize::try_from(TOOL_PACKAGE_IMPORT_POLICY_V1.max_manifest_bytes)
                .expect("V1 limit fits usize")
                + 1
        ];
        let error = parse_tool_package_manifest_v1(Cursor::new(too_large))
            .expect_err("oversized source must fail before malformed JSON is parsed");
        assert!(matches!(error, ToolPackageManifestParseErrorV1::ManifestTooLarge));
        assert_eq!(error.public_code(), ToolPackageErrorCode::ManifestTooLarge);
    }

    #[test]
    fn exactly_maximum_manifest_bytes_are_accepted_without_truncation() {
        let maximum = usize::try_from(TOOL_PACKAGE_IMPORT_POLICY_V1.max_manifest_bytes)
            .expect("V1 limit fits usize");
        let mut source = STDIO_MANIFEST.to_vec();
        source.resize(maximum, b' ');

        let parsed =
            parse_tool_package_manifest_v1(Cursor::new(source)).expect("exactly-at-limit Manifest");
        assert_eq!(parsed.source_byte_length(), TOOL_PACKAGE_IMPORT_POLICY_V1.max_manifest_bytes);
    }

    #[test]
    fn strict_gate_rejects_nested_duplicate_keys_and_trailing_content() {
        let duplicate_input_schema = STDIO_MANIFEST
            .split(|byte| *byte == b'\n')
            .map(|line| String::from_utf8_lossy(line))
            .collect::<Vec<_>>()
            .join("\n")
            .replacen(
                "\"input_schema\": {",
                "\"input_schema\": { \"type\": \"object\", \"type\": \"object\",",
                1,
            );
        let duplicate_result_schema = HTTP_MANIFEST.replacen(
            "\"result_schema\": {",
            "\"result_schema\": { \"type\": \"object\", \"type\": \"object\",",
            1,
        );
        let duplicate_conformance_arguments = COMMAND_MANIFEST.replacen(
            "\"arguments\": {",
            "\"arguments\": { \"duplicate\": 1, \"duplicate\": 2,",
            1,
        );

        for source in [
            duplicate_input_schema.into_bytes(),
            duplicate_result_schema.into_bytes(),
            duplicate_conformance_arguments.into_bytes(),
            [STDIO_MANIFEST, br#" {\"second\":true}"#].concat(),
            [STDIO_MANIFEST, b" trailing"].concat(),
        ] {
            let error = parse_tool_package_manifest_v1(Cursor::new(source))
                .expect_err("strict parser must reject duplicate or trailing content");
            assert!(matches!(error, ToolPackageManifestParseErrorV1::StrictJson(_)));
            assert_eq!(error.public_code(), ToolPackageErrorCode::ManifestInvalid);
        }
    }

    #[test]
    fn source_length_participates_in_the_expanded_package_limit() {
        let mut manifest: Value = serde_json::from_slice(STDIO_MANIFEST).expect("fixture JSON");
        let maximum_file = TOOL_PACKAGE_IMPORT_POLICY_V1.max_file_bytes;
        manifest["files"] = Value::Array(
            (0..4)
                .map(|index| {
                    json!({
                        "path": format!("bin/file-{index}.bin"),
                        "byte_length": maximum_file,
                        "sha256": {
                            "algorithm": "sha256",
                            "hex": "0490260c5d222af129fed18947234110e88caf0f7561e5f8d5d9289957d17ce5"
                        },
                        "role": "runtime_resource"
                    })
                })
                .collect(),
        );
        manifest["tools"][0]["runtime"]["executable"] = json!("bin/file-0.bin");
        let source = serde_json::to_vec(&manifest).expect("serialize test Manifest");

        let error = parse_tool_package_manifest_v1(Cursor::new(source))
            .expect_err("Raw source bytes must count toward the expanded package limit");
        assert!(matches!(error, ToolPackageManifestParseErrorV1::RawSemantic(_)));
    }

    #[test]
    fn source_read_failure_never_falls_through_to_partial_json() {
        let error = parse_tool_package_manifest_v1(FailingReader)
            .expect_err("injected read failure must stop parsing");
        assert!(matches!(error, ToolPackageManifestParseErrorV1::SourceRead(_)));
        assert_eq!(error.public_code(), ToolPackageErrorCode::ManifestInvalid);
    }

    struct FailingReader;

    impl Read for FailingReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if buffer.is_empty() {
                return Ok(0);
            }
            Err(io::Error::other("injected W1 read failure"))
        }
    }
}
