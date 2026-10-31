//! Read-only attachment authority. A send binds one already-open file identity; it never
//! authorizes a directory, a replacement file, or a different conversation.
use super::*;

pub struct AttachmentGrant {
    pub record: ExternalGrantRecord,
    /// Keep the verified object open until the acceptance owner settles both stores.
    pub file: VerifiedFile,
}

fn record(
    id: ExternalGrantId,
    session: SessionId,
    workspace: WorkspaceId,
    selected: String,
    info: &NativePathInfo,
) -> ExternalGrantRecord {
    let now = UtcTimestamp::now();
    ExternalGrantRecord {
        id,
        session_id: session,
        workspace_id: workspace,
        permission_grant_id: None,
        selected_root: canonical_path(selected),
        canonical_root: canonical_path(info.final_path.display().to_string()),
        root_identity: domain_identity(&info.identity),
        target_kind: ExternalGrantTargetKind::File,
        capabilities: vec![ExternalGrantCapability::Read],
        recursive_scope: false,
        lifecycle: ExternalGrantLifecycle::Active,
        revalidation_revision: 1,
        created_at: now,
        updated_at: now,
        suspended_at: None,
        revoked_at: None,
        expires_at: None,
        turn_task_id: None,
        term: ExternalGrantTerm::Session,
    }
}

impl ExternalGrantService {
    pub fn attach_read_only(
        &self,
        input: AttachExternalReadOnlyInput,
        correlation: CorrelationId,
    ) -> Result<ExternalGrantView, ExternalGrantError> {
        self.ensure_session_workspace(input.session_id, input.workspace_id)?;
        let selected = PathBuf::from(&input.selected_path);
        if !selected.is_absolute() {
            return Err(ExternalGrantError::InvalidSelection);
        }
        let info = inspect_existing_path(&selected, true)?;
        if info.is_directory {
            return Err(ExternalGrantError::InvalidSelection);
        }
        let record = record(
            ExternalGrantId::new(),
            input.session_id,
            input.workspace_id,
            input.selected_path,
            &info,
        );
        Ok(ExternalGrantView::from(&self.persistence.create_external_grant(record, correlation)?))
    }

    pub fn prepare_attachment_read(
        &self,
        id: ExternalGrantId,
        session: SessionId,
        workspace: WorkspaceId,
        selected: String,
        file: VerifiedFile,
    ) -> Result<AttachmentGrant, ExternalGrantError> {
        self.ensure_session_workspace(session, workspace)?;
        if !Path::new(&selected).is_absolute() || file.info().is_directory {
            return Err(ExternalGrantError::InvalidSelection);
        }
        Ok(AttachmentGrant { record: record(id, session, workspace, selected, file.info()), file })
    }
}
