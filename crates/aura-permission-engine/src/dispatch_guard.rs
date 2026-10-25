use crate::policy::{PolicyInput, authorize};
use aura_execution_contract::{
    AuthorizedDispatch, BackendDescriptor, CancellationState, DispatchRequest,
    ExecutionContractError, SecuredOperation,
};
use aura_security_contract::{AuthorizationBasis, AuthorizationDecision, PolicyAuthorization};

/// Current facts must be freshly resolved by Core, not copied from the request.
/// Full content comparison covers exact source/Registry, action and plan
/// revisions, file identities and credential revisions. Store reservation is
/// still required before the returned dispatch may reach a Backend.
pub fn revalidate(
    request: DispatchRequest,
    original_authority: &AuthorizationDecision,
    current_operation: &SecuredOperation,
    input: &PolicyInput<'_>,
    backend: &BackendDescriptor,
    cancellation: &CancellationState,
) -> Result<AuthorizedDispatch, ExecutionContractError> {
    if &request.operation != current_operation || input.operation != current_operation {
        return Err(ExecutionContractError::FingerprintMismatch);
    }
    if matches!(original_authority.basis, AuthorizationBasis::Composite { .. }) {
        if crate::policy::assess(input).is_err()
            || !crate::network::validate_composite(input, backend, original_authority)
        {
            return Err(ExecutionContractError::AuthorizationMismatch);
        }
        return AuthorizedDispatch::revalidate(
            request,
            original_authority,
            backend,
            cancellation,
            input.now,
        );
    }
    let mut exact_input = input.clone();
    match &original_authority.basis {
        // A Policy caller must not convert a UI transport identity into an
        // Agent authorization. Core's closed UI adapter has its own live check.
        AuthorizationBasis::AuthenticatedMain { .. } => {
            return Err(ExecutionContractError::AuthorizationMismatch);
        }
        AuthorizationBasis::PermissionMode { snapshot_id, mode, session_permission_revision }
            if *snapshot_id != input.snapshot.id
                || *mode != input.snapshot.mode
                || *session_permission_revision != input.snapshot.session_permission_revision =>
        {
            return Err(ExecutionContractError::AuthorizationMismatch);
        }
        AuthorizationBasis::Grant { grant_id, revision } => {
            let current = input
                .grants
                .iter()
                .find(|g| g.grant_id == *grant_id)
                .ok_or(ExecutionContractError::AuthorizationMismatch)?;
            if current.revision != *revision
                || !current.is_live_at(input.now)
                || !crate::policy::explicit_grant_covers_operation(current, input)
            {
                return Err(ExecutionContractError::AuthorizationMismatch);
            }
            // Exact approval keeps exact identity. File and Native session
            // consent derive only the new operation fingerprint; every other
            // scope field must equal the stored, revision-checked grant.
            if aura_security_contract::AuthorizationScope::derive_for(
                &current.scope,
                &current_operation.fingerprint,
            )
            .as_ref()
                != Some(&original_authority.scope)
            {
                return Err(ExecutionContractError::AuthorizationMismatch);
            }
            exact_input.grants = std::slice::from_ref(current);
        }
        _ => {}
    }
    if (original_authority.scope.file_consent.is_none()
        && original_authority.scope.profile != input.profile)
        || !matches!(
            authorize(&exact_input, backend, Some(original_authority)),
            PolicyAuthorization::Allow { .. }
        )
    {
        return Err(ExecutionContractError::AuthorizationMismatch);
    }
    AuthorizedDispatch::revalidate(request, original_authority, backend, cancellation, input.now)
}
