use aura_domain_common::{
    ContentHash, CredentialId, CredentialRevision, EndpointId, NormalizedIssueCode, Revision,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointAuthScheme {
    None,
    OpenAiBearerApiKey,
    AnthropicApiKey,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointCredentialState {
    Unconfigured,
    Configured,
    Cleared,
    Unavailable,
    ReconciliationRequired,
}

/// Renderer-safe classification of the Endpoint's currently selected
/// authentication mode. The concrete wire/header scheme remains on
/// `ModelEndpointConfig`; this discriminant only controls active versus
/// dormant credential projection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointAuthenticationKind {
    None,
    Secret,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
pub enum ModelCredentialSlot {
    #[serde(rename = "api-key")]
    ApiKey,
}

pub const MODEL_API_KEY_MIN_BYTES: usize = 1;
pub const MODEL_API_KEY_MAX_BYTES: usize = 2_560;

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ModelApiKeyValidationError {
    #[error("model_credential.api_key_empty")]
    Empty,
    #[error("model_credential.api_key_too_large")]
    TooLarge,
    #[error("model_credential.api_key_invalid_utf8")]
    InvalidUtf8,
    #[error("model_credential.api_key_prohibited_character")]
    ProhibitedCharacter,
}

/// Validates the exact bytes accepted by the S3 API-key input path.
///
/// The function never trims or normalizes the value and its errors deliberately
/// omit byte counts and character details so they remain safe for no-log error
/// surfaces. An empty input means "do not replace" at the command boundary and
/// therefore is rejected before any credential mutation is prepared.
pub fn validate_model_api_key_bytes(bytes: &[u8]) -> Result<(), ModelApiKeyValidationError> {
    if bytes.len() < MODEL_API_KEY_MIN_BYTES {
        return Err(ModelApiKeyValidationError::Empty);
    }
    if bytes.len() > MODEL_API_KEY_MAX_BYTES {
        return Err(ModelApiKeyValidationError::TooLarge);
    }
    let value = std::str::from_utf8(bytes).map_err(|_| ModelApiKeyValidationError::InvalidUtf8)?;
    if value.chars().any(|character| {
        matches!(character, '\u{0000}'..='\u{001f}' | '\u{007f}' | '\u{2028}' | '\u{2029}')
    }) {
        return Err(ModelApiKeyValidationError::ProhibitedCharacter);
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CredentialBindingFingerprint(pub ContentHash);

/// Renderer-safe state for one stable Endpoint credential slot.
///
/// `CredentialId` is a non-secret logical identity, not an authority-bearing
/// capability. Storage locators, targets, binding fingerprints, Journal facts,
/// and Secret material are intentionally absent.
#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointCredentialStatus {
    pub endpoint_id: EndpointId,
    pub slot_name: ModelCredentialSlot,
    pub credential_id: CredentialId,
    pub state: EndpointCredentialState,
    pub active_credential_revision: Option<CredentialRevision>,
    pub last_committed_credential_revision: Option<CredentialRevision>,
    pub row_revision: Revision,
    pub error_code: Option<NormalizedIssueCode>,
}

/// Renderer-safe summary for a retained credential slot whose Endpoint is
/// currently configured with `auth_scheme = none`.
///
/// The summary preserves the distinction between a never-created slot and a
/// retained dormant slot without exposing an active revision, storage
/// locator, binding fingerprint, Journal state, or native platform error.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DormantEndpointCredentialSummary {
    pub endpoint_id: EndpointId,
    pub slot_name: ModelCredentialSlot,
    pub credential_id: CredentialId,
    pub retained_state: EndpointCredentialState,
    pub last_committed_credential_revision: Option<CredentialRevision>,
    pub row_revision: Revision,
    pub error_code: Option<NormalizedIssueCode>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum EndpointCredentialStatusError {
    #[error("unconfigured status must not contain an active or committed credential revision")]
    InvalidUnconfigured,
    #[error("configured status requires equal active and last committed credential revisions")]
    InvalidConfigured,
    #[error("cleared status requires no active revision and a last committed revision")]
    InvalidCleared,
    #[error("unavailable status must not contain an active credential revision")]
    InvalidUnavailable,
    #[error("reconciliation_required status must not contain an active credential revision")]
    InvalidReconciliationRequired,
}

impl EndpointCredentialStatus {
    pub fn validate(&self) -> Result<(), EndpointCredentialStatusError> {
        match self.state {
            EndpointCredentialState::Unconfigured
                if self.active_credential_revision.is_none()
                    && self.last_committed_credential_revision.is_none() =>
            {
                Ok(())
            }
            EndpointCredentialState::Configured
                if self.active_credential_revision.is_some()
                    && self.active_credential_revision
                        == self.last_committed_credential_revision =>
            {
                Ok(())
            }
            EndpointCredentialState::Cleared
                if self.active_credential_revision.is_none()
                    && self.last_committed_credential_revision.is_some() =>
            {
                Ok(())
            }
            EndpointCredentialState::Unavailable if self.active_credential_revision.is_none() => {
                Ok(())
            }
            EndpointCredentialState::ReconciliationRequired
                if self.active_credential_revision.is_none() =>
            {
                Ok(())
            }
            EndpointCredentialState::Unconfigured => {
                Err(EndpointCredentialStatusError::InvalidUnconfigured)
            }
            EndpointCredentialState::Configured => {
                Err(EndpointCredentialStatusError::InvalidConfigured)
            }
            EndpointCredentialState::Cleared => Err(EndpointCredentialStatusError::InvalidCleared),
            EndpointCredentialState::Unavailable => {
                Err(EndpointCredentialStatusError::InvalidUnavailable)
            }
            EndpointCredentialState::ReconciliationRequired => {
                Err(EndpointCredentialStatusError::InvalidReconciliationRequired)
            }
        }
    }
}

impl<'de> Deserialize<'de> for EndpointCredentialStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireStatus {
            endpoint_id: EndpointId,
            slot_name: ModelCredentialSlot,
            credential_id: CredentialId,
            state: EndpointCredentialState,
            active_credential_revision: Option<CredentialRevision>,
            last_committed_credential_revision: Option<CredentialRevision>,
            row_revision: Revision,
            error_code: Option<NormalizedIssueCode>,
        }

        let wire = WireStatus::deserialize(deserializer)?;
        let status = Self {
            endpoint_id: wire.endpoint_id,
            slot_name: wire.slot_name,
            credential_id: wire.credential_id,
            state: wire.state,
            active_credential_revision: wire.active_credential_revision,
            last_committed_credential_revision: wire.last_committed_credential_revision,
            row_revision: wire.row_revision,
            error_code: wire.error_code,
        };
        status.validate().map_err(D::Error::custom)?;
        Ok(status)
    }
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{CredentialId, CredentialRevision, EndpointId, Revision};
    use serde_json::json;

    use super::{
        EndpointCredentialState, EndpointCredentialStatus, ModelApiKeyValidationError,
        validate_model_api_key_bytes,
    };

    fn revision(value: u64) -> CredentialRevision {
        CredentialRevision::new(Revision::new(value)).expect("non-zero credential revision")
    }

    fn status(
        state: EndpointCredentialState,
        active: Option<CredentialRevision>,
        last: Option<CredentialRevision>,
    ) -> EndpointCredentialStatus {
        EndpointCredentialStatus {
            endpoint_id: EndpointId::new(),
            slot_name: super::ModelCredentialSlot::ApiKey,
            credential_id: CredentialId::new(),
            state,
            active_credential_revision: active,
            last_committed_credential_revision: last,
            row_revision: Revision::new(7),
            error_code: None,
        }
    }

    #[test]
    fn frozen_status_revision_matrix_is_enforced() {
        status(EndpointCredentialState::Unconfigured, None, None).validate().expect("unconfigured");
        status(EndpointCredentialState::Configured, Some(revision(2)), Some(revision(2)))
            .validate()
            .expect("configured");
        status(EndpointCredentialState::Cleared, None, Some(revision(2)))
            .validate()
            .expect("cleared");
        status(EndpointCredentialState::Unavailable, None, Some(revision(2)))
            .validate()
            .expect("unavailable");
        status(EndpointCredentialState::ReconciliationRequired, None, Some(revision(2)))
            .validate()
            .expect("reconciliation required");

        assert!(
            status(EndpointCredentialState::Configured, None, Some(revision(2)))
                .validate()
                .is_err()
        );
        assert!(
            status(EndpointCredentialState::Cleared, Some(revision(2)), Some(revision(2)))
                .validate()
                .is_err()
        );
        assert!(
            status(EndpointCredentialState::Unavailable, Some(revision(2)), Some(revision(2)))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn renderer_status_contains_no_storage_locator_or_binding_fingerprint() {
        let value = serde_json::to_value(status(
            EndpointCredentialState::Configured,
            Some(revision(1)),
            Some(revision(1)),
        ))
        .expect("status wire");
        let serialized = value.to_string().to_ascii_lowercase();
        for forbidden in
            ["credential_ref", "wincred", "target", "binding_fingerprint", "secret", "journal"]
        {
            assert!(!serialized.contains(forbidden), "unexpected {forbidden}");
        }

        let mut invalid = value;
        invalid["state"] = json!("cleared");
        assert!(serde_json::from_value::<EndpointCredentialStatus>(invalid).is_err());
    }

    #[test]
    fn api_key_validation_preserves_exact_utf8_bytes_and_rejects_frozen_controls() {
        for accepted in [b" leading-and-trailing ".as_slice(), "密钥-ß".as_bytes(), b".".as_slice()]
        {
            assert_eq!(validate_model_api_key_bytes(accepted), Ok(()));
        }
        assert_eq!(validate_model_api_key_bytes(b""), Err(ModelApiKeyValidationError::Empty));
        assert_eq!(validate_model_api_key_bytes(&vec![b'a'; 2_560]), Ok(()));
        assert_eq!(
            validate_model_api_key_bytes(&vec![b'a'; 2_561]),
            Err(ModelApiKeyValidationError::TooLarge)
        );
        assert_eq!(
            validate_model_api_key_bytes(&[0xff]),
            Err(ModelApiKeyValidationError::InvalidUtf8)
        );
        for rejected in ["a\nb", "a\u{7f}b", "a\u{2028}b", "a\u{2029}b"] {
            assert_eq!(
                validate_model_api_key_bytes(rejected.as_bytes()),
                Err(ModelApiKeyValidationError::ProhibitedCharacter)
            );
        }
    }
}
