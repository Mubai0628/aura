//! What a
//! message carried, read again for the conversation's 文件 tab. A text file or a zip's listing
//! comes back from where the message keeps it; an attached file outside the workspace is opened
//! again through its read-only authorization, which also proves it is still the file attached.
use super::*;
use aura_agent_domain::{
    AttachmentLocateRequest, AttachmentLocation, AttachmentText, AttachmentTextRequest,
};
use aura_agent_execution::agent_execution;
use aura_domain_common::WorkspacePath;
use aura_permission_domain::{ExternalGrantCapability, ExternalGrantTargetKind};
use aura_windows_platform::NativeFileAccess;

impl AgentApplication {
    /// A text file or a zip's listing a message of this conversation carries.
    pub async fn attachment_text(
        &self,
        input: AttachmentTextRequest,
    ) -> anyhow::Result<AttachmentText> {
        let journal = Arc::clone(&self.execution.journal);
        let text = self
            .store
            .run_critical(
                StoreStep::operation("agent.attachments.text"),
                StoreAccess::Read,
                move || {
                    let journal = journal.lock()?;
                    Ok(aura_persistence_v2::agent::attachments::text(
                        journal.database.connection(),
                        input.session_id,
                        input.artifact_id,
                    )
                    .ok())
                },
            )
            .await?
            .ok_or_else(|| anyhow::anyhow!("agent.attachment_unavailable: 这个附件已不可用。"))?;
        Ok(AttachmentText { text })
    }

    /// Where an attached file outside the workspace is now. Core opens it through the
    /// conversation's read-only authorization for that one file, so a revoked authorization, a
    /// deleted file or a different file at the same place is refused.
    pub fn locate_attachment(
        &self,
        input: AttachmentLocateRequest,
    ) -> anyhow::Result<AttachmentLocation> {
        let grants = &self.execution.external_grants;
        let unavailable = || {
            anyhow::anyhow!(
                "agent.attachment_missing: 找不到这个附件，它可能已被移动、删除或替换。"
            )
        };
        let record = grants.record(input.external_grant_id).map_err(|_| unavailable())?;
        anyhow::ensure!(
            record.session_id == input.session_id
                && record.target_kind == ExternalGrantTargetKind::File,
            unavailable()
        );
        let resolved = grants
            .resolve_existing(
                input.external_grant_id,
                input.session_id,
                &WorkspacePath::root(),
                ExternalGrantCapability::Read,
                NativeFileAccess::Read,
                aura_domain_common::CorrelationId::new(),
            )
            .map_err(|_| unavailable())?;
        anyhow::ensure!(!resolved.file.info().is_directory, unavailable());
        let byte_length = resolved.file.file().metadata()?.len();
        // The short spelling of the same file: Explorer and other programs take it.
        let path = agent_execution::protected_paths::strip_verbatim_prefix(
            &resolved.file.info().final_path.display().to_string(),
        );
        Ok(AttachmentLocation { path, byte_length })
    }
}
