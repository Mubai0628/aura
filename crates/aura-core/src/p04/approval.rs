use super::*;

impl P04Services {
    #[must_use]
    pub fn handles_main_command(method_name: &str) -> bool {
        aura_ipc::routes::owned_by(method_name, aura_ipc::routes::RouteOwner::Security)
    }

    pub async fn dispatch_main_command(
        &self,
        method_name: &str,
        params: &Value,
        correlation_id: CorrelationId,
    ) -> Option<Result<Value, IpcErrorEnvelope>> {
        if !Self::handles_main_command(method_name) {
            return None;
        }
        Some(self.dispatch_known_main_command(method_name, params, correlation_id).await)
    }

    pub fn on_workspace_restored(&self, workspace_id: WorkspaceId, correlation_id: CorrelationId) {
        // Failure leaves grants suspended or revoked, which is the safe state.
        let _ =
            self.external_grants.revalidate_suspended_for_workspace(workspace_id, correlation_id);
    }

    pub(super) async fn dispatch_known_main_command(
        &self,
        method_name: &str,
        params: &Value,
        correlation_id: CorrelationId,
    ) -> Result<Value, IpcErrorEnvelope> {
        match method_name {
            method::PERMISSION_GET_DEFAULTS => encode(
                self.permission_defaults()
                    .map_err(|error| internal_error(error, correlation_id))?,
                correlation_id,
            ),
            method::PERMISSION_SET_DEFAULTS => {
                let input = decode(params, correlation_id)?;
                encode(self.set_permission_defaults(input, correlation_id)?, correlation_id)
            }
            method::PERMISSION_GET_SESSION_STATE => {
                let request: GetSessionPermissionStateInput = decode(params, correlation_id)?;
                let value = self
                    .ensure_permission_state(request.session_id, correlation_id)
                    .map_err(|error| persistence_error(error, correlation_id))?;
                encode(value, correlation_id)
            }
            method::PERMISSION_SET_SESSION_MODE => {
                let request: aura_permission_domain::SetSessionPermissionModeInput =
                    decode(params, correlation_id)?;
                self.ensure_permission_state(request.session_id, correlation_id)
                    .map_err(|error| persistence_error(error, correlation_id))?;
                encode(
                    self.persistence
                        .change_session_permission_mode(
                            request.session_id,
                            request.expected_revision,
                            request.mode,
                            correlation_id,
                        )
                        .await
                        .map_err(|error| persistence_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::PERMISSION_LIST_GRANTS => {
                let request: ListPermissionGrantsInput = decode(params, correlation_id)?;
                encode(
                    self.persistence
                        .list_permission_grants(request.workspace_id, request.session_id)
                        .map_err(|error| persistence_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::NATIVE_SESSION_CONSENT_LIST | method::NATIVE_SESSION_CONSENT_REVOKE => {
                self.native_consent_command(method_name, params, correlation_id)
            }
            method::NETWORK_SESSION_CONSENT_LIST | method::NETWORK_SESSION_CONSENT_REVOKE => {
                self.network_consent_command(method_name, params, correlation_id)
            }
            method::FILE_SESSION_CONSENT_LIST | method::FILE_SESSION_CONSENT_REVOKE => {
                self.file_consent_command(method_name, params, correlation_id)
            }
            method::MEMORY_SESSION_CONSENT_LIST | method::MEMORY_SESSION_CONSENT_REVOKE => {
                self.memory_consent_command(method_name, params, correlation_id)
            }
            method::PERMISSION_REVOKE_GRANT => {
                let request: RevokePermissionGrantInput = decode(params, correlation_id)?;
                encode(
                    self.persistence
                        .revoke_permission_grant(request.grant_id, correlation_id)
                        .map_err(|error| persistence_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::APPROVAL_LIST_PENDING => {
                decode_empty(params, correlation_id)?;
                encode(
                    self.persistence
                        .pending_approvals()
                        .map_err(|error| persistence_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::APPROVAL_GET_DETAILS => {
                let request: GetApprovalDetailsInput = decode(params, correlation_id)?;
                encode(
                    self.persistence
                        .approval_details(request.approval_id)
                        .map_err(|error| persistence_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::APPROVAL_RESOLVE => {
                let request: ResolveApprovalInput = decode(params, correlation_id)?;
                encode(
                    self.persistence
                        .resolve_approval(
                            request.approval_id,
                            request.expected_request_revision,
                            request.decision,
                            correlation_id,
                        )
                        .map_err(|error| persistence_error(error, correlation_id))?
                        .resolution,
                    correlation_id,
                )
            }
            method::SECURITY_HOST_PREPARATION_STATUS => {
                decode_empty(params, correlation_id)?;
                encode(self.host_preparation_status(correlation_id)?, correlation_id)
            }
            method::SECURITY_HOST_PREPARATION_PROBE => {
                decode_empty(params, correlation_id)?;
                encode(
                    self.probe_and_persist(correlation_id)
                        .map_err(|error| internal_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::SECURITY_HOST_PREPARATION_PREPARE => {
                let request: HostPreparationMutationInput = decode(params, correlation_id)?;
                encode(
                    self.begin_host_preparation(
                        HostPreparationAction::Prepare,
                        request,
                        correlation_id,
                    )?,
                    correlation_id,
                )
            }
            method::SECURITY_HOST_PREPARATION_REPAIR => {
                let request: HostPreparationMutationInput = decode(params, correlation_id)?;
                encode(
                    self.begin_host_preparation(
                        HostPreparationAction::Repair,
                        request,
                        correlation_id,
                    )?,
                    correlation_id,
                )
            }
            method::SECURITY_HOST_PREPARATION_REMOVE => {
                let request: HostPreparationMutationInput = decode(params, correlation_id)?;
                encode(
                    self.begin_host_preparation(
                        HostPreparationAction::Remove,
                        request,
                        correlation_id,
                    )?,
                    correlation_id,
                )
            }
            method::SECURITY_HOST_PREPARATION_RESOLVE => {
                let request: ResolveHostPreparationInput = decode(params, correlation_id)?;
                encode(
                    self.resolve_host_preparation(request, correlation_id).await?,
                    correlation_id,
                )
            }
            method::SECURITY_QUARANTINE_LIST => {
                decode_empty(params, correlation_id)?;
                encode(
                    self.persistence
                        .list_quarantine()
                        .map_err(|error| persistence_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::SECURITY_EXTERNAL_GRANT_ATTACH_READ_ONLY => {
                let request: AttachExternalReadOnlyInput = decode(params, correlation_id)?;
                encode(
                    self.external_grants
                        .attach_read_only(request, correlation_id)
                        .map_err(|error| external_grant_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::SECURITY_EXTERNAL_GRANT_DERIVE_WORKSPACE => {
                let request: DeriveWorkspaceExternalGrantInput = decode(params, correlation_id)?;
                encode(
                    self.external_grants
                        .derive_workspace_permission(request, correlation_id)
                        .map_err(|error| external_grant_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::SECURITY_EXTERNAL_GRANT_LIST => {
                let request: ListSessionExternalGrantsInput = decode(params, correlation_id)?;
                encode(
                    self.external_grants
                        .list_session(request.session_id)
                        .map_err(|error| external_grant_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::SECURITY_EXTERNAL_GRANT_REVALIDATE => {
                let request: RevalidateExternalGrantInput = decode(params, correlation_id)?;
                encode(
                    self.external_grants
                        .revalidate(request.external_grant_id, request.session_id, correlation_id)
                        .map_err(|error| external_grant_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::SECURITY_EXTERNAL_GRANT_REVOKE => {
                let request: RevokeExternalGrantInput = decode(params, correlation_id)?;
                encode(
                    self.external_grants
                        .revoke(request.external_grant_id, request.session_id, correlation_id)
                        .map_err(|error| external_grant_error(error, correlation_id))?,
                    correlation_id,
                )
            }
            method::SECURITY_QUARANTINE_PREPARE_RELEASE => {
                let request: PrepareQuarantineReleaseInput = decode(params, correlation_id)?;
                encode(self.prepare_quarantine_release(request, correlation_id)?, correlation_id)
            }
            method::SECURITY_QUARANTINE_RELEASE => {
                self.dispatch_publication(method_name, params, correlation_id).await
            }
            method::SECURITY_QUARANTINE_ISSUE_DISCARD_CONFIRMATION => {
                let request: IssueQuarantineDiscardConfirmationInput =
                    decode(params, correlation_id)?;
                encode(
                    self.issue_quarantine_discard_confirmation(request, correlation_id)?,
                    correlation_id,
                )
            }
            method::SECURITY_QUARANTINE_DISCARD => {
                let request: DiscardQuarantineInput = decode(params, correlation_id)?;
                encode(self.discard_quarantine(request, correlation_id)?, correlation_id)
            }
            method::ARTIFACT_EXPORT => {
                self.dispatch_publication(method_name, params, correlation_id).await
            }
            _ => unreachable!("P04 command was checked before dispatch"),
        }
    }

    pub(crate) fn default_permission_mode(&self) -> Result<PermissionMode> {
        Ok(self.permission_defaults()?.mode)
    }
}
