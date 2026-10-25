use std::borrow::Cow;

use aura_domain_common::jcs::{CanonicalSha256HashV1, canonical_sha256};
use aura_domain_common::{Revision, UtcTimestamp};
use aura_domain_common::{ToolConformanceRunId, ToolDraftId};
use aura_tool_contract::{
    CredentialSlotKey, EnvironmentVariableNameV1, FixedHttpsOriginV1, HttpBasePathV1,
    HttpEndpointSlotKeyV1, HttpHeaderNameV1, PackageFingerprintV1, PackageRuntimeClosureHashV1,
    ToolKey, ToolPackageFileSha256V1,
};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::Value;
use thiserror::Error;

use crate::ToolPackageConformanceCaseNameV1;

pub const CONFORMANCE_RUN_BINDING_VERSION_V1: u32 = 1;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ConformanceRunBindingVersionV1;

impl Serialize for ConformanceRunBindingVersionV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u32(CONFORMANCE_RUN_BINDING_VERSION_V1)
    }
}

impl<'de> Deserialize<'de> for ConformanceRunBindingVersionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        if value != CONFORMANCE_RUN_BINDING_VERSION_V1 {
            return Err(D::Error::custom("conformance binding version must be 1"));
        }
        Ok(Self)
    }
}

impl JsonSchema for ConformanceRunBindingVersionV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ConformanceRunBindingVersionV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "integer", "const": 1 })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConformanceDraftRevisionRefV1 {
    pub tool_draft_id: ToolDraftId,
    pub draft_revision: Revision,
    pub draft_snapshot_hash: CanonicalSha256HashV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConformanceSyntheticCredentialPlacementV1 {
    CommandEnvironment { environment_variable: EnvironmentVariableNameV1 },
    HttpBearerAuthorization,
    HttpFixedHeader { header_name: HttpHeaderNameV1 },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConformanceSyntheticCredentialBindingV1 {
    pub slot_key: CredentialSlotKey,
    pub placement: ConformanceSyntheticCredentialPlacementV1,
    /// SHA-256 of ephemeral canary bytes. The secret itself is never part of
    /// the persisted binding or any evidence object.
    pub canary_sha256: ToolPackageFileSha256V1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConformanceFixtureEndpointBindingV1 {
    pub endpoint_slot_key: HttpEndpointSlotKeyV1,
    pub origin: FixedHttpsOriginV1,
    pub base_path: HttpBasePathV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConformanceFixtureResponseBindingV1 {
    pub expected_request_hash: CanonicalSha256HashV1,
    #[schemars(range(min = 100, max = 599))]
    pub status: u16,
    pub response_body_sha256: ToolPackageFileSha256V1,
}

/// Exact, restart-verifiable W3 input. The only open JSON values are the
/// package arguments and non-secret instance configuration, and both are
/// validated against the exact schemas frozen by this binding.
#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConformanceRunBindingV1 {
    binding_version: ConformanceRunBindingVersionV1,
    tool_conformance_run_id: ToolConformanceRunId,
    draft: ConformanceDraftRevisionRefV1,
    tool_key: ToolKey,
    conformance_case: ToolPackageConformanceCaseNameV1,
    canonical_manifest_hash: CanonicalSha256HashV1,
    runtime_closure_hash: PackageRuntimeClosureHashV1,
    package_fingerprint: PackageFingerprintV1,
    arguments: Value,
    arguments_hash: CanonicalSha256HashV1,
    instance_configuration: Option<Value>,
    instance_configuration_hash: CanonicalSha256HashV1,
    synthetic_credential_bindings: Vec<ConformanceSyntheticCredentialBindingV1>,
    fixture_endpoint_bindings: Vec<ConformanceFixtureEndpointBindingV1>,
    fixture_response_bindings: Vec<ConformanceFixtureResponseBindingV1>,
    binding_hash: CanonicalSha256HashV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ConformanceRunBindingPreimageV1<'a> {
    binding_version: ConformanceRunBindingVersionV1,
    tool_conformance_run_id: ToolConformanceRunId,
    draft: &'a ConformanceDraftRevisionRefV1,
    tool_key: &'a ToolKey,
    conformance_case: &'a ToolPackageConformanceCaseNameV1,
    canonical_manifest_hash: &'a CanonicalSha256HashV1,
    runtime_closure_hash: &'a PackageRuntimeClosureHashV1,
    package_fingerprint: &'a PackageFingerprintV1,
    arguments: &'a Value,
    arguments_hash: &'a CanonicalSha256HashV1,
    instance_configuration: &'a Option<Value>,
    instance_configuration_hash: &'a CanonicalSha256HashV1,
    synthetic_credential_bindings: &'a [ConformanceSyntheticCredentialBindingV1],
    fixture_endpoint_bindings: &'a [ConformanceFixtureEndpointBindingV1],
    fixture_response_bindings: &'a [ConformanceFixtureResponseBindingV1],
}

impl ConformanceRunBindingV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tool_conformance_run_id: ToolConformanceRunId,
        draft: ConformanceDraftRevisionRefV1,
        tool_key: ToolKey,
        conformance_case: ToolPackageConformanceCaseNameV1,
        canonical_manifest_hash: CanonicalSha256HashV1,
        runtime_closure_hash: PackageRuntimeClosureHashV1,
        package_fingerprint: PackageFingerprintV1,
        arguments: Value,
        instance_configuration: Option<Value>,
        mut synthetic_credential_bindings: Vec<ConformanceSyntheticCredentialBindingV1>,
        mut fixture_endpoint_bindings: Vec<ConformanceFixtureEndpointBindingV1>,
        mut fixture_response_bindings: Vec<ConformanceFixtureResponseBindingV1>,
    ) -> Result<Self, ConformanceBindingErrorV1> {
        if !arguments.is_object()
            || instance_configuration.as_ref().is_some_and(|value| !value.is_object())
        {
            return Err(ConformanceBindingErrorV1::InvalidJsonObject);
        }
        synthetic_credential_bindings.sort_by(|left, right| {
            left.slot_key.as_str().as_bytes().cmp(right.slot_key.as_str().as_bytes())
        });
        fixture_endpoint_bindings.sort_by(|left, right| {
            left.endpoint_slot_key
                .as_str()
                .as_bytes()
                .cmp(right.endpoint_slot_key.as_str().as_bytes())
        });
        fixture_response_bindings.sort_by(|left, right| {
            left.expected_request_hash
                .hex()
                .as_bytes()
                .cmp(right.expected_request_hash.hex().as_bytes())
        });
        if !collections_are_strictly_ordered(&synthetic_credential_bindings, |value| {
            value.slot_key.as_str().as_bytes()
        }) || !collections_are_strictly_ordered(&fixture_endpoint_bindings, |value| {
            value.endpoint_slot_key.as_str().as_bytes()
        }) || !collections_are_strictly_ordered(&fixture_response_bindings, |value| {
            value.expected_request_hash.hex().as_bytes()
        }) {
            return Err(ConformanceBindingErrorV1::DuplicateBinding);
        }

        let arguments_hash = canonical_sha256(&arguments)
            .map_err(|_| ConformanceBindingErrorV1::Canonicalization)?;
        let instance_configuration_hash = canonical_sha256(&instance_configuration)
            .map_err(|_| ConformanceBindingErrorV1::Canonicalization)?;
        let mut binding = Self {
            binding_version: ConformanceRunBindingVersionV1,
            tool_conformance_run_id,
            draft,
            tool_key,
            conformance_case,
            canonical_manifest_hash,
            runtime_closure_hash,
            package_fingerprint,
            arguments,
            arguments_hash,
            instance_configuration,
            instance_configuration_hash,
            synthetic_credential_bindings,
            fixture_endpoint_bindings,
            fixture_response_bindings,
            binding_hash: CanonicalSha256HashV1::sha256(b"pending"),
        };
        binding.binding_hash = binding.recompute_hash()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), ConformanceBindingErrorV1> {
        if !self.arguments.is_object()
            || self.instance_configuration.as_ref().is_some_and(|value| !value.is_object())
            || !collections_are_strictly_ordered(&self.synthetic_credential_bindings, |value| {
                value.slot_key.as_str().as_bytes()
            })
            || !collections_are_strictly_ordered(&self.fixture_endpoint_bindings, |value| {
                value.endpoint_slot_key.as_str().as_bytes()
            })
            || !collections_are_strictly_ordered(&self.fixture_response_bindings, |value| {
                value.expected_request_hash.hex().as_bytes()
            })
        {
            return Err(ConformanceBindingErrorV1::InvalidBinding);
        }
        if self.arguments_hash
            != canonical_sha256(&self.arguments)
                .map_err(|_| ConformanceBindingErrorV1::Canonicalization)?
            || self.instance_configuration_hash
                != canonical_sha256(&self.instance_configuration)
                    .map_err(|_| ConformanceBindingErrorV1::Canonicalization)?
            || self.binding_hash != self.recompute_hash()?
        {
            return Err(ConformanceBindingErrorV1::HashMismatch);
        }
        Ok(())
    }

    fn recompute_hash(&self) -> Result<CanonicalSha256HashV1, ConformanceBindingErrorV1> {
        canonical_sha256(&ConformanceRunBindingPreimageV1 {
            binding_version: self.binding_version,
            tool_conformance_run_id: self.tool_conformance_run_id,
            draft: &self.draft,
            tool_key: &self.tool_key,
            conformance_case: &self.conformance_case,
            canonical_manifest_hash: &self.canonical_manifest_hash,
            runtime_closure_hash: &self.runtime_closure_hash,
            package_fingerprint: &self.package_fingerprint,
            arguments: &self.arguments,
            arguments_hash: &self.arguments_hash,
            instance_configuration: &self.instance_configuration,
            instance_configuration_hash: &self.instance_configuration_hash,
            synthetic_credential_bindings: &self.synthetic_credential_bindings,
            fixture_endpoint_bindings: &self.fixture_endpoint_bindings,
            fixture_response_bindings: &self.fixture_response_bindings,
        })
        .map_err(|_| ConformanceBindingErrorV1::Canonicalization)
    }

    #[must_use]
    pub const fn tool_conformance_run_id(&self) -> ToolConformanceRunId {
        self.tool_conformance_run_id
    }

    #[must_use]
    pub const fn draft(&self) -> &ConformanceDraftRevisionRefV1 {
        &self.draft
    }

    #[must_use]
    pub const fn tool_key(&self) -> &ToolKey {
        &self.tool_key
    }

    #[must_use]
    pub const fn conformance_case(&self) -> &ToolPackageConformanceCaseNameV1 {
        &self.conformance_case
    }

    #[must_use]
    pub const fn canonical_manifest_hash(&self) -> &CanonicalSha256HashV1 {
        &self.canonical_manifest_hash
    }

    #[must_use]
    pub const fn runtime_closure_hash(&self) -> &PackageRuntimeClosureHashV1 {
        &self.runtime_closure_hash
    }

    #[must_use]
    pub const fn package_fingerprint(&self) -> &PackageFingerprintV1 {
        &self.package_fingerprint
    }

    #[must_use]
    pub const fn arguments(&self) -> &Value {
        &self.arguments
    }

    #[must_use]
    pub const fn arguments_hash(&self) -> &CanonicalSha256HashV1 {
        &self.arguments_hash
    }

    #[must_use]
    pub const fn instance_configuration(&self) -> &Option<Value> {
        &self.instance_configuration
    }

    #[must_use]
    pub const fn instance_configuration_hash(&self) -> &CanonicalSha256HashV1 {
        &self.instance_configuration_hash
    }

    #[must_use]
    pub fn synthetic_credential_bindings(&self) -> &[ConformanceSyntheticCredentialBindingV1] {
        &self.synthetic_credential_bindings
    }

    #[must_use]
    pub fn fixture_endpoint_bindings(&self) -> &[ConformanceFixtureEndpointBindingV1] {
        &self.fixture_endpoint_bindings
    }

    #[must_use]
    pub fn fixture_response_bindings(&self) -> &[ConformanceFixtureResponseBindingV1] {
        &self.fixture_response_bindings
    }

    #[must_use]
    pub const fn binding_hash(&self) -> &CanonicalSha256HashV1 {
        &self.binding_hash
    }
}

impl<'de> Deserialize<'de> for ConformanceRunBindingV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            binding_version: ConformanceRunBindingVersionV1,
            tool_conformance_run_id: ToolConformanceRunId,
            draft: ConformanceDraftRevisionRefV1,
            tool_key: ToolKey,
            conformance_case: ToolPackageConformanceCaseNameV1,
            canonical_manifest_hash: CanonicalSha256HashV1,
            runtime_closure_hash: PackageRuntimeClosureHashV1,
            package_fingerprint: PackageFingerprintV1,
            arguments: Value,
            arguments_hash: CanonicalSha256HashV1,
            instance_configuration: Option<Value>,
            instance_configuration_hash: CanonicalSha256HashV1,
            synthetic_credential_bindings: Vec<ConformanceSyntheticCredentialBindingV1>,
            fixture_endpoint_bindings: Vec<ConformanceFixtureEndpointBindingV1>,
            fixture_response_bindings: Vec<ConformanceFixtureResponseBindingV1>,
            binding_hash: CanonicalSha256HashV1,
        }

        let wire = Wire::deserialize(deserializer)?;
        let binding = Self {
            binding_version: wire.binding_version,
            tool_conformance_run_id: wire.tool_conformance_run_id,
            draft: wire.draft,
            tool_key: wire.tool_key,
            conformance_case: wire.conformance_case,
            canonical_manifest_hash: wire.canonical_manifest_hash,
            runtime_closure_hash: wire.runtime_closure_hash,
            package_fingerprint: wire.package_fingerprint,
            arguments: wire.arguments,
            arguments_hash: wire.arguments_hash,
            instance_configuration: wire.instance_configuration,
            instance_configuration_hash: wire.instance_configuration_hash,
            synthetic_credential_bindings: wire.synthetic_credential_bindings,
            fixture_endpoint_bindings: wire.fixture_endpoint_bindings,
            fixture_response_bindings: wire.fixture_response_bindings,
            binding_hash: wire.binding_hash,
        };
        binding.validate().map_err(D::Error::custom)?;
        Ok(binding)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConformanceCaseStatusV1 {
    Passed,
    Failed,
    Cancelled,
    RecoveryRequired,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConformanceCaseEvidenceV1 {
    pub tool_conformance_run_id: ToolConformanceRunId,
    pub binding_hash: CanonicalSha256HashV1,
    pub status: ConformanceCaseStatusV1,
    pub runtime_evidence_hash: Option<CanonicalSha256HashV1>,
    pub secret_leak_detected: bool,
    pub started_at: UtcTimestamp,
    pub completed_at: UtcTimestamp,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ConformanceBindingErrorV1 {
    #[error("conformance binding JSON values must be object-shaped")]
    InvalidJsonObject,
    #[error("conformance binding contains a duplicate frozen binding")]
    DuplicateBinding,
    #[error("conformance binding is not in canonical order or contains invalid facts")]
    InvalidBinding,
    #[error("conformance binding canonicalization failed")]
    Canonicalization,
    #[error("conformance binding hash does not match its exact facts")]
    HashMismatch,
}

fn collections_are_strictly_ordered<'a, T>(
    values: &'a [T],
    key: impl Fn(&'a T) -> &'a [u8],
) -> bool {
    values.windows(2).all(|pair| key(&pair[0]) < key(&pair[1]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn sha(value: &[u8]) -> CanonicalSha256HashV1 {
        CanonicalSha256HashV1::sha256(value)
    }

    fn binding() -> ConformanceRunBindingV1 {
        ConformanceRunBindingV1::new(
            ToolConformanceRunId::new(),
            ConformanceDraftRevisionRefV1 {
                tool_draft_id: ToolDraftId::new(),
                draft_revision: Revision::new(2),
                draft_snapshot_hash: sha(b"draft"),
            },
            ToolKey::new("echo").expect("tool key"),
            ToolPackageConformanceCaseNameV1::new("empty").expect("case"),
            sha(b"manifest"),
            PackageRuntimeClosureHashV1(sha(b"closure")),
            PackageFingerprintV1(sha(b"package")),
            json!({}),
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect("binding")
    }

    #[test]
    fn binding_round_trips_and_rejects_changed_evidence() {
        let binding = binding();
        let bytes = serde_json::to_vec(&binding).expect("serialize");
        assert_eq!(
            serde_json::from_slice::<ConformanceRunBindingV1>(&bytes).expect("round trip"),
            binding
        );

        let mut value: Value = serde_json::from_slice(&bytes).expect("JSON");
        value["arguments"] = json!({"changed": true});
        assert!(serde_json::from_value::<ConformanceRunBindingV1>(value).is_err());
    }

    #[test]
    fn binding_rejects_unknown_fields_and_duplicate_slots() {
        let mut value = serde_json::to_value(binding()).expect("value");
        value["unknown"] = json!(true);
        assert!(serde_json::from_value::<ConformanceRunBindingV1>(value).is_err());

        let slot = ConformanceSyntheticCredentialBindingV1 {
            slot_key: CredentialSlotKey::new("api-key").expect("slot"),
            placement: ConformanceSyntheticCredentialPlacementV1::HttpBearerAuthorization,
            canary_sha256: ToolPackageFileSha256V1::sha256(b"canary"),
        };
        let error = ConformanceRunBindingV1::new(
            ToolConformanceRunId::new(),
            ConformanceDraftRevisionRefV1 {
                tool_draft_id: ToolDraftId::new(),
                draft_revision: Revision::INITIAL,
                draft_snapshot_hash: sha(b"draft"),
            },
            ToolKey::new("echo").expect("tool key"),
            ToolPackageConformanceCaseNameV1::new("empty").expect("case"),
            sha(b"manifest"),
            PackageRuntimeClosureHashV1(sha(b"closure")),
            PackageFingerprintV1(sha(b"package")),
            json!({}),
            None,
            vec![slot.clone(), slot],
            Vec::new(),
            Vec::new(),
        )
        .expect_err("duplicate slot");
        assert_eq!(error, ConformanceBindingErrorV1::DuplicateBinding);
    }

    #[test]
    fn version_and_hash_wire_are_stable() {
        let value = serde_json::to_value(binding()).expect("wire");
        assert_eq!(value["binding_version"], json!(1));
        assert_eq!(value["binding_hash"]["algorithm"], json!("sha256"));
    }
}
