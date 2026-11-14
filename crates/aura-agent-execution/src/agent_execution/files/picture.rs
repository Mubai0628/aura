//! `fs.read` of a picture. A PNG, JPEG, GIF or WebP file goes to a model that takes images as the
//! picture itself, with the result, the way a browser screenshot does; any other model is told it
//! cannot see it. The file is opened the way its text would be read, so the same authority holds.
use super::*;
use aura_domain_common::{ArtifactId, UtcTimestamp};
use std::io::Read;

/// The largest picture sent, as for one the user attaches.
const LARGEST: u64 = aura_agent_domain::MAX_ATTACHED_IMAGE_BYTES;

/// What a file's first bytes say it is, when it is a picture a model can be sent.
pub(in crate::agent_execution) fn picture_type(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some("image/png")
    } else if head.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

pub(super) struct Picture {
    path: String,
    media_type: &'static str,
    size: u64,
    /// The bytes, when the picture is small enough to send.
    bytes: Option<Vec<u8>>,
}

/// The picture the call reads, when its file is one; `None` for any other file.
pub(super) fn open(service: &AgentExecution, call: &CallContext) -> Result<Option<Picture>> {
    let content = &call.operation.content;
    let input: AgentFileReadInput = serde_json::from_value(content.action.clone())?;
    let (path, file) = if let Some(grant_id) = external_grant(call) {
        let relative = input.path.into_relative();
        let record = service.external_grants.record(grant_id)?;
        let resolved = service.external_grants.resolve_existing(
            grant_id,
            call.snapshot.session_id,
            &relative,
            aura_permission_domain::ExternalGrantCapability::Read,
            aura_windows_platform::NativeFileAccess::Read,
            content.identity.correlation_id,
        )?;
        (join_display(&record.selected_root.display, &relative), resolved.file)
    } else if policy_external_path(call).is_some() {
        let (path, file) =
            open_authorized_external(call, aura_windows_platform::NativeFileAccess::Read)?;
        (path.display, file)
    } else {
        let relative = input.path.into_relative();
        let resolved =
            service.workspace.resolver(call.snapshot.workspace_id)?.resolve_existing(&relative)?;
        if resolved.info.is_directory {
            return Ok(None);
        }
        let file = aura_windows_platform::open_verified_existing(
            &resolved.absolute_path,
            aura_windows_platform::NativeFileAccess::Read,
        )?;
        (relative.as_str().to_owned(), file)
    };
    if file.info().is_directory {
        return Ok(None);
    }
    let mut head = Vec::with_capacity(12);
    file.file().take(12).read_to_end(&mut head)?;
    let Some(media_type) = picture_type(&head) else { return Ok(None) };
    let size = file.file().metadata()?.len();
    let bytes = if size <= LARGEST {
        let mut bytes = head;
        file.file().take(LARGEST + 1 - bytes.len() as u64).read_to_end(&mut bytes)?;
        anyhow::ensure!(bytes.len() as u64 == size, "agent.file_changed");
        Some(bytes)
    } else {
        None
    };
    Ok(Some(Picture { path, media_type, size, bytes }))
}

/// The result of reading `picture`. A picture the step's model is sent is stored for the
/// conversation and its run, as a screenshot is.
pub(super) fn result(
    service: &AgentExecution,
    call: &CallContext,
    picture: Picture,
) -> Result<Value> {
    let identity = &call.batch.invocation_identity;
    let takes_images = aura_persistence_v2::model::images::step_takes_images(
        service.journal.lock()?.database.connection(),
        identity.model_step_id,
    )?;
    let sent = picture.bytes.as_ref().filter(|_| takes_images);
    let id = sent.map(|_| ArtifactId::new());
    if let (Some(id), Some(bytes)) = (id, sent) {
        let owner = call.operation.operation_id.to_string();
        service.journal.lock()?.database.unit_of_work(|tx| {
            let at = UtcTimestamp::now();
            aura_persistence_core::artifact::put(tx, id, &owner, picture.media_type, bytes, at)?;
            for reader in [identity.session_id.to_string(), identity.agent_run_id.to_string()] {
                aura_persistence_core::artifact::grant_access(tx, id, &owner, &reader)?;
            }
            Ok::<(), aura_persistence_core::PersistenceCoreError>(())
        })?;
    }
    let note = if id.is_some() {
        "图片随这条结果发给你。"
    } else if !takes_images {
        "这是一张图片。当前模型不接收图片，看不到它的内容；需要时请用户描述。"
    } else {
        "图片大于 3 MB，没有发给你。"
    };
    Ok(serde_json::to_value(AgentFilePictureResult {
        path: picture.path,
        media_type: picture.media_type.to_owned(),
        byte_length: picture.size,
        shown_to_model: id.is_some(),
        image_artifact_id: id,
        note: note.to_owned(),
    })?)
}

#[cfg(test)]
mod tests {
    use super::picture_type;

    #[test]
    fn pictures_are_told_by_their_first_bytes() {
        assert_eq!(picture_type(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"), Some("image/png"));
        assert_eq!(picture_type(&[0xff, 0xd8, 0xff, 0xe0]), Some("image/jpeg"));
        assert_eq!(picture_type(b"GIF89a\x01\0"), Some("image/gif"));
        assert_eq!(picture_type(b"RIFF\x24\0\0\0WEBPVP8 "), Some("image/webp"));
        for other in [&b"PK\x03\x04"[..], b"%PDF-1.7", b"plain text", b"RIFF\0\0\0\0WAVE"] {
            assert_eq!(picture_type(other), None);
        }
    }
}
