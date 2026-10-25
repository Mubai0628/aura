use aura_domain_common::ToolWireVersionV1;
use aura_domain_common::jcs::{CanonicalSha256HashV1, canonical_sha256};
use aura_domain_common::{CredentialId, CredentialRevision};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use thiserror::Error;

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum CredentialReflectionScanStatusV1 {
    NotApplicable,
    Passed,
    Detected,
    Incomplete,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum CredentialReflectionSurfaceV1 {
    RedirectHeaderValue,
    FinalHeaderValue,
    ErrorHeaderValue,
    FinalResponseBody,
    ErrorResponseBody,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum CredentialReflectionEncodingV1 {
    RawUtf8,
    JsonStringEscaped,
    PercentEncodedUppercase,
    PercentEncodedLowercase,
    Base64StandardPadded,
    Base64StandardUnpadded,
    Base64UrlSafePadded,
    Base64UrlSafeUnpadded,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialReflectionSurfaceSetV1 {
    pub values: Vec<CredentialReflectionSurfaceV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialReflectionEncodingSetV1 {
    pub values: Vec<CredentialReflectionEncodingV1>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CredentialReflectionEvidenceErrorV1 {
    #[error("credential reflection evidence collections must be strictly ordered and unique")]
    InvalidCollection,
    #[error("credential reflection detection fields do not match the scan status")]
    InvalidDetection,
    #[error("credential reflection evidence hash is invalid")]
    HashMismatch,
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialReflectionScanEvidenceV1 {
    pub policy_version: ToolWireVersionV1,
    pub status: CredentialReflectionScanStatusV1,
    pub credential_id: CredentialId,
    pub credential_revision: CredentialRevision,
    pub surfaces_scanned: CredentialReflectionSurfaceSetV1,
    pub encoding_variants_scanned: CredentialReflectionEncodingSetV1,
    pub scanned_header_count: u32,
    pub scanned_body_bytes: u64,
    pub detected_surface: Option<CredentialReflectionSurfaceV1>,
    pub detected_encoding: Option<CredentialReflectionEncodingV1>,
    pub evidence_hash: CanonicalSha256HashV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct CredentialReflectionEvidenceHashMaterialV1<'a> {
    policy_version: ToolWireVersionV1,
    status: CredentialReflectionScanStatusV1,
    credential_id: CredentialId,
    credential_revision: CredentialRevision,
    surfaces_scanned: &'a CredentialReflectionSurfaceSetV1,
    encoding_variants_scanned: &'a CredentialReflectionEncodingSetV1,
    scanned_header_count: u32,
    scanned_body_bytes: u64,
    detected_surface: Option<CredentialReflectionSurfaceV1>,
    detected_encoding: Option<CredentialReflectionEncodingV1>,
}

impl CredentialReflectionScanEvidenceV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        status: CredentialReflectionScanStatusV1,
        credential_id: CredentialId,
        credential_revision: CredentialRevision,
        surfaces_scanned: CredentialReflectionSurfaceSetV1,
        encoding_variants_scanned: CredentialReflectionEncodingSetV1,
        scanned_header_count: u32,
        scanned_body_bytes: u64,
        detected_surface: Option<CredentialReflectionSurfaceV1>,
        detected_encoding: Option<CredentialReflectionEncodingV1>,
    ) -> Result<Self, CredentialReflectionEvidenceErrorV1> {
        let mut value = Self {
            policy_version: ToolWireVersionV1,
            status,
            credential_id,
            credential_revision,
            surfaces_scanned,
            encoding_variants_scanned,
            scanned_header_count,
            scanned_body_bytes,
            detected_surface,
            detected_encoding,
            evidence_hash: CanonicalSha256HashV1::sha256(&[]),
        };
        value.validate_without_hash()?;
        value.evidence_hash = value.compute_hash()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), CredentialReflectionEvidenceErrorV1> {
        self.validate_without_hash()?;
        if self.compute_hash()? != self.evidence_hash {
            return Err(CredentialReflectionEvidenceErrorV1::HashMismatch);
        }
        Ok(())
    }

    fn validate_without_hash(&self) -> Result<(), CredentialReflectionEvidenceErrorV1> {
        if !strictly_ordered(&self.surfaces_scanned.values)
            || !strictly_ordered(&self.encoding_variants_scanned.values)
        {
            return Err(CredentialReflectionEvidenceErrorV1::InvalidCollection);
        }
        let fields_present = self.detected_surface.is_some() && self.detected_encoding.is_some();
        if (self.status == CredentialReflectionScanStatusV1::Detected) != fields_present
            || self.detected_surface.is_some() != self.detected_encoding.is_some()
        {
            return Err(CredentialReflectionEvidenceErrorV1::InvalidDetection);
        }
        Ok(())
    }

    fn compute_hash(&self) -> Result<CanonicalSha256HashV1, CredentialReflectionEvidenceErrorV1> {
        canonical_sha256(&CredentialReflectionEvidenceHashMaterialV1 {
            policy_version: self.policy_version,
            status: self.status,
            credential_id: self.credential_id,
            credential_revision: self.credential_revision,
            surfaces_scanned: &self.surfaces_scanned,
            encoding_variants_scanned: &self.encoding_variants_scanned,
            scanned_header_count: self.scanned_header_count,
            scanned_body_bytes: self.scanned_body_bytes,
            detected_surface: self.detected_surface,
            detected_encoding: self.detected_encoding,
        })
        .map_err(|_| CredentialReflectionEvidenceErrorV1::HashMismatch)
    }
}

impl<'de> Deserialize<'de> for CredentialReflectionScanEvidenceV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            policy_version: ToolWireVersionV1,
            status: CredentialReflectionScanStatusV1,
            credential_id: CredentialId,
            credential_revision: CredentialRevision,
            surfaces_scanned: CredentialReflectionSurfaceSetV1,
            encoding_variants_scanned: CredentialReflectionEncodingSetV1,
            scanned_header_count: u32,
            scanned_body_bytes: u64,
            detected_surface: Option<CredentialReflectionSurfaceV1>,
            detected_encoding: Option<CredentialReflectionEncodingV1>,
            evidence_hash: CanonicalSha256HashV1,
        }
        let wire = Wire::deserialize(deserializer)?;
        let value = Self {
            policy_version: wire.policy_version,
            status: wire.status,
            credential_id: wire.credential_id,
            credential_revision: wire.credential_revision,
            surfaces_scanned: wire.surfaces_scanned,
            encoding_variants_scanned: wire.encoding_variants_scanned,
            scanned_header_count: wire.scanned_header_count,
            scanned_body_bytes: wire.scanned_body_bytes,
            detected_surface: wire.detected_surface,
            detected_encoding: wire.detected_encoding,
            evidence_hash: wire.evidence_hash,
        };
        value.validate().map_err(D::Error::custom)?;
        Ok(value)
    }
}

fn strictly_ordered<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{CredentialId, CredentialRevision};

    use super::{
        CredentialReflectionEncodingSetV1, CredentialReflectionEncodingV1,
        CredentialReflectionScanEvidenceV1, CredentialReflectionScanStatusV1,
        CredentialReflectionSurfaceSetV1, CredentialReflectionSurfaceV1,
    };

    #[test]
    fn reflection_evidence_is_redacted_ordered_and_hash_bound() {
        let evidence = CredentialReflectionScanEvidenceV1::new(
            CredentialReflectionScanStatusV1::Detected,
            CredentialId::new(),
            CredentialRevision::INITIAL,
            CredentialReflectionSurfaceSetV1 {
                values: vec![CredentialReflectionSurfaceV1::FinalResponseBody],
            },
            CredentialReflectionEncodingSetV1 {
                values: vec![CredentialReflectionEncodingV1::RawUtf8],
            },
            3,
            64,
            Some(CredentialReflectionSurfaceV1::FinalResponseBody),
            Some(CredentialReflectionEncodingV1::RawUtf8),
        )
        .expect("redacted evidence");
        evidence.validate().expect("valid evidence");
        let text = serde_json::to_string(&evidence).expect("serialize evidence");
        assert!(!text.contains("secret"));

        let mut wire = serde_json::to_value(&evidence).expect("serialize evidence");
        wire["scanned_body_bytes"] = serde_json::json!(65);
        assert!(serde_json::from_value::<CredentialReflectionScanEvidenceV1>(wire).is_err());
    }

    #[test]
    fn detected_status_requires_both_redacted_location_fields() {
        assert!(
            CredentialReflectionScanEvidenceV1::new(
                CredentialReflectionScanStatusV1::Detected,
                CredentialId::new(),
                CredentialRevision::INITIAL,
                CredentialReflectionSurfaceSetV1 {
                    values: vec![CredentialReflectionSurfaceV1::FinalHeaderValue],
                },
                CredentialReflectionEncodingSetV1 {
                    values: vec![CredentialReflectionEncodingV1::RawUtf8],
                },
                1,
                0,
                Some(CredentialReflectionSurfaceV1::FinalHeaderValue),
                None,
            )
            .is_err()
        );
    }
}
