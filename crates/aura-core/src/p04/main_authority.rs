use super::*;

impl P04Services {
    #[must_use]
    pub fn gates_authenticated_main_workspace_mutation(method_name: &str) -> bool {
        matches!(
            method_name,
            method::BUFFER_SAVE
                | method::FS_WRITE_FILE
                | method::FS_APPLY_PATCH
                | method::FS_CREATE_DIRECTORY
                | method::FS_COPY
                | method::FS_MOVE
                | method::FS_DELETE
                | method::FS_APPLY_CHANGESET
        )
    }

    pub fn authorize_authenticated_main_workspace_mutation(
        &self,
        method_name: &str,
        params: &Value,
        correlation_id: CorrelationId,
    ) -> Result<(), IpcErrorEnvelope> {
        let resolved = resolve_workspace_mutation(method_name, params, correlation_id)?;
        self.ensure_permission_state(resolved.session_id, correlation_id)
            .map_err(|error| persistence_error(error, correlation_id))?;
        let snapshot = self
            .persistence
            .create_permission_snapshot(resolved.session_id, None, correlation_id)
            .map_err(|error| persistence_error(error, correlation_id))?;
        let mut identity = InvocationIdentity::runtime_user(
            resolved.workspace_id,
            resolved.session_id,
            correlation_id,
        );
        identity.permission_snapshot_id = Some(snapshot.id);
        super::workspace_policy::authorize(
            method_name,
            params,
            identity,
            snapshot,
            resolved.capabilities,
            resolved.paths,
        )
        .map_err(|reason| {
            envelope(
                if reason == aura_security_contract::PolicyDenyReason::ApprovalRequired {
                    "permission.approval_required"
                } else {
                    "permission.denied"
                },
                ErrorCategory::Permission,
                format!("The authenticated user operation was denied: {reason:?}."),
                false,
                correlation_id,
            )
        })
    }

    /// Binds a closed Tool-settings mutation to the authenticated Main/Core
    /// channel. This authority is Core-only: neither the Renderer nor a model
    /// can deserialize or author the runtime/channel facts included here.
    pub fn authorize_authenticated_main_tool_mutation(
        &self,
        method_name: &str,
        params: &Value,
        command_invocation_id: CommandInvocationId,
        correlation_id: CorrelationId,
        channel: AuthenticatedConfigurationChannelIdentity,
    ) -> Result<ContentHash, IpcErrorEnvelope> {
        authenticated_tool_mutation_identity_hash(
            method_name,
            params,
            command_invocation_id,
            correlation_id,
            channel,
        )
    }
}

impl ResolvedWorkspaceMutation {
    pub(super) fn new(workspace_id: WorkspaceId, session_id: SessionId) -> Self {
        Self { workspace_id, session_id, capabilities: Vec::new(), paths: Vec::new() }
    }

    pub(super) fn capability(&mut self, capability: ResolvedCapability) {
        if !self.capabilities.contains(&capability) {
            self.capabilities.push(capability);
        }
    }

    pub(super) fn path(&mut self, path: WorkspacePath) {
        if !self.paths.contains(&path) {
            self.paths.push(path);
        }
    }
}

pub(crate) fn resolve_workspace_mutation(
    method_name: &str,
    params: &Value,
    correlation_id: CorrelationId,
) -> Result<ResolvedWorkspaceMutation, IpcErrorEnvelope> {
    match method_name {
        method::BUFFER_SAVE => {
            let request: BufferSaveCommand = decode(params, correlation_id)?;
            let mut resolved =
                ResolvedWorkspaceMutation::new(request.workspace_id, request.session_id);
            resolved.capability(ResolvedCapability::WorkspaceFileWrite);
            resolved.path(request.path);
            Ok(resolved)
        }
        method::FS_WRITE_FILE => {
            let request: FsWriteFileRequest = decode(params, correlation_id)?;
            let mut resolved =
                ResolvedWorkspaceMutation::new(request.workspace_id, request.session_id);
            resolved.capability(write_capability(&request.expected));
            resolved.path(request.target.path);
            Ok(resolved)
        }
        method::FS_APPLY_PATCH => {
            let request: FsApplyPatchRequest = decode(params, correlation_id)?;
            let mut resolved =
                ResolvedWorkspaceMutation::new(request.workspace_id, request.session_id);
            resolved.capability(ResolvedCapability::WorkspaceFileWrite);
            resolved.path(request.target.path);
            Ok(resolved)
        }
        method::FS_CREATE_DIRECTORY => {
            let request: FsCreateDirectoryRequest = decode(params, correlation_id)?;
            let mut resolved =
                ResolvedWorkspaceMutation::new(request.workspace_id, request.session_id);
            resolved.capability(ResolvedCapability::WorkspaceFileCreate);
            resolved.path(request.target.path);
            Ok(resolved)
        }
        method::FS_COPY => {
            let request: FsCopyRequest = decode(params, correlation_id)?;
            let mut resolved =
                ResolvedWorkspaceMutation::new(request.workspace_id, request.session_id);
            resolved.capability(ResolvedCapability::WorkspaceFileRead);
            resolved.capability(write_capability(&request.destination_expected));
            resolved.path(request.source.path);
            resolved.path(request.destination.path);
            Ok(resolved)
        }
        method::FS_MOVE => {
            let request: FsMoveRequest = decode(params, correlation_id)?;
            let mut resolved =
                ResolvedWorkspaceMutation::new(request.workspace_id, request.session_id);
            resolved.capability(ResolvedCapability::WorkspaceFileMove);
            resolved.path(request.source.path);
            resolved.path(request.destination.path);
            Ok(resolved)
        }
        method::FS_DELETE => {
            let request: FsDeleteRequest = decode(params, correlation_id)?;
            let mut resolved =
                ResolvedWorkspaceMutation::new(request.workspace_id, request.session_id);
            resolved.capability(ResolvedCapability::WorkspaceFileDelete);
            resolved.path(request.target.path);
            Ok(resolved)
        }
        method::FS_APPLY_CHANGESET => {
            let request: FsApplyChangesetRequest = decode(params, correlation_id)?;
            let mut resolved =
                ResolvedWorkspaceMutation::new(request.workspace_id, request.session_id);
            for operation in request.operations {
                match operation {
                    FsChangesetOperation::WriteFile { target, expected, .. } => {
                        resolved.capability(write_capability(&expected));
                        resolved.path(target.path);
                    }
                    FsChangesetOperation::ApplyPatch { target, .. } => {
                        resolved.capability(ResolvedCapability::WorkspaceFileWrite);
                        resolved.path(target.path);
                    }
                    FsChangesetOperation::CreateDirectory { target } => {
                        resolved.capability(ResolvedCapability::WorkspaceFileCreate);
                        resolved.path(target.path);
                    }
                    FsChangesetOperation::Copy {
                        source,
                        destination,
                        destination_expected,
                        ..
                    } => {
                        resolved.capability(ResolvedCapability::WorkspaceFileRead);
                        resolved.capability(write_capability(&destination_expected));
                        resolved.path(source.path);
                        resolved.path(destination.path);
                    }
                    FsChangesetOperation::Move { source, destination, .. } => {
                        resolved.capability(ResolvedCapability::WorkspaceFileMove);
                        resolved.path(source.path);
                        resolved.path(destination.path);
                    }
                    FsChangesetOperation::Delete { target, .. } => {
                        resolved.capability(ResolvedCapability::WorkspaceFileDelete);
                        resolved.path(target.path);
                    }
                }
            }
            if resolved.capabilities.is_empty() || resolved.paths.is_empty() {
                return Err(envelope(
                    "fs.invalid_request",
                    ErrorCategory::Validation,
                    "Workspace changeset must contain at least one resolved operation.",
                    false,
                    correlation_id,
                ));
            }
            Ok(resolved)
        }
        _ => Err(envelope(
            "ipc.method_not_found",
            ErrorCategory::NotFound,
            "The method is not an authenticated Workspace mutation.",
            false,
            correlation_id,
        )),
    }
}

pub(super) const fn write_capability(expected: &FsExpectedState) -> ResolvedCapability {
    match expected {
        FsExpectedState::Absent => ResolvedCapability::WorkspaceFileCreate,
        FsExpectedState::FileHash { .. } | FsExpectedState::Directory => {
            ResolvedCapability::WorkspaceFileWrite
        }
    }
}
