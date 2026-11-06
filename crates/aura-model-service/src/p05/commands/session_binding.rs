//! A conversation's model: the one the user chose in its selector, and the user's default model
//! for a conversation Aura made itself, as the composer gives a new conversation.
use super::{P05CommandDispatch, encode, journal_error, projection_error};
use crate::p05::P05Services;
use crate::p05::config_journal::ModelConfigJournalError;
use aura_domain_common::{
    CorrelationId, ExpectedRevision, ModelProfileId, NormalizedIssueCode, SessionId, UtcTimestamp,
};
use aura_model_config::ModelConfigError;
use aura_model_domain::{
    EndpointAuthScheme, EndpointCredentialState, ModelExecutionReadinessView,
    SessionModelBindingMutationResult, SessionModelBindingSetRequest,
};
use aura_persistence::CommitSessionModelBinding;
use aura_runtime_contract::IpcErrorEnvelope;

impl P05Services {
    /// Binds the conversation to the model asked for, or to none.
    pub(super) fn set_session_binding(
        &self,
        request: SessionModelBindingSetRequest,
        correlation_id: CorrelationId,
    ) -> Result<P05CommandDispatch, IpcErrorEnvelope> {
        let snapshot = self
            .model_config
            .snapshot()
            .map_err(|error| journal_error(error.into(), correlation_id))?;
        let current = self
            .persistence
            .session_model_binding_state(request.session_id)
            .map_err(|error| journal_error(error.into(), correlation_id))?;
        let readable_ref_at_binding = request
            .model_profile_id
            .map(|model_profile_id| -> Result<String, ModelConfigJournalError> {
                let profile = snapshot
                    .registry
                    .models
                    .get(&model_profile_id)
                    .ok_or(ModelConfigError::ProfileNotFound(model_profile_id))?;
                if !profile.enabled {
                    return Err(ModelConfigError::ProfileDisabled(model_profile_id).into());
                }
                if profile.effective_max_output_tokens().is_none() {
                    return Err(ModelConfigError::OutputLimitConfigurationRequired(
                        model_profile_id,
                    )
                    .into());
                }
                let endpoint = snapshot
                    .registry
                    .endpoints
                    .get(&profile.endpoint_id)
                    .ok_or(ModelConfigError::EndpointNotFound(model_profile_id))?;
                let provider = snapshot
                    .registry
                    .providers
                    .get(&endpoint.provider_id)
                    .ok_or(ModelConfigError::ProviderNotFound(model_profile_id))?;
                let credential_status = self.projections.active_credential_status(endpoint)?;
                if endpoint.auth_scheme != EndpointAuthScheme::None
                    && !credential_status
                        .as_ref()
                        .is_some_and(|status| status.state == EndpointCredentialState::Configured)
                {
                    return Err(ModelConfigError::InvalidExecutionCredentialBinding.into());
                }
                Ok(format!("{}/{}/{}", provider.slug, endpoint.slug, profile.provider_model_name))
            })
            .transpose()
            .map_err(|error| journal_error(error, correlation_id))?;
        let mut selector = self
            .projections
            .session_model_selector(&snapshot, request.session_id, current.binding)
            .map_err(|error| projection_error(error, correlation_id))?;
        selector.selected_model_profile_id = request.model_profile_id;
        selector.selected_model_execution_readiness = request
            .model_profile_id
            .and_then(|model_profile_id| {
                selector
                    .models
                    .iter()
                    .find(|model| model.model_profile_id == model_profile_id)
                    .map(|model| model.execution_readiness.clone())
            })
            .unwrap_or_else(|| ModelExecutionReadinessView {
                is_executable: false,
                issue_code: Some(
                    NormalizedIssueCode::new("configuration.model_profile_not_selected")
                        .expect("frozen selector issue code"),
                ),
            });
        let commit = self
            .persistence
            .commit_session_model_binding(CommitSessionModelBinding {
                session_id: request.session_id,
                expected_revision: request.expected_revision,
                model_profile_id: request.model_profile_id,
                readable_ref_at_binding,
                bound_at: UtcTimestamp::now(),
                correlation_id,
                atomic_group_id: None,
                selector_projection: selector,
            })
            .map_err(|error| journal_error(error.into(), correlation_id))?;
        let result = SessionModelBindingMutationResult {
            changed: commit.changed,
            revision: commit.revision,
            value: commit.binding,
        };
        Ok(P05CommandDispatch {
            value: encode(result, correlation_id)?,
            projection_changes: commit.projection_changes,
        })
    }

    /// Binds a conversation Aura made itself to the user's default model, as the composer binds
    /// a new conversation when it is first sent to. A conversation bound already keeps its model;
    /// without a default model it stays unbound, and its first message waits for the choice.
    pub fn bind_default_model(
        &self,
        session_id: SessionId,
        correlation_id: CorrelationId,
    ) -> Result<Option<ModelProfileId>, IpcErrorEnvelope> {
        let default = self
            .model_config
            .default_model_selection()
            .map_err(|error| journal_error(error.into(), correlation_id))?;
        let current = self
            .persistence
            .session_model_binding_state(session_id)
            .map_err(|error| journal_error(error.into(), correlation_id))?;
        if let Some(bound) = current.binding {
            return Ok(Some(bound.model_profile_id));
        }
        let Some(model_profile_id) = default.model_profile_id else { return Ok(None) };
        self.bind_model(session_id, model_profile_id, correlation_id)?;
        Ok(Some(model_profile_id))
    }

    /// Binds the conversation to the model at its binding's revision now, and publishes the
    /// conversation's selector, as choosing the model in the selector does.
    pub fn bind_model(
        &self,
        session_id: SessionId,
        model_profile_id: ModelProfileId,
        correlation_id: CorrelationId,
    ) -> Result<(), IpcErrorEnvelope> {
        let current = self
            .persistence
            .session_model_binding_state(session_id)
            .map_err(|error| journal_error(error.into(), correlation_id))?;
        let request = SessionModelBindingSetRequest {
            expected_revision: ExpectedRevision::new(current.revision.value()),
            session_id,
            model_profile_id: Some(model_profile_id),
        };
        let dispatch = self.set_session_binding(request, correlation_id)?;
        self.persistence
            .publish_model_projection_changes(&dispatch.projection_changes)
            .map_err(|error| journal_error(error.into(), correlation_id))
    }
}
