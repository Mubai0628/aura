//! Credential material is read outside the network poll, under the prepared attempt's owner.
use super::{ToolCredentialService, ToolSecretError};
use aura_domain_common::ToolBatchId;
use aura_permission_domain::{ToolCredentialPlacementV1 as Placement, ToolCredentialUseV1};
use aura_persistence::PersistenceCoordinator;
use aura_tool_contract::{ToolDefinitionSnapshotRef, ToolRegistryScopeV1};
use aura_tool_domain::{ToolCredentialManagementTargetV1, ToolCredentialPlacementV1};
use aura_windows_platform::SecretBytes;

pub(super) fn read(
    persistence: &PersistenceCoordinator,
    definition: &ToolDefinitionSnapshotRef,
    batch: ToolBatchId,
    credential: &ToolCredentialUseV1,
) -> Result<SecretBytes, ToolSecretError> {
    let authority = persistence
        .load_exact_tool_execution_authority(definition, Some(batch))?
        .ok_or(ToolSecretError::NotAuthorized)?;
    let placement = match &credential.placement {
        Placement::Environment { environment_variable } => {
            ToolCredentialPlacementV1::CommandEnvironment {
                environment_variable: environment_variable.clone(),
            }
        }
        Placement::BearerAuthorization => ToolCredentialPlacementV1::HttpBearerAuthorization,
        Placement::FixedHeader { header_name } => {
            ToolCredentialPlacementV1::HttpFixedHeader { header_name: header_name.clone() }
        }
    };
    let snapshot = persistence.tool_authority_snapshot(None, None, None)?;
    let bound = snapshot
        .instance_credential_bindings
        .into_iter()
        .find(|b| {
            b.tool_instance_id == authority.instance.tool_instance_id
                && b.credential_slot_key == credential.slot_key
        })
        .ok_or(ToolSecretError::NotAuthorized)?;
    if bound.credential_id != credential.credential_id
        || bound.credential_revision != credential.credential_revision
        || bound.placement != placement
    {
        return Err(ToolSecretError::NotAuthorized);
    }
    let (workspace_id, session_id) = match &authority.registry.scope {
        ToolRegistryScopeV1::Workspace { workspace_id } => (Some(*workspace_id), None),
        ToolRegistryScopeV1::Session { workspace_id, session_id, .. }
        | ToolRegistryScopeV1::ChildAgent { workspace_id, session_id, .. } => {
            (Some(*workspace_id), Some(*session_id))
        }
    };
    let target = ToolCredentialManagementTargetV1 {
        tool_instance_id: authority.instance.tool_instance_id,
        expected_instance_revision: authority.instance.entity_revision,
        credential_slot_key: credential.slot_key.clone(),
        placement,
        workspace_id,
        session_id,
    };
    ToolCredentialService::new(std::sync::Arc::new(persistence.clone())).read_authorized(
        target,
        credential.credential_id,
        credential.credential_revision,
    )
}

pub(crate) struct HttpSecret {
    binding: ToolCredentialUseV1,
    value: SecretBytes,
    origin: aura_permission_domain::NormalizedOrigin,
}
impl HttpSecret {
    pub(crate) fn load_managed(
        persistence: &PersistenceCoordinator,
        target: ToolCredentialManagementTargetV1,
        binding: ToolCredentialUseV1,
        origin: aura_permission_domain::NormalizedOrigin,
    ) -> Result<Self, ToolSecretError> {
        let value = ToolCredentialService::new(std::sync::Arc::new(persistence.clone()))
            .read_authorized(target, binding.credential_id, binding.credential_revision)?;
        Ok(Self { binding, value, origin })
    }
    pub(crate) fn load(
        persistence: &PersistenceCoordinator,
        definition: &ToolDefinitionSnapshotRef,
        batch: ToolBatchId,
        binding: ToolCredentialUseV1,
        origin: aura_permission_domain::NormalizedOrigin,
    ) -> Result<Self, ToolSecretError> {
        let value = read(persistence, definition, batch, &binding)?;
        Ok(Self { binding, value, origin })
    }
}
impl aura_network_control::ToolCredentialResolver for HttpSecret {
    fn resolve(
        &self,
        binding: &ToolCredentialUseV1,
    ) -> Result<
        aura_network_control::ResolvedToolCredential,
        aura_network_control::NetworkControlError,
    > {
        use aura_network_control::{
            NetworkControlError, ResolvedToolCredential, ToolCredentialBinding,
        };
        if binding != &self.binding {
            return Err(NetworkControlError::CredentialBindingMismatch);
        }
        let header_name = match &binding.placement {
            Placement::BearerAuthorization => "authorization".into(),
            Placement::FixedHeader { header_name } => header_name.as_str().to_owned(),
            Placement::Environment { .. } => {
                return Err(NetworkControlError::CredentialBindingMismatch);
            }
        };
        let origin = self.origin.clone();
        let mut value = Vec::new();
        if matches!(binding.placement, Placement::BearerAuthorization) {
            value.extend_from_slice(b"Bearer ");
        }
        value.extend_from_slice(self.value.expose());
        Ok(ResolvedToolCredential {
            binding: ToolCredentialBinding {
                credential_id: binding.credential_id,
                credential_revision: binding.credential_revision,
                origin,
                header_name,
            },
            secret_header_value: value,
        })
    }
}
