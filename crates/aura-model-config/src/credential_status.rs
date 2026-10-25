use aura_domain_common::{
    CredentialId, CredentialRevision, EndpointId, NormalizedIssueCode, Revision,
};
use aura_model_domain::{
    EndpointAuthScheme, EndpointCredentialState, EndpointCredentialStatus, ModelCredentialSlot,
};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialAvailability {
    Available,
    Unavailable,
}

/// Internal redacted facts read from P02's `credential_records` table.
///
/// This type intentionally has no JSON/Schema implementation. It carries no
/// storage locator, target, binding fingerprint, Journal state, or Secret.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialSlotStatusFacts {
    pub endpoint_id: EndpointId,
    pub slot_name: ModelCredentialSlot,
    pub credential_id: CredentialId,
    pub state: EndpointCredentialState,
    pub last_committed_credential_revision: Option<CredentialRevision>,
    pub row_revision: Revision,
    pub error_code: Option<NormalizedIssueCode>,
}

#[derive(Debug, Error)]
pub enum ModelCredentialStatusError {
    #[error("authentication=none must not have a credential slot record")]
    UnexpectedCredentialSlot,
    #[error("a secret-bearing endpoint must have its credential slot allocated at creation")]
    MissingCredentialSlot,
    #[error("unconfigured credential record must not have a committed revision")]
    InvalidUnconfiguredRecord,
    #[error("configured credential record requires a last committed revision")]
    InvalidConfiguredRecord,
    #[error("cleared credential record requires a last committed revision")]
    InvalidClearedRecord,
    #[error("derived endpoint credential status is invalid: {0}")]
    InvalidStatus(String),
}

pub fn derive_endpoint_credential_status(
    auth_scheme: EndpointAuthScheme,
    facts: Option<&CredentialSlotStatusFacts>,
    availability: CredentialAvailability,
) -> Result<Option<EndpointCredentialStatus>, ModelCredentialStatusError> {
    if auth_scheme == EndpointAuthScheme::None {
        return if facts.is_none() {
            Ok(None)
        } else {
            Err(ModelCredentialStatusError::UnexpectedCredentialSlot)
        };
    }

    let facts = facts.ok_or(ModelCredentialStatusError::MissingCredentialSlot)?;
    match facts.state {
        EndpointCredentialState::Unconfigured
            if facts.last_committed_credential_revision.is_some() =>
        {
            return Err(ModelCredentialStatusError::InvalidUnconfiguredRecord);
        }
        EndpointCredentialState::Configured
            if facts.last_committed_credential_revision.is_none() =>
        {
            return Err(ModelCredentialStatusError::InvalidConfiguredRecord);
        }
        EndpointCredentialState::Cleared if facts.last_committed_credential_revision.is_none() => {
            return Err(ModelCredentialStatusError::InvalidClearedRecord);
        }
        _ => {}
    }

    let state = if facts.state == EndpointCredentialState::Configured
        && availability == CredentialAvailability::Unavailable
    {
        EndpointCredentialState::Unavailable
    } else {
        facts.state
    };
    let active_credential_revision = if state == EndpointCredentialState::Configured {
        facts.last_committed_credential_revision
    } else {
        None
    };
    let status = EndpointCredentialStatus {
        endpoint_id: facts.endpoint_id,
        slot_name: facts.slot_name,
        credential_id: facts.credential_id,
        state,
        active_credential_revision,
        last_committed_credential_revision: facts.last_committed_credential_revision,
        row_revision: facts.row_revision,
        error_code: facts.error_code.clone(),
    };
    status
        .validate()
        .map_err(|error| ModelCredentialStatusError::InvalidStatus(error.to_string()))?;
    Ok(Some(status))
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{CredentialId, CredentialRevision, EndpointId, Revision};
    use aura_model_domain::{EndpointAuthScheme, EndpointCredentialState, ModelCredentialSlot};

    use super::{
        CredentialAvailability, CredentialSlotStatusFacts, ModelCredentialStatusError,
        derive_endpoint_credential_status,
    };

    fn revision(value: u64) -> CredentialRevision {
        CredentialRevision::new(Revision::new(value)).expect("credential revision")
    }

    fn facts(
        state: EndpointCredentialState,
        last: Option<CredentialRevision>,
    ) -> CredentialSlotStatusFacts {
        CredentialSlotStatusFacts {
            endpoint_id: EndpointId::new(),
            slot_name: ModelCredentialSlot::ApiKey,
            credential_id: CredentialId::new(),
            state,
            last_committed_credential_revision: last,
            row_revision: Revision::new(4),
            error_code: None,
        }
    }

    #[test]
    fn authentication_none_has_no_outer_credential_status() {
        assert_eq!(
            derive_endpoint_credential_status(
                EndpointAuthScheme::None,
                None,
                CredentialAvailability::Unavailable
            )
            .expect("auth none"),
            None
        );
        assert!(matches!(
            derive_endpoint_credential_status(
                EndpointAuthScheme::None,
                Some(&facts(EndpointCredentialState::Unconfigured, None)),
                CredentialAvailability::Unavailable
            ),
            Err(ModelCredentialStatusError::UnexpectedCredentialSlot)
        ));
    }

    #[test]
    fn configured_requires_available_wincred_and_cleared_preserves_history() {
        let committed = facts(EndpointCredentialState::Configured, Some(revision(2)));
        let configured = derive_endpoint_credential_status(
            EndpointAuthScheme::OpenAiBearerApiKey,
            Some(&committed),
            CredentialAvailability::Available,
        )
        .expect("status")
        .expect("slot");
        assert_eq!(configured.state, EndpointCredentialState::Configured);
        assert_eq!(configured.active_credential_revision, Some(revision(2)));

        let unavailable = derive_endpoint_credential_status(
            EndpointAuthScheme::OpenAiBearerApiKey,
            Some(&committed),
            CredentialAvailability::Unavailable,
        )
        .expect("status")
        .expect("slot");
        assert_eq!(unavailable.state, EndpointCredentialState::Unavailable);
        assert_eq!(unavailable.active_credential_revision, None);
        assert_eq!(unavailable.last_committed_credential_revision, Some(revision(2)));

        let cleared = derive_endpoint_credential_status(
            EndpointAuthScheme::OpenAiBearerApiKey,
            Some(&facts(EndpointCredentialState::Cleared, Some(revision(2)))),
            CredentialAvailability::Unavailable,
        )
        .expect("status")
        .expect("slot");
        assert_eq!(cleared.state, EndpointCredentialState::Cleared);
        assert_eq!(cleared.active_credential_revision, None);
        assert_eq!(cleared.last_committed_credential_revision, Some(revision(2)));
    }

    #[test]
    fn secret_endpoint_without_preallocated_slot_fails_closed() {
        assert!(matches!(
            derive_endpoint_credential_status(
                EndpointAuthScheme::AnthropicApiKey,
                None,
                CredentialAvailability::Unavailable
            ),
            Err(ModelCredentialStatusError::MissingCredentialSlot)
        ));
    }
}
