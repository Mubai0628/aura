//! The pictures and files the
//! composer sends with a message, read and checked before the message is queued or started. A
//! picture must be the PNG or JPEG it says it is, for a model that takes images. A selected file
//! is classified by its checked native identity: workspace-relative, or one read-only external
//! file authorized for this conversation when the message is accepted.
use super::*;
use aura_agent_domain::{AgentAttachmentDraft, AttachmentError, validate_attachment_drafts};
use aura_persistence_v2::agent::attachments::PreparedAttachment;
use aura_security_service::external_grants;
use base64::Engine as _;
mod acceptance;
mod files;
mod zip_listing;

pub(super) struct PreparedAttachments {
    pub values: Vec<PreparedAttachment>,
    pub grants: Vec<external_grants::AttachmentGrant>,
}

/// The last part of a path as the user named the file, for a message about it.
fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).find(|part| !part.is_empty()).unwrap_or(path)
}

impl AgentApplication {
    /// The drafts read and checked; `model` is the one the message's turn uses.
    pub(super) async fn prepare_attachments(
        &self,
        command: CommandInvocationId,
        session: SessionId,
        workspace: Option<WorkspaceId>,
        model: Option<ModelProfileId>,
        drafts: &[AgentAttachmentDraft],
    ) -> anyhow::Result<PreparedAttachments> {
        if drafts.is_empty() {
            return Ok(PreparedAttachments { values: Vec::new(), grants: Vec::new() });
        }
        validate_attachment_drafts(drafts).map_err(|error| anyhow::anyhow!(error.to_string()))?;
        if drafts.iter().any(|draft| matches!(draft, AgentAttachmentDraft::Image { .. })) {
            anyhow::ensure!(
                self.model_takes_images(model)?,
                "agent.attachment_images_unsupported: 当前模型不接收图片：换一个接收图片的模型，或去掉图片后再发送。"
            );
        }
        let mut prepared = Vec::with_capacity(drafts.len());
        let mut grants = Vec::new();
        for (index, draft) in drafts.iter().enumerate() {
            prepared.push(match draft {
                AgentAttachmentDraft::Image { name, media_type, data_base64 } => {
                    let invalid =
                        || anyhow::anyhow!(AttachmentError::ImageInvalid(name.clone()).to_string());
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(data_base64)
                        .map_err(|_| invalid())?;
                    if super::browser_screenshot::image_media_type(&bytes)
                        != Some(media_type.as_str())
                    {
                        return Err(invalid());
                    }
                    PreparedAttachment::Image {
                        name: name.clone(),
                        media_type: media_type.clone(),
                        bytes,
                    }
                }
                AgentAttachmentDraft::Text { name, text } => {
                    PreparedAttachment::Text { name: name.clone(), text: text.clone() }
                }
                AgentAttachmentDraft::File { path } => {
                    let resolver =
                        workspace.map(|w| self.execution.workspace.resolver(w)).transpose()?;
                    let path = path.clone();
                    let external = Arc::clone(&self.execution.external_grants);
                    let (value, grant) = tokio::task::spawn_blocking(move || {
                        files::prepare(
                            resolver.as_deref(),
                            workspace,
                            session,
                            command,
                            index,
                            &path,
                            &external,
                        )
                    })
                    .await??;
                    if let Some(grant) = grant {
                        grants.push(grant);
                    }
                    value
                }
            });
        }
        Ok(PreparedAttachments { values: prepared, grants })
    }

    fn model_takes_images(&self, model: Option<ModelProfileId>) -> anyhow::Result<bool> {
        let Some(model) = model else {
            return Ok(false);
        };
        let config = self.p05.model_config.snapshot()?;
        Ok(config.registry.models.get(&model).is_some_and(|profile| {
            aura_model_domain::takes_images(&profile.request_defaults, profile.catalog.as_ref())
        }))
    }
}
