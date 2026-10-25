//! Each attachment a request carries is declared once among its egress sources, under the
//! artifact it is read from. A file or a shared page travels as an attachment and needs its own
//! grants (the egress manifest checks them); an image a tool returned (the built-in browser's
//! screenshot of the model's own page) travels as that tool's result, on the same basis as the
//! result's text. A picture the user sent with a message travels as that message, on the
//! user's own submission (the egress check confirms it is one).
use aura_context_domain::{ContextSourceRef, ModelEgressSourceKind};
use aura_model_domain::ModelAttachmentSnapshot;

use crate::{ContextPreparationError, ModelEgressSourceDraft};

pub(crate) fn validate_attachment_sources(
    attachments: &[ModelAttachmentSnapshot],
    sources: &[ModelEgressSourceDraft],
) -> Result<(), ContextPreparationError> {
    if attachments.len() != sources.len() {
        return Err(ContextPreparationError::AttachmentSourceInvalid);
    }
    for attachment in attachments {
        let matches = sources
            .iter()
            .filter(|source| {
                source.source_ref
                    == ContextSourceRef::Artifact { artifact_id: attachment.artifact_id }
                    && source.content_hash == attachment.content_hash
                    && source.byte_length == attachment.byte_length
                    && matches!(
                        source.source_kind,
                        ModelEgressSourceKind::ExternalFileAttachment
                            | ModelEgressSourceKind::ArtifactAttachment
                            | ModelEgressSourceKind::BrowserAttachment
                            | ModelEgressSourceKind::DiagnosticAttachment
                            | ModelEgressSourceKind::ToolResult
                            | ModelEgressSourceKind::UserEvent
                    )
            })
            .count();
        if matches != 1 {
            return Err(ContextPreparationError::AttachmentSourceInvalid);
        }
    }
    Ok(())
}
