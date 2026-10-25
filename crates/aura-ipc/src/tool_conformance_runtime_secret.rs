use std::fmt;

use aura_domain_common::SandboxExecutionId;
use aura_domain_common::jcs::{
    CanonicalHashV1, CanonicalSha256HashV1, canonical_json_jcs, parse_json_strict,
};
use aura_domain_common::{ToolCallId, ToolConformanceRunId, ToolExecutionAttemptId};
use aura_tool_contract::{CredentialSlotKey, EnvironmentVariableNameV1};
use aura_windows_platform::{MAX_CREDENTIAL_BYTES, SecretBytes};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

pub const TOOL_CONFORMANCE_RUNTIME_SECRET_FRAME_VERSION: u8 = 1;
const TOOL_CONFORMANCE_RUNTIME_SECRET_LENGTH_HEADER_BYTES: usize = 1 + 4 + 4;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ToolConformanceRuntimeSecretMetadataV1 {
    tool_conformance_run_id: ToolConformanceRunId,
    sandbox_execution_id: SandboxExecutionId,
    tool_call_id: ToolCallId,
    tool_execution_attempt_id: ToolExecutionAttemptId,
    slot_key: CredentialSlotKey,
    environment_variable: EnvironmentVariableNameV1,
    synthetic_credential_binding_hash: CanonicalSha256HashV1,
    runtime_binding_hash: CanonicalHashV1,
}

/// Dedicated Core-to-Sandbox-Host secret frame for one W3 synthetic
/// Credential. It contains no reusable CredentialId, has no Serde contract,
/// and zeroizes both encoded and decoded plaintext buffers.
pub struct ToolConformanceRuntimeSecretFrameV1 {
    metadata: ToolConformanceRuntimeSecretMetadataV1,
    canonical_metadata: Vec<u8>,
    secret: SecretBytes,
}

impl ToolConformanceRuntimeSecretFrameV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tool_conformance_run_id: ToolConformanceRunId,
        sandbox_execution_id: SandboxExecutionId,
        tool_call_id: ToolCallId,
        tool_execution_attempt_id: ToolExecutionAttemptId,
        slot_key: CredentialSlotKey,
        environment_variable: EnvironmentVariableNameV1,
        synthetic_credential_binding_hash: CanonicalSha256HashV1,
        runtime_binding_hash: CanonicalHashV1,
        secret: Vec<u8>,
    ) -> Result<Self, ToolConformanceRuntimeSecretValidationError> {
        if secret.is_empty() {
            return Err(ToolConformanceRuntimeSecretValidationError::Empty);
        }
        if secret.len() > MAX_CREDENTIAL_BYTES {
            return Err(ToolConformanceRuntimeSecretValidationError::TooLarge);
        }
        let value = std::str::from_utf8(&secret)
            .map_err(|_| ToolConformanceRuntimeSecretValidationError::InvalidUtf8)?;
        if value.contains('\0') {
            return Err(ToolConformanceRuntimeSecretValidationError::ProhibitedNul);
        }
        let metadata = ToolConformanceRuntimeSecretMetadataV1 {
            tool_conformance_run_id,
            sandbox_execution_id,
            tool_call_id,
            tool_execution_attempt_id,
            slot_key,
            environment_variable,
            synthetic_credential_binding_hash,
            runtime_binding_hash,
        };
        let canonical_metadata = canonical_metadata_bytes(&metadata)?;
        Ok(Self { metadata, canonical_metadata, secret: SecretBytes::new(secret) })
    }

    #[must_use]
    pub const fn tool_conformance_run_id(&self) -> ToolConformanceRunId {
        self.metadata.tool_conformance_run_id
    }

    #[must_use]
    pub const fn sandbox_execution_id(&self) -> SandboxExecutionId {
        self.metadata.sandbox_execution_id
    }

    #[must_use]
    pub const fn tool_call_id(&self) -> ToolCallId {
        self.metadata.tool_call_id
    }

    #[must_use]
    pub const fn tool_execution_attempt_id(&self) -> ToolExecutionAttemptId {
        self.metadata.tool_execution_attempt_id
    }

    #[must_use]
    pub fn slot_key(&self) -> &CredentialSlotKey {
        &self.metadata.slot_key
    }

    #[must_use]
    pub fn environment_variable(&self) -> &EnvironmentVariableNameV1 {
        &self.metadata.environment_variable
    }

    #[must_use]
    pub fn synthetic_credential_binding_hash(&self) -> &CanonicalSha256HashV1 {
        &self.metadata.synthetic_credential_binding_hash
    }

    #[must_use]
    pub fn runtime_binding_hash(&self) -> &CanonicalHashV1 {
        &self.metadata.runtime_binding_hash
    }

    #[must_use]
    pub fn secret(&self) -> &SecretBytes {
        &self.secret
    }

    #[must_use]
    pub fn into_secret(self) -> SecretBytes {
        self.secret
    }

    pub(crate) fn payload_len(&self) -> usize {
        TOOL_CONFORMANCE_RUNTIME_SECRET_LENGTH_HEADER_BYTES
            + self.canonical_metadata.len()
            + self.secret.expose().len()
    }
}

impl PartialEq for ToolConformanceRuntimeSecretFrameV1 {
    fn eq(&self, other: &Self) -> bool {
        self.metadata == other.metadata && self.secret.expose() == other.secret.expose()
    }
}

impl Eq for ToolConformanceRuntimeSecretFrameV1 {}

impl fmt::Debug for ToolConformanceRuntimeSecretFrameV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolConformanceRuntimeSecretFrameV1")
            .field("frame_version", &TOOL_CONFORMANCE_RUNTIME_SECRET_FRAME_VERSION)
            .field("tool_conformance_run_id", &self.metadata.tool_conformance_run_id)
            .field("sandbox_execution_id", &self.metadata.sandbox_execution_id)
            .field("tool_call_id", &self.metadata.tool_call_id)
            .field("tool_execution_attempt_id", &self.metadata.tool_execution_attempt_id)
            .field("slot_key", &self.metadata.slot_key)
            .field("environment_variable", &self.metadata.environment_variable)
            .field(
                "synthetic_credential_binding_hash",
                &self.metadata.synthetic_credential_binding_hash,
            )
            .field("runtime_binding_hash", &self.metadata.runtime_binding_hash)
            .field("secret", &"REDACTED")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ToolConformanceRuntimeSecretValidationError {
    #[error("ipc.tool_conformance_runtime_secret.empty")]
    Empty,
    #[error("ipc.tool_conformance_runtime_secret.too_large")]
    TooLarge,
    #[error("ipc.tool_conformance_runtime_secret.invalid_utf8")]
    InvalidUtf8,
    #[error("ipc.tool_conformance_runtime_secret.prohibited_nul")]
    ProhibitedNul,
    #[error("ipc.tool_conformance_runtime_secret.metadata_invalid")]
    MetadataInvalid,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("ipc.tool_conformance_runtime_secret.malformed")]
pub struct ToolConformanceRuntimeSecretDecodeError;

pub fn encode_tool_conformance_runtime_secret_payload(
    frame: &ToolConformanceRuntimeSecretFrameV1,
) -> Result<Zeroizing<Vec<u8>>, ToolConformanceRuntimeSecretValidationError> {
    let mut payload = Zeroizing::new(Vec::with_capacity(frame.payload_len()));
    append_tool_conformance_runtime_secret_payload(frame, &mut payload)?;
    Ok(payload)
}

pub(crate) fn append_tool_conformance_runtime_secret_payload(
    frame: &ToolConformanceRuntimeSecretFrameV1,
    payload: &mut Vec<u8>,
) -> Result<(), ToolConformanceRuntimeSecretValidationError> {
    let metadata_length = u32::try_from(frame.canonical_metadata.len())
        .map_err(|_| ToolConformanceRuntimeSecretValidationError::MetadataInvalid)?;
    let secret_length = u32::try_from(frame.secret.expose().len())
        .map_err(|_| ToolConformanceRuntimeSecretValidationError::TooLarge)?;
    payload.push(TOOL_CONFORMANCE_RUNTIME_SECRET_FRAME_VERSION);
    payload.extend_from_slice(&metadata_length.to_le_bytes());
    payload.extend_from_slice(&secret_length.to_le_bytes());
    payload.extend_from_slice(&frame.canonical_metadata);
    payload.extend_from_slice(frame.secret.expose());
    Ok(())
}

pub fn decode_tool_conformance_runtime_secret_payload(
    payload: &mut [u8],
) -> Result<ToolConformanceRuntimeSecretFrameV1, ToolConformanceRuntimeSecretDecodeError> {
    let result = (|| {
        if payload.len() < TOOL_CONFORMANCE_RUNTIME_SECRET_LENGTH_HEADER_BYTES
            || payload[0] != TOOL_CONFORMANCE_RUNTIME_SECRET_FRAME_VERSION
        {
            return Err(ToolConformanceRuntimeSecretDecodeError);
        }
        let metadata_length = u32::from_le_bytes(
            payload[1..5].try_into().map_err(|_| ToolConformanceRuntimeSecretDecodeError)?,
        ) as usize;
        let secret_length = u32::from_le_bytes(
            payload[5..9].try_into().map_err(|_| ToolConformanceRuntimeSecretDecodeError)?,
        ) as usize;
        if !(1..=MAX_CREDENTIAL_BYTES).contains(&secret_length)
            || payload.len()
                != TOOL_CONFORMANCE_RUNTIME_SECRET_LENGTH_HEADER_BYTES
                    .checked_add(metadata_length)
                    .and_then(|value| value.checked_add(secret_length))
                    .ok_or(ToolConformanceRuntimeSecretDecodeError)?
        {
            return Err(ToolConformanceRuntimeSecretDecodeError);
        }
        let metadata_end = TOOL_CONFORMANCE_RUNTIME_SECRET_LENGTH_HEADER_BYTES + metadata_length;
        let metadata_value = parse_json_strict(
            &payload[TOOL_CONFORMANCE_RUNTIME_SECRET_LENGTH_HEADER_BYTES..metadata_end],
        )
        .map_err(|_| ToolConformanceRuntimeSecretDecodeError)?;
        let metadata: ToolConformanceRuntimeSecretMetadataV1 =
            serde_json::from_value(metadata_value)
                .map_err(|_| ToolConformanceRuntimeSecretDecodeError)?;
        ToolConformanceRuntimeSecretFrameV1::new(
            metadata.tool_conformance_run_id,
            metadata.sandbox_execution_id,
            metadata.tool_call_id,
            metadata.tool_execution_attempt_id,
            metadata.slot_key,
            metadata.environment_variable,
            metadata.synthetic_credential_binding_hash,
            metadata.runtime_binding_hash,
            payload[metadata_end..].to_vec(),
        )
        .map_err(|_| ToolConformanceRuntimeSecretDecodeError)
    })();
    payload.zeroize();
    result
}

fn canonical_metadata_bytes(
    metadata: &ToolConformanceRuntimeSecretMetadataV1,
) -> Result<Vec<u8>, ToolConformanceRuntimeSecretValidationError> {
    let value = serde_json::to_value(metadata)
        .map_err(|_| ToolConformanceRuntimeSecretValidationError::MetadataInvalid)?;
    canonical_json_jcs(&value)
        .map_err(|_| ToolConformanceRuntimeSecretValidationError::MetadataInvalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(secret: &[u8]) -> ToolConformanceRuntimeSecretFrameV1 {
        ToolConformanceRuntimeSecretFrameV1::new(
            ToolConformanceRunId::from_bytes([1; 16]),
            SandboxExecutionId::from_bytes([2; 16]),
            ToolCallId::from_bytes([3; 16]),
            ToolExecutionAttemptId::from_bytes([4; 16]),
            CredentialSlotKey::new("api-key").expect("slot"),
            EnvironmentVariableNameV1::new("TOOL_API_KEY").expect("environment"),
            CanonicalSha256HashV1::sha256(b"credential binding"),
            CanonicalHashV1::sha256(b"runtime binding"),
            secret.to_vec(),
        )
        .expect("frame")
    }

    #[test]
    fn round_trip_is_exact_and_debug_is_redacted() {
        let frame = frame(b"synthetic-canary");
        let debug = format!("{frame:?}");
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("synthetic-canary"));
        let mut payload = encode_tool_conformance_runtime_secret_payload(&frame).expect("encode");
        let decoded = decode_tool_conformance_runtime_secret_payload(&mut payload).expect("decode");
        assert_eq!(decoded, frame);
        assert!(payload.iter().all(|byte| *byte == 0));
    }
}
