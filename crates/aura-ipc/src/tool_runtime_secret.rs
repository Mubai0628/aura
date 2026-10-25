use std::fmt;

use aura_domain_common::ToolExecutionAttemptId;
use aura_domain_common::jcs::{CanonicalHashV1, canonical_json_jcs, parse_json_strict};
use aura_domain_common::{CredentialId, CredentialRevision, SandboxExecutionId};
use aura_tool_contract::{CredentialSlotKey, EnvironmentVariableNameV1};
use aura_windows_platform::{MAX_CREDENTIAL_BYTES, SecretBytes};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

pub const TOOL_RUNTIME_SECRET_FRAME_VERSION: u8 = 1;
const TOOL_RUNTIME_SECRET_LENGTH_HEADER_BYTES: usize = 1 + 4 + 4;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ToolRuntimeSecretMetadataV1 {
    sandbox_execution_id: SandboxExecutionId,
    tool_execution_attempt_id: ToolExecutionAttemptId,
    credential_id: CredentialId,
    credential_revision: CredentialRevision,
    slot_key: CredentialSlotKey,
    environment_variable: EnvironmentVariableNameV1,
    runtime_binding_hash: CanonicalHashV1,
}

/// Dedicated Core-to-Sandbox-Host secret frame for one authorized Tool
/// attempt. This value has no Serde implementation, never enters control JSON,
/// and redacts both the secret and its byte length from Debug output.
pub struct ToolRuntimeSecretFrameV1 {
    metadata: ToolRuntimeSecretMetadataV1,
    canonical_metadata: Vec<u8>,
    secret: SecretBytes,
}

impl ToolRuntimeSecretFrameV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sandbox_execution_id: SandboxExecutionId,
        tool_execution_attempt_id: ToolExecutionAttemptId,
        credential_id: CredentialId,
        credential_revision: CredentialRevision,
        slot_key: CredentialSlotKey,
        environment_variable: EnvironmentVariableNameV1,
        runtime_binding_hash: CanonicalHashV1,
        secret: Vec<u8>,
    ) -> Result<Self, ToolRuntimeSecretValidationError> {
        if secret.is_empty() {
            return Err(ToolRuntimeSecretValidationError::Empty);
        }
        if secret.len() > MAX_CREDENTIAL_BYTES {
            return Err(ToolRuntimeSecretValidationError::TooLarge);
        }
        let value = std::str::from_utf8(&secret)
            .map_err(|_| ToolRuntimeSecretValidationError::InvalidUtf8)?;
        if value.contains('\0') {
            return Err(ToolRuntimeSecretValidationError::ProhibitedNul);
        }
        let metadata = ToolRuntimeSecretMetadataV1 {
            sandbox_execution_id,
            tool_execution_attempt_id,
            credential_id,
            credential_revision,
            slot_key,
            environment_variable,
            runtime_binding_hash,
        };
        let canonical_metadata = canonical_metadata_bytes(&metadata)?;
        Ok(Self { metadata, canonical_metadata, secret: SecretBytes::new(secret) })
    }

    #[must_use]
    pub const fn sandbox_execution_id(&self) -> SandboxExecutionId {
        self.metadata.sandbox_execution_id
    }

    #[must_use]
    pub const fn tool_execution_attempt_id(&self) -> ToolExecutionAttemptId {
        self.metadata.tool_execution_attempt_id
    }

    #[must_use]
    pub const fn credential_id(&self) -> CredentialId {
        self.metadata.credential_id
    }

    #[must_use]
    pub const fn credential_revision(&self) -> CredentialRevision {
        self.metadata.credential_revision
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
        TOOL_RUNTIME_SECRET_LENGTH_HEADER_BYTES
            + self.canonical_metadata.len()
            + self.secret.expose().len()
    }
}

impl PartialEq for ToolRuntimeSecretFrameV1 {
    fn eq(&self, other: &Self) -> bool {
        self.metadata == other.metadata && self.secret.expose() == other.secret.expose()
    }
}

impl Eq for ToolRuntimeSecretFrameV1 {}

impl fmt::Debug for ToolRuntimeSecretFrameV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolRuntimeSecretFrameV1")
            .field("frame_version", &TOOL_RUNTIME_SECRET_FRAME_VERSION)
            .field("sandbox_execution_id", &self.metadata.sandbox_execution_id)
            .field("tool_execution_attempt_id", &self.metadata.tool_execution_attempt_id)
            .field("credential_id", &self.metadata.credential_id)
            .field("credential_revision", &self.metadata.credential_revision)
            .field("slot_key", &self.metadata.slot_key)
            .field("environment_variable", &self.metadata.environment_variable)
            .field("runtime_binding_hash", &self.metadata.runtime_binding_hash)
            .field("secret", &"REDACTED")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ToolRuntimeSecretValidationError {
    #[error("ipc.tool_runtime_secret.empty")]
    Empty,
    #[error("ipc.tool_runtime_secret.too_large")]
    TooLarge,
    #[error("ipc.tool_runtime_secret.invalid_utf8")]
    InvalidUtf8,
    #[error("ipc.tool_runtime_secret.prohibited_nul")]
    ProhibitedNul,
    #[error("ipc.tool_runtime_secret.metadata_invalid")]
    MetadataInvalid,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("ipc.tool_runtime_secret.malformed")]
pub struct ToolRuntimeSecretDecodeError;

pub fn encode_tool_runtime_secret_payload(
    frame: &ToolRuntimeSecretFrameV1,
) -> Result<Zeroizing<Vec<u8>>, ToolRuntimeSecretValidationError> {
    let mut payload = Zeroizing::new(Vec::with_capacity(
        TOOL_RUNTIME_SECRET_LENGTH_HEADER_BYTES
            + frame.canonical_metadata.len()
            + frame.secret.expose().len(),
    ));
    append_payload(frame, &frame.canonical_metadata, &mut payload)?;
    Ok(payload)
}

pub(crate) fn append_tool_runtime_secret_payload(
    frame: &ToolRuntimeSecretFrameV1,
    payload: &mut Vec<u8>,
) -> Result<(), ToolRuntimeSecretValidationError> {
    append_payload(frame, &frame.canonical_metadata, payload)
}

fn append_payload(
    frame: &ToolRuntimeSecretFrameV1,
    metadata: &[u8],
    payload: &mut Vec<u8>,
) -> Result<(), ToolRuntimeSecretValidationError> {
    let metadata_length = u32::try_from(metadata.len())
        .map_err(|_| ToolRuntimeSecretValidationError::MetadataInvalid)?;
    let secret_length = u32::try_from(frame.secret.expose().len())
        .map_err(|_| ToolRuntimeSecretValidationError::TooLarge)?;
    payload.push(TOOL_RUNTIME_SECRET_FRAME_VERSION);
    payload.extend_from_slice(&metadata_length.to_le_bytes());
    payload.extend_from_slice(&secret_length.to_le_bytes());
    payload.extend_from_slice(metadata);
    payload.extend_from_slice(frame.secret.expose());
    Ok(())
}

pub fn decode_tool_runtime_secret_payload(
    payload: &mut [u8],
) -> Result<ToolRuntimeSecretFrameV1, ToolRuntimeSecretDecodeError> {
    let result = (|| {
        if payload.len() < TOOL_RUNTIME_SECRET_LENGTH_HEADER_BYTES
            || payload[0] != TOOL_RUNTIME_SECRET_FRAME_VERSION
        {
            return Err(ToolRuntimeSecretDecodeError);
        }
        let metadata_length =
            u32::from_le_bytes(payload[1..5].try_into().map_err(|_| ToolRuntimeSecretDecodeError)?)
                as usize;
        let secret_length =
            u32::from_le_bytes(payload[5..9].try_into().map_err(|_| ToolRuntimeSecretDecodeError)?)
                as usize;
        if !(1..=MAX_CREDENTIAL_BYTES).contains(&secret_length)
            || payload.len()
                != TOOL_RUNTIME_SECRET_LENGTH_HEADER_BYTES
                    .checked_add(metadata_length)
                    .and_then(|value| value.checked_add(secret_length))
                    .ok_or(ToolRuntimeSecretDecodeError)?
        {
            return Err(ToolRuntimeSecretDecodeError);
        }
        let metadata_end = TOOL_RUNTIME_SECRET_LENGTH_HEADER_BYTES + metadata_length;
        let metadata_value =
            parse_json_strict(&payload[TOOL_RUNTIME_SECRET_LENGTH_HEADER_BYTES..metadata_end])
                .map_err(|_| ToolRuntimeSecretDecodeError)?;
        let metadata: ToolRuntimeSecretMetadataV1 =
            serde_json::from_value(metadata_value).map_err(|_| ToolRuntimeSecretDecodeError)?;
        let secret = payload[metadata_end..].to_vec();
        ToolRuntimeSecretFrameV1::new(
            metadata.sandbox_execution_id,
            metadata.tool_execution_attempt_id,
            metadata.credential_id,
            metadata.credential_revision,
            metadata.slot_key,
            metadata.environment_variable,
            metadata.runtime_binding_hash,
            secret,
        )
        .map_err(|_| ToolRuntimeSecretDecodeError)
    })();
    payload.zeroize();
    result
}

fn canonical_metadata_bytes(
    metadata: &ToolRuntimeSecretMetadataV1,
) -> Result<Vec<u8>, ToolRuntimeSecretValidationError> {
    let value = serde_json::to_value(metadata)
        .map_err(|_| ToolRuntimeSecretValidationError::MetadataInvalid)?;
    canonical_json_jcs(&value).map_err(|_| ToolRuntimeSecretValidationError::MetadataInvalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_domain_common::{CredentialRevision, Revision};

    fn frame(secret: &[u8]) -> ToolRuntimeSecretFrameV1 {
        ToolRuntimeSecretFrameV1::new(
            SandboxExecutionId::from_bytes([1; 16]),
            ToolExecutionAttemptId::from_bytes([2; 16]),
            CredentialId::from_bytes([3; 16]),
            CredentialRevision::new(Revision::new(4)).expect("revision"),
            CredentialSlotKey::new("api-key").expect("slot"),
            EnvironmentVariableNameV1::new("TOOL_API_KEY").expect("environment"),
            CanonicalHashV1::sha256(b"runtime"),
            secret.to_vec(),
        )
        .expect("frame")
    }

    #[test]
    fn round_trip_is_exact_and_debug_never_exposes_secret_or_length() {
        let frame = frame(b"fixture-canary");
        let debug = format!("{frame:?}");
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("fixture-canary"));
        assert!(!debug.contains("secret_length"));
        assert!(!debug.contains("byte_length"));
        let mut payload = encode_tool_runtime_secret_payload(&frame).expect("encode");
        let decoded = decode_tool_runtime_secret_payload(&mut payload).expect("decode");
        assert_eq!(decoded.secret().expose(), b"fixture-canary");
        assert_eq!(decoded.runtime_binding_hash(), frame.runtime_binding_hash());
        assert!(payload.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn malformed_duplicate_metadata_and_invalid_secret_fail_closed() {
        assert!(matches!(
            ToolRuntimeSecretFrameV1::new(
                SandboxExecutionId::new(),
                ToolExecutionAttemptId::new(),
                CredentialId::new(),
                CredentialRevision::INITIAL,
                CredentialSlotKey::new("slot").expect("slot"),
                EnvironmentVariableNameV1::new("TOKEN").expect("environment"),
                CanonicalHashV1::sha256(b"runtime"),
                b"nul\0secret".to_vec(),
            ),
            Err(ToolRuntimeSecretValidationError::ProhibitedNul)
        ));

        let frame = frame(b"secret");
        let payload = encode_tool_runtime_secret_payload(&frame).expect("encode");
        let metadata_length = u32::from_le_bytes(payload[1..5].try_into().expect("length"));
        let metadata = &payload[9..9 + metadata_length as usize];
        let text = std::str::from_utf8(metadata).expect("metadata");
        let duplicate =
            text.replacen("{", "{\"sandbox_execution_id\":\"sbx_01040G2081040G2081040G2081\",", 1);
        let mut malformed = Vec::new();
        malformed.push(TOOL_RUNTIME_SECRET_FRAME_VERSION);
        malformed.extend_from_slice(&(duplicate.len() as u32).to_le_bytes());
        malformed.extend_from_slice(&6_u32.to_le_bytes());
        malformed.extend_from_slice(duplicate.as_bytes());
        malformed.extend_from_slice(b"secret");
        assert!(decode_tool_runtime_secret_payload(&mut malformed).is_err());
        assert!(malformed.iter().all(|byte| *byte == 0));
    }
}
