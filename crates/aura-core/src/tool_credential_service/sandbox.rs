//! Materialize only the credential frozen into an already authorized, prepared attempt.
use super::ToolSecretError;
use aura_ipc::ToolRuntimeSecretFrameV1;
use aura_permission_domain::{
    AuthorizedToolSandboxExecutionV1, ToolCredentialPlacementV1 as Placement,
};
use aura_persistence::PersistenceCoordinator;

pub(crate) fn secret_frame(
    persistence: &PersistenceCoordinator,
    execution: &AuthorizedToolSandboxExecutionV1,
) -> Result<Option<ToolRuntimeSecretFrameV1>, ToolSecretError> {
    execution.validate().map_err(|_| ToolSecretError::NotAuthorized)?;
    let Some(credential) = execution.operation.runtime.credential() else {
        return Ok(None);
    };
    let Placement::Environment { environment_variable } = &credential.placement else {
        return Err(ToolSecretError::NotAuthorized);
    };
    let secret = super::execution::read(
        persistence,
        &execution.operation.source.definition_ref,
        execution.binding.tool_batch_id,
        credential,
    )?;
    ToolRuntimeSecretFrameV1::new(
        execution.binding.sandbox_execution_id,
        execution.binding.tool_execution_attempt_id,
        credential.credential_id,
        credential.credential_revision,
        credential.slot_key.clone(),
        environment_variable.clone(),
        execution.binding.runtime_binding_hash.clone(),
        secret.expose().to_vec(),
    )
    .map(Some)
    .map_err(|_| ToolSecretError::NotAuthorized)
}
