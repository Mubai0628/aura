use std::{future::Future, pin::Pin};

use aura_domain_common::ToolConformanceRunId;
use aura_domain_common::UtcTimestamp;
use aura_domain_common::jcs::{CanonicalSha256HashV1, canonical_sha256};
use aura_tool_contract::{
    CompiledToolSchemaV1, CredentialSlotKey, ToolConformanceErrorCode, ToolKey,
    ToolPackageFileSha256V1,
};
use aura_tool_domain::{
    CanonicalToolPackageDeclarativeCommandV1, CanonicalToolPackageRuntimeV1,
    CanonicalToolPackageToolV1, ConformanceBindingErrorV1, ConformanceCaseEvidenceV1,
    ConformanceCaseStatusV1, ConformanceDraftRevisionRefV1, ConformanceFixtureEndpointBindingV1,
    ConformanceFixtureResponseBindingV1, ConformanceRunBindingV1,
    ConformanceSyntheticCredentialBindingV1, ConformanceSyntheticCredentialPlacementV1,
    ToolPackageConformanceCaseNameV1,
};
use aura_tool_runtime::PreparedToolResultContractV1;
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;
use zeroize::Zeroizing;

use crate::{StagedToolPackageV1, ToolPackageStagingErrorV1};

const SYNTHETIC_SECRET_PREFIX: &[u8] = b"aura-conformance-canary-v1-";

#[derive(Clone, Debug)]
pub struct PrepareConformanceRunV1 {
    pub tool_conformance_run_id: ToolConformanceRunId,
    pub draft: ConformanceDraftRevisionRefV1,
    pub tool_key: ToolKey,
    pub conformance_case: ToolPackageConformanceCaseNameV1,
    pub instance_configuration: Option<Value>,
    pub fixture_endpoint_bindings: Vec<ConformanceFixtureEndpointBindingV1>,
    pub fixture_responses: Vec<ConformanceFixtureResponseInputV1>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConformanceFixtureResponseInputV1 {
    pub status: u16,
    pub response_body: Vec<u8>,
}

/// Secret material is intentionally neither Clone, Debug, Serialize nor
/// Deserialize. Dropping a prepared run clears every generated canary.
pub struct SyntheticCredentialMaterialV1 {
    binding: ConformanceSyntheticCredentialBindingV1,
    secret: Zeroizing<Vec<u8>>,
}

impl SyntheticCredentialMaterialV1 {
    fn generate(
        slot_key: CredentialSlotKey,
        placement: ConformanceSyntheticCredentialPlacementV1,
    ) -> Result<Self, ToolConformancePreparationErrorV1> {
        let mut random = [0_u8; 32];
        getrandom::fill(&mut random)
            .map_err(|_| ToolConformancePreparationErrorV1::SyntheticCredentialUnavailable)?;
        let mut secret = Vec::with_capacity(SYNTHETIC_SECRET_PREFIX.len() + random.len() * 2);
        secret.extend_from_slice(SYNTHETIC_SECRET_PREFIX);
        for byte in random {
            secret.extend_from_slice(format!("{byte:02x}").as_bytes());
        }
        let secret = Zeroizing::new(secret);
        let binding = ConformanceSyntheticCredentialBindingV1 {
            slot_key,
            placement,
            canary_sha256: ToolPackageFileSha256V1::sha256(secret.as_slice()),
        };
        Ok(Self { binding, secret })
    }

    #[must_use]
    pub const fn binding(&self) -> &ConformanceSyntheticCredentialBindingV1 {
        &self.binding
    }

    /// The only plaintext accessor. Runtime adapters must inject these bytes
    /// directly into the declared slot and must not copy them into evidence.
    #[must_use]
    pub fn expose_for_injection(&self) -> &[u8] {
        self.secret.as_slice()
    }
}

pub struct PreparedConformanceRunV1 {
    binding: ConformanceRunBindingV1,
    tool: CanonicalToolPackageToolV1,
    credentials: Vec<SyntheticCredentialMaterialV1>,
}

impl PreparedConformanceRunV1 {
    #[must_use]
    pub const fn binding(&self) -> &ConformanceRunBindingV1 {
        &self.binding
    }

    #[must_use]
    pub const fn tool(&self) -> &CanonicalToolPackageToolV1 {
        &self.tool
    }

    #[must_use]
    pub fn credentials(&self) -> &[SyntheticCredentialMaterialV1] {
        &self.credentials
    }
}

pub fn prepare_conformance_run_v1(
    staged_package: &StagedToolPackageV1,
    request: PrepareConformanceRunV1,
) -> Result<PreparedConformanceRunV1, ToolConformancePreparationErrorV1> {
    staged_package.verify_integrity()?;
    let evidence = staged_package.fingerprint_evidence()?;
    let manifest = staged_package.canonical_manifest();
    let tool = manifest
        .tools
        .iter()
        .find(|tool| tool.tool_key == request.tool_key)
        .ok_or(ToolConformancePreparationErrorV1::ToolNotFound)?
        .clone();
    let case = tool
        .conformance_cases
        .iter()
        .find(|case| case.name == request.conformance_case)
        .ok_or(ToolConformancePreparationErrorV1::CaseNotFound)?;

    let arguments_validator =
        CompiledToolSchemaV1::compile(tool.model.input_schema.canonical_document())
            .map_err(|_| ToolConformancePreparationErrorV1::SchemaInvalid)?;
    if !arguments_validator.validate(case.arguments.value()).valid {
        return Err(ToolConformancePreparationErrorV1::ArgumentsInvalid);
    }
    validate_instance_configuration(&tool, request.instance_configuration.as_ref())?;
    validate_fixture_bindings(
        &tool,
        &request.fixture_endpoint_bindings,
        &request.fixture_responses,
    )?;

    let credentials = generate_synthetic_credentials(&tool)?;
    let credential_bindings: Vec<ConformanceSyntheticCredentialBindingV1> =
        credentials.iter().map(|item| item.binding.clone()).collect();
    let fixture_request_hash = conformance_fixture_request_hash_v1(
        &tool,
        case.arguments.value(),
        &request.instance_configuration,
        &credential_bindings,
        &request.fixture_endpoint_bindings,
    )?;
    let fixture_response_bindings = request
        .fixture_responses
        .iter()
        .map(|response| ConformanceFixtureResponseBindingV1 {
            expected_request_hash: fixture_request_hash.clone(),
            status: response.status,
            response_body_sha256: ToolPackageFileSha256V1::sha256(&response.response_body),
        })
        .collect();
    let runtime_closure_hash = staged_package
        .runtime_closure()
        .canonical_hash()
        .map_err(|_| ToolConformancePreparationErrorV1::StagingMismatch)?;
    let binding = ConformanceRunBindingV1::new(
        request.tool_conformance_run_id,
        request.draft,
        request.tool_key,
        request.conformance_case,
        CanonicalSha256HashV1::sha256(staged_package.canonical_manifest_bytes()),
        runtime_closure_hash,
        evidence.package_fingerprint().clone(),
        case.arguments.value().clone(),
        request.instance_configuration,
        credential_bindings,
        request.fixture_endpoint_bindings,
        fixture_response_bindings,
    )?;
    verify_binding_against_staged(&binding, staged_package, &tool)?;
    Ok(PreparedConformanceRunV1 { binding, tool, credentials })
}

fn validate_instance_configuration(
    tool: &CanonicalToolPackageToolV1,
    configuration: Option<&Value>,
) -> Result<(), ToolConformancePreparationErrorV1> {
    match (&tool.instance_configuration.0, configuration) {
        (None, None) => Ok(()),
        (None, Some(_)) | (Some(_), None) => {
            Err(ToolConformancePreparationErrorV1::InstanceConfigurationInvalid)
        }
        (Some(schema), Some(value)) => {
            let validator = CompiledToolSchemaV1::compile(schema.canonical_document())
                .map_err(|_| ToolConformancePreparationErrorV1::SchemaInvalid)?;
            if validator.validate(value).valid {
                Ok(())
            } else {
                Err(ToolConformancePreparationErrorV1::InstanceConfigurationInvalid)
            }
        }
    }
}

fn validate_fixture_bindings(
    tool: &CanonicalToolPackageToolV1,
    endpoints: &[ConformanceFixtureEndpointBindingV1],
    responses: &[ConformanceFixtureResponseInputV1],
) -> Result<(), ToolConformancePreparationErrorV1> {
    match &tool.runtime {
        CanonicalToolPackageRuntimeV1::DeclarativeHttp { request } => {
            if responses.len() != 1 || !(100..=599).contains(&responses[0].status) {
                return Err(ToolConformancePreparationErrorV1::FixtureBindingMissing);
            }
            if let aura_tool_contract::DeclarativeHttpTargetV1::UserBoundEndpoint {
                endpoint_slot_key,
            } = &request.target
                && endpoints.iter().all(|binding| binding.endpoint_slot_key != *endpoint_slot_key)
            {
                return Err(ToolConformancePreparationErrorV1::FixtureBindingMissing);
            }
            Ok(())
        }
        _ if endpoints.is_empty() && responses.is_empty() => Ok(()),
        _ => Err(ToolConformancePreparationErrorV1::FixtureBindingUnexpected),
    }
}

fn generate_synthetic_credentials(
    tool: &CanonicalToolPackageToolV1,
) -> Result<Vec<SyntheticCredentialMaterialV1>, ToolConformancePreparationErrorV1> {
    let value = match &tool.runtime {
        CanonicalToolPackageRuntimeV1::DeclarativeCommand { command } => match command {
            CanonicalToolPackageDeclarativeCommandV1::Argv { credential_slot, .. }
            | CanonicalToolPackageDeclarativeCommandV1::PowershellScript {
                credential_slot, ..
            } => credential_slot.0.as_ref().map(|slot| {
                (
                    slot.slot_key.clone(),
                    ConformanceSyntheticCredentialPlacementV1::CommandEnvironment {
                        environment_variable: slot.environment_variable.clone(),
                    },
                )
            }),
        },
        CanonicalToolPackageRuntimeV1::AuraStdio { credential_slot, .. } => {
            credential_slot.0.as_ref().map(|slot| {
                (
                    slot.slot_key.clone(),
                    ConformanceSyntheticCredentialPlacementV1::CommandEnvironment {
                        environment_variable: slot.environment_variable.clone(),
                    },
                )
            })
        }
        CanonicalToolPackageRuntimeV1::DeclarativeHttp { request } => {
            request.credential_slot.0.as_ref().map(|slot| match slot {
                aura_tool_contract::HttpCredentialSlotV1::BearerAuthorization { slot_key } => (
                    slot_key.clone(),
                    ConformanceSyntheticCredentialPlacementV1::HttpBearerAuthorization,
                ),
                aura_tool_contract::HttpCredentialSlotV1::FixedHeader { slot_key, header_name } => {
                    (
                        slot_key.clone(),
                        ConformanceSyntheticCredentialPlacementV1::HttpFixedHeader {
                            header_name: header_name.clone(),
                        },
                    )
                }
            })
        }
    };
    value
        .map(|(slot_key, placement)| {
            SyntheticCredentialMaterialV1::generate(slot_key, placement).map(|value| vec![value])
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn verify_binding_against_staged(
    binding: &ConformanceRunBindingV1,
    staged_package: &StagedToolPackageV1,
    tool: &CanonicalToolPackageToolV1,
) -> Result<(), ToolConformancePreparationErrorV1> {
    binding.validate()?;
    let current_closure_hash = staged_package
        .runtime_closure()
        .canonical_hash()
        .map_err(|_| ToolConformancePreparationErrorV1::StagingMismatch)?;
    let case_matches = tool.conformance_cases.iter().any(|case| {
        case.name == *binding.conformance_case() && case.arguments.value() == binding.arguments()
    });
    if binding.tool_key() != &tool.tool_key
        || binding.canonical_manifest_hash()
            != &CanonicalSha256HashV1::sha256(staged_package.canonical_manifest_bytes())
        || binding.runtime_closure_hash() != &current_closure_hash
        || !case_matches
    {
        return Err(ToolConformancePreparationErrorV1::StagingMismatch);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConformanceRuntimeTerminalV1 {
    Succeeded,
    Failed,
    Cancelled,
    Indeterminate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConformanceObservedBytesV1 {
    pub channel: &'static str,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConformanceRuntimeObservationV1 {
    pub terminal: ConformanceRuntimeTerminalV1,
    pub result_body: Vec<u8>,
    pub observed_bytes: Vec<ConformanceObservedBytesV1>,
    pub cleanup_verified: bool,
    pub protocol_verified: bool,
    pub artifacts_verified: bool,
}

pub struct ConformanceRuntimeInvocationV1<'a> {
    pub staged_package: &'a StagedToolPackageV1,
    pub binding: &'a ConformanceRunBindingV1,
    pub tool: &'a CanonicalToolPackageToolV1,
    pub credentials: &'a [SyntheticCredentialMaterialV1],
}

pub trait ConformanceRuntimeHostV1: Send + Sync {
    fn execute<'a>(
        &'a self,
        invocation: ConformanceRuntimeInvocationV1<'a>,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<ConformanceRuntimeObservationV1, ConformanceRuntimeErrorV1>>
                + Send
                + 'a,
        >,
    >;
}

pub async fn execute_prepared_conformance_run_v1(
    prepared: &PreparedConformanceRunV1,
    staged_package: &StagedToolPackageV1,
    host: &dyn ConformanceRuntimeHostV1,
) -> Result<ConformanceCaseEvidenceV1, ToolConformanceExecutionErrorV1> {
    staged_package.verify_integrity()?;
    verify_binding_against_staged(prepared.binding(), staged_package, prepared.tool())?;
    let started_at = UtcTimestamp::now();
    let observation = host
        .execute(ConformanceRuntimeInvocationV1 {
            staged_package,
            binding: prepared.binding(),
            tool: prepared.tool(),
            credentials: prepared.credentials(),
        })
        .await?;
    let completed_at = UtcTimestamp::now();

    evaluate_prepared_conformance_observation_v1(
        prepared,
        staged_package,
        observation,
        started_at,
        completed_at,
    )
}

/// Converts one observation from an already-authorized production Runtime
/// adapter into the same immutable W3 evidence as the Fixture Host path.
/// Runtime adapters cannot choose Passed/Failed or bypass the exact staged
/// Package and result-contract checks performed here.
pub fn evaluate_prepared_conformance_observation_v1(
    prepared: &PreparedConformanceRunV1,
    staged_package: &StagedToolPackageV1,
    observation: ConformanceRuntimeObservationV1,
    started_at: UtcTimestamp,
    completed_at: UtcTimestamp,
) -> Result<ConformanceCaseEvidenceV1, ToolConformanceExecutionErrorV1> {
    staged_package.verify_integrity()?;
    verify_binding_against_staged(prepared.binding(), staged_package, prepared.tool())?;
    if completed_at < started_at {
        return Err(ToolConformanceExecutionErrorV1::EvidenceInvalid);
    }

    let secret_leak_detected =
        prepared.credentials().iter().any(|credential| {
            observation.observed_bytes.iter().any(|observed| {
                contains_subslice(&observed.bytes, credential.expose_for_injection())
            }) || contains_subslice(&observation.result_body, credential.expose_for_injection())
        });
    let result_contract =
        PreparedToolResultContractV1::compile(&prepared.tool().model.result_contract)
            .map_err(|_| ToolConformanceExecutionErrorV1::ResultContractInvalid)?;
    let result_valid = matches!(observation.terminal, ConformanceRuntimeTerminalV1::Succeeded)
        && result_contract.validate_candidate(&observation.result_body).is_ok();
    let status = if matches!(observation.terminal, ConformanceRuntimeTerminalV1::Indeterminate)
        || !observation.cleanup_verified
    {
        ConformanceCaseStatusV1::RecoveryRequired
    } else if matches!(observation.terminal, ConformanceRuntimeTerminalV1::Cancelled) {
        ConformanceCaseStatusV1::Cancelled
    } else if result_valid
        && observation.protocol_verified
        && observation.artifacts_verified
        && !secret_leak_detected
    {
        ConformanceCaseStatusV1::Passed
    } else {
        ConformanceCaseStatusV1::Failed
    };

    let runtime_evidence_hash = canonical_sha256(&RuntimeEvidencePreimageV1 {
        terminal: match observation.terminal {
            ConformanceRuntimeTerminalV1::Succeeded => "succeeded",
            ConformanceRuntimeTerminalV1::Failed => "failed",
            ConformanceRuntimeTerminalV1::Cancelled => "cancelled",
            ConformanceRuntimeTerminalV1::Indeterminate => "indeterminate",
        },
        result_body_sha256: ToolPackageFileSha256V1::sha256(&observation.result_body),
        observed_channels: observation
            .observed_bytes
            .iter()
            .map(|value| RuntimeObservedChannelEvidenceV1 {
                channel: value.channel,
                content_sha256: ToolPackageFileSha256V1::sha256(&value.bytes),
            })
            .collect(),
        cleanup_verified: observation.cleanup_verified,
        protocol_verified: observation.protocol_verified,
        artifacts_verified: observation.artifacts_verified,
        secret_leak_detected,
    })
    .map_err(|_| ToolConformanceExecutionErrorV1::EvidenceInvalid)?;

    Ok(ConformanceCaseEvidenceV1 {
        tool_conformance_run_id: prepared.binding().tool_conformance_run_id(),
        binding_hash: prepared.binding().binding_hash().clone(),
        status,
        runtime_evidence_hash: Some(runtime_evidence_hash),
        secret_leak_detected,
        started_at,
        completed_at,
    })
}

/// Produces the exact redacted Fixture request identity used by W3. The
/// preimage includes the full canonical Runtime mapping and inputs, including
/// synthetic credential placement/hash, but never includes credential bytes.
pub fn conformance_fixture_request_hash_v1(
    tool: &CanonicalToolPackageToolV1,
    arguments: &Value,
    instance_configuration: &Option<Value>,
    synthetic_credential_bindings: &[ConformanceSyntheticCredentialBindingV1],
    fixture_endpoint_bindings: &[ConformanceFixtureEndpointBindingV1],
) -> Result<CanonicalSha256HashV1, ConformanceBindingErrorV1> {
    #[derive(Serialize)]
    #[serde(deny_unknown_fields)]
    struct Preimage<'a> {
        kind: &'static str,
        preimage_version: u32,
        tool_key: &'a ToolKey,
        runtime: &'a CanonicalToolPackageRuntimeV1,
        arguments: &'a Value,
        instance_configuration: &'a Option<Value>,
        synthetic_credential_bindings: &'a [ConformanceSyntheticCredentialBindingV1],
        fixture_endpoint_bindings: &'a [ConformanceFixtureEndpointBindingV1],
    }

    canonical_sha256(&Preimage {
        kind: "aura.p07.conformance_fixture_request",
        preimage_version: 1,
        tool_key: &tool.tool_key,
        runtime: &tool.runtime,
        arguments,
        instance_configuration,
        synthetic_credential_bindings,
        fixture_endpoint_bindings,
    })
    .map_err(|_| ConformanceBindingErrorV1::Canonicalization)
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct RuntimeEvidencePreimageV1<'a> {
    terminal: &'a str,
    result_body_sha256: ToolPackageFileSha256V1,
    observed_channels: Vec<RuntimeObservedChannelEvidenceV1<'a>>,
    cleanup_verified: bool,
    protocol_verified: bool,
    artifacts_verified: bool,
    secret_leak_detected: bool,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct RuntimeObservedChannelEvidenceV1<'a> {
    channel: &'a str,
    content_sha256: ToolPackageFileSha256V1,
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ConformanceRuntimeErrorV1 {
    #[error("conformance runtime isolation is unavailable")]
    IsolationUnavailable,
    #[error("conformance fixture request did not match the frozen binding")]
    FixtureRequestMismatch,
    #[error("conformance runtime failed deterministically")]
    RuntimeFailed,
    #[error("conformance runtime terminal state or cleanup is uncertain")]
    RuntimeIndeterminate,
}

#[derive(Debug, Error)]
pub enum ToolConformancePreparationErrorV1 {
    #[error(transparent)]
    Staging(#[from] ToolPackageStagingErrorV1),
    #[error(transparent)]
    Binding(#[from] ConformanceBindingErrorV1),
    #[error("conformance Tool is absent from the exact canonical Manifest")]
    ToolNotFound,
    #[error("conformance case is absent from the exact Tool contract")]
    CaseNotFound,
    #[error("conformance schema could not be compiled")]
    SchemaInvalid,
    #[error("conformance arguments do not satisfy the exact Tool input schema")]
    ArgumentsInvalid,
    #[error("conformance instance configuration is missing or invalid")]
    InstanceConfigurationInvalid,
    #[error("required local conformance Fixture binding is missing")]
    FixtureBindingMissing,
    #[error("non-HTTP conformance case contains an HTTP Fixture binding")]
    FixtureBindingUnexpected,
    #[error("synthetic conformance credential could not be generated")]
    SyntheticCredentialUnavailable,
    #[error("staged package evidence changed or does not match the binding")]
    StagingMismatch,
}

impl ToolConformancePreparationErrorV1 {
    #[must_use]
    pub const fn public_code(&self) -> ToolConformanceErrorCode {
        match self {
            Self::ToolNotFound | Self::CaseNotFound => ToolConformanceErrorCode::CaseMissing,
            Self::ArgumentsInvalid
            | Self::InstanceConfigurationInvalid
            | Self::FixtureBindingMissing
            | Self::FixtureBindingUnexpected
            | Self::SchemaInvalid => ToolConformanceErrorCode::CaseInvalid,
            Self::SyntheticCredentialUnavailable => ToolConformanceErrorCode::IsolationUnavailable,
            Self::Staging(_) | Self::Binding(_) | Self::StagingMismatch => {
                ToolConformanceErrorCode::EvidenceInvalid
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum ToolConformanceExecutionErrorV1 {
    #[error(transparent)]
    Staging(#[from] ToolPackageStagingErrorV1),
    #[error(transparent)]
    Preparation(#[from] ToolConformancePreparationErrorV1),
    #[error(transparent)]
    Runtime(#[from] ConformanceRuntimeErrorV1),
    #[error("conformance result contract is invalid")]
    ResultContractInvalid,
    #[error("conformance evidence could not be canonicalized")]
    EvidenceInvalid,
}

impl ToolConformanceExecutionErrorV1 {
    #[must_use]
    pub const fn public_code(&self) -> ToolConformanceErrorCode {
        match self {
            Self::Staging(_) | Self::Preparation(_) | Self::EvidenceInvalid => {
                ToolConformanceErrorCode::EvidenceInvalid
            }
            Self::Runtime(ConformanceRuntimeErrorV1::IsolationUnavailable) => {
                ToolConformanceErrorCode::IsolationUnavailable
            }
            Self::Runtime(_) | Self::ResultContractInvalid => ToolConformanceErrorCode::RunFailed,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use aura_domain_common::ToolDraftId;
    use aura_tool_contract::{TOOL_PACKAGE_MANIFEST_NAME, ToolPackageFileSha256V1};
    use serde_json::json;

    use super::*;
    use crate::stage_directory_package_v1;

    struct Host {
        leak_secret: bool,
        cleanup_verified: bool,
    }

    impl ConformanceRuntimeHostV1 for Host {
        fn execute<'a>(
            &'a self,
            invocation: ConformanceRuntimeInvocationV1<'a>,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<ConformanceRuntimeObservationV1, ConformanceRuntimeErrorV1>,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async move {
                let observed = if self.leak_secret {
                    invocation.credentials[0].expose_for_injection().to_vec()
                } else {
                    b"safe diagnostic".to_vec()
                };
                Ok(ConformanceRuntimeObservationV1 {
                    terminal: ConformanceRuntimeTerminalV1::Succeeded,
                    result_body: b"ok".to_vec(),
                    observed_bytes: vec![ConformanceObservedBytesV1 {
                        channel: "stdout",
                        bytes: observed,
                    }],
                    cleanup_verified: self.cleanup_verified,
                    protocol_verified: true,
                    artifacts_verified: true,
                })
            })
        }
    }

    fn write_command_package(root: &Path) {
        let runtime = b"fixture";
        fs::create_dir_all(root.join("bin")).expect("bin");
        fs::write(root.join("bin/fixture.exe"), runtime).expect("runtime");
        let manifest = json!({
            "schema_version": 1,
            "package_key": "example.conformance",
            "package_version": "1.0.0",
            "minimum_aura_version": "0.1.0",
            "ui": { "display_name": "Conformance", "description": null },
            "files": [{
                "path": "bin/fixture.exe",
                "byte_length": runtime.len(),
                "sha256": ToolPackageFileSha256V1::sha256(runtime),
                "role": "runtime_executable"
            }],
            "tools": [{
                "tool_key": "echo",
                "semantic_version": null,
                "ui": { "display_name": "Echo" },
                "model": {
                    "canonical_suffix": "echo",
                    "description": "Echoes a local conformance value.",
                    "summary": "Echo a value",
                    "input_schema": {
                        "type": "object",
                        "properties": { "value": { "type": "string" } },
                        "required": ["value"],
                        "additionalProperties": false
                    },
                    "strict": true,
                    "result_contract": { "type": "utf8_text" }
                },
                "instance_configuration": {
                    "type": "object",
                    "properties": { "mode": { "type": "string" } },
                    "required": ["mode"],
                    "additionalProperties": false
                },
                "runtime": {
                    "type": "declarative_command",
                    "command": {
                        "type": "argv",
                        "executable": "bin/fixture.exe",
                        "arguments": [],
                        "working_directory": { "type": "package_root" },
                        "environment": [],
                        "credential_slot": {
                            "slot_key": "canary",
                            "environment_variable": "AURA_CANARY"
                        }
                    }
                },
                "capability_requirements": ["process_execute"],
                "resource_claims": [],
                "execution_semantics": {
                    "automatic_retry_allowed": true,
                    "repeat_is_idempotent": true,
                    "may_create_persistent_effects": false
                },
                "conformance_cases": [{
                    "name": "echo",
                    "arguments": { "value": "ok" }
                }]
            }]
        });
        fs::write(
            root.join(TOOL_PACKAGE_MANIFEST_NAME),
            serde_json::to_vec(&manifest).expect("manifest"),
        )
        .expect("manifest file");
    }

    fn prepared(source: &Path) -> (StagedToolPackageV1, PreparedConformanceRunV1) {
        let staging_parent = source.parent().expect("source parent");
        let staged = stage_directory_package_v1(source, staging_parent).expect("staged");
        let request = PrepareConformanceRunV1 {
            tool_conformance_run_id: ToolConformanceRunId::new(),
            draft: ConformanceDraftRevisionRefV1 {
                tool_draft_id: ToolDraftId::new(),
                draft_revision: aura_domain_common::Revision::INITIAL,
                draft_snapshot_hash: CanonicalSha256HashV1::sha256(b"draft"),
            },
            tool_key: ToolKey::new("echo").expect("tool key"),
            conformance_case: ToolPackageConformanceCaseNameV1::new("echo").expect("case"),
            instance_configuration: Some(json!({"mode": "test"})),
            fixture_endpoint_bindings: Vec::new(),
            fixture_responses: Vec::new(),
        };
        let prepared = prepare_conformance_run_v1(&staged, request).expect("prepare");
        (staged, prepared)
    }

    #[tokio::test]
    async fn valid_isolated_case_passes_and_secret_is_not_serialized() {
        let source = tempfile::tempdir().expect("source");
        write_command_package(source.path());
        let (staged, prepared) = prepared(source.path());
        let binding_bytes = serde_json::to_vec(prepared.binding()).expect("binding");
        assert!(
            !binding_bytes
                .windows(SYNTHETIC_SECRET_PREFIX.len())
                .any(|window| { window == SYNTHETIC_SECRET_PREFIX })
        );
        let result = execute_prepared_conformance_run_v1(
            &prepared,
            &staged,
            &Host { leak_secret: false, cleanup_verified: true },
        )
        .await
        .expect("execute");
        assert_eq!(result.status, ConformanceCaseStatusV1::Passed);
        assert!(!result.secret_leak_detected);
    }

    #[tokio::test]
    async fn leaked_canary_fails_and_uncertain_cleanup_requires_recovery() {
        let source = tempfile::tempdir().expect("source");
        write_command_package(source.path());
        let (staged, prepared) = prepared(source.path());
        let leaked = execute_prepared_conformance_run_v1(
            &prepared,
            &staged,
            &Host { leak_secret: true, cleanup_verified: true },
        )
        .await
        .expect("leak result");
        assert_eq!(leaked.status, ConformanceCaseStatusV1::Failed);
        assert!(leaked.secret_leak_detected);

        let uncertain = execute_prepared_conformance_run_v1(
            &prepared,
            &staged,
            &Host { leak_secret: false, cleanup_verified: false },
        )
        .await
        .expect("uncertain result");
        assert_eq!(uncertain.status, ConformanceCaseStatusV1::RecoveryRequired);
    }

    #[test]
    fn missing_or_schema_invalid_instance_configuration_fails_before_runtime() {
        let source = tempfile::tempdir().expect("source");
        write_command_package(source.path());
        let staging_parent = source.path().parent().expect("source parent");
        let staged = stage_directory_package_v1(source.path(), staging_parent).expect("staged");
        for instance_configuration in [None, Some(json!({"mode": 42}))] {
            let result = prepare_conformance_run_v1(
                &staged,
                PrepareConformanceRunV1 {
                    tool_conformance_run_id: ToolConformanceRunId::new(),
                    draft: ConformanceDraftRevisionRefV1 {
                        tool_draft_id: ToolDraftId::new(),
                        draft_revision: aura_domain_common::Revision::INITIAL,
                        draft_snapshot_hash: CanonicalSha256HashV1::sha256(b"draft"),
                    },
                    tool_key: ToolKey::new("echo").expect("tool key"),
                    conformance_case: ToolPackageConformanceCaseNameV1::new("echo").expect("case"),
                    instance_configuration,
                    fixture_endpoint_bindings: Vec::new(),
                    fixture_responses: Vec::new(),
                },
            );
            let Err(error) = result else {
                panic!("invalid configuration was accepted");
            };
            assert!(matches!(
                error,
                ToolConformancePreparationErrorV1::InstanceConfigurationInvalid
            ));
        }
    }
}
