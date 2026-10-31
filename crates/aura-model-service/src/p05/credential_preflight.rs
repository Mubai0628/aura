use aura_model_domain::{
    EndpointAuthScheme, EndpointCredentialState, EndpointCredentialStatus, ModelCredentialSlot,
    ModelEndpointConfig,
};
use aura_network_control::ConfigurationRequestCredentialBindingV1;
use aura_persistence::PersistenceError;
use thiserror::Error;

use super::{P05Services, projection::P05ProjectionError};

pub struct ConfigurationCredentialPreflight {
    pub credential_status: Option<EndpointCredentialStatus>,
    pub credential_binding: Option<ConfigurationRequestCredentialBindingV1>,
}

#[derive(Debug, Error)]
pub enum ConfigurationCredentialPreflightError {
    #[error("the endpoint credential is not configured and available for its current binding")]
    Unavailable,
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(transparent)]
    Projection(#[from] P05ProjectionError),
}

impl P05Services {
    pub fn configuration_credential_preflight(
        &self,
        endpoint: &ModelEndpointConfig,
    ) -> Result<ConfigurationCredentialPreflight, ConfigurationCredentialPreflightError> {
        if endpoint.auth_scheme == EndpointAuthScheme::None {
            return Ok(ConfigurationCredentialPreflight {
                credential_status: None,
                credential_binding: None,
            });
        }
        let status = self
            .projections
            .active_credential_status(endpoint)?
            .ok_or(ConfigurationCredentialPreflightError::Unavailable)?;
        let credential_revision = status
            .active_credential_revision
            .ok_or(ConfigurationCredentialPreflightError::Unavailable)?;
        if status.state != EndpointCredentialState::Configured
            || status.last_committed_credential_revision != Some(credential_revision)
        {
            return Err(ConfigurationCredentialPreflightError::Unavailable);
        }
        let record = self
            .persistence
            .model_credential_record_for_endpoint(endpoint.id, ModelCredentialSlot::ApiKey)?
            .ok_or(ConfigurationCredentialPreflightError::Unavailable)?;
        if record.credential_id != status.credential_id
            || record.binding.auth_scheme != endpoint.auth_scheme
            || record.last_committed_credential_revision != Some(credential_revision)
        {
            return Err(ConfigurationCredentialPreflightError::Unavailable);
        }
        self.persistence
            .credential_for_model_transport(
                record.credential_id,
                credential_revision,
                &record.binding.credential_binding_fingerprint,
            )
            .map_err(|_| ConfigurationCredentialPreflightError::Unavailable)?;
        Ok(ConfigurationCredentialPreflight {
            credential_status: Some(status),
            credential_binding: Some(ConfigurationRequestCredentialBindingV1 {
                credential_id: record.credential_id,
                credential_revision,
                credential_binding_fingerprint: record.binding.credential_binding_fingerprint,
            }),
        })
    }
}
