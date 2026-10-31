//! File selections are classified by the final path and identity of a readable handle.
use super::*;
use aura_security_service::external_grants;
use aura_windows_platform::{NativeFileAccess, final_path_within, open_verified_existing};

pub(super) fn prepare(
    resolver: Option<&aura_workspace::SecurePathResolver>,
    workspace: Option<WorkspaceId>,
    session: SessionId,
    command: CommandInvocationId,
    index: usize,
    path: &str,
    external: &external_grants::ExternalGrantService,
) -> anyhow::Result<(PreparedAttachment, Option<external_grants::AttachmentGrant>)> {
    let name = file_name(path).to_owned();
    anyhow::ensure!(
        std::path::Path::new(path).is_absolute(),
        "agent.attachment_path_invalid: 「{name}」的位置不可用，请重新选择文件。"
    );
    let mut file = open_verified_existing(std::path::Path::new(path), NativeFileAccess::Read)
        .map_err(|error| unopened(&name, &error))?;
    anyhow::ensure!(
        !file.info().is_directory,
        "agent.attachment_directory: 「{name}」是文件夹，请选择一个文件。"
    );
    let byte_length = file
        .file()
        .metadata()
        .map_err(|_| anyhow::anyhow!("agent.attachment_unreadable: 无法读取「{name}」。"))?
        .len();
    let listing = super::zip_listing::listing(path, file.file_mut());
    if let Some(resolver) = resolver {
        resolver.verify_root_identity()?;
        let root = resolver.resolve_existing(&WorkspacePath::parse(WorkspacePath::ROOT)?)?;
        if final_path_within(&root.info, file.info()) {
            let relative = file
                .info()
                .final_path
                .components()
                .skip(root.info.final_path.components().count())
                .map(|part| part.as_os_str().to_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    anyhow::anyhow!("agent.attachment_path_invalid: 「{name}」的位置不可用。")
                })?
                .join("/");
            let logical = WorkspacePath::parse(&relative)?;
            let found = resolver.resolve_existing(&logical)?;
            anyhow::ensure!(
                found.info.identity == file.info().identity,
                "agent.attachment_identity_changed: 「{name}」已被替换，请重新选择文件。"
            );
            return Ok((
                PreparedAttachment::File { name, path: relative, byte_length, listing },
                None,
            ));
        }
    }
    let workspace = workspace.ok_or_else(|| anyhow::anyhow!("agent.workspace_missing"))?;
    let mut hash = blake3::Hasher::new();
    hash.update(b"aura/l13.3/attachment-grant/v1");
    hash.update(&command.to_bytes());
    hash.update(&session.to_bytes());
    hash.update(&workspace.to_bytes());
    hash.update(&(index as u64).to_le_bytes());
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&hash.finalize().as_bytes()[..16]);
    let id = ExternalGrantId::from_bytes(bytes);
    let grant = external.prepare_attachment_read(id, session, workspace, path.to_owned(), file)?;
    Ok((
        PreparedAttachment::ExternalFile {
            name,
            path: path.to_owned(),
            byte_length,
            external_grant_id: id,
            listing,
        },
        Some(grant),
    ))
}

/// A chosen file that could not be opened, as the composer shows it: the reason in the user's
/// words, never the system's own (English) error text.
fn unopened(name: &str, error: &std::io::Error) -> anyhow::Error {
    match error.kind() {
        std::io::ErrorKind::NotFound => {
            anyhow::anyhow!("agent.attachment_missing: 找不到「{name}」。")
        }
        std::io::ErrorKind::PermissionDenied => anyhow::anyhow!(
            "agent.attachment_unreadable: 没有读取「{name}」的权限，请换一个位置的文件或调整它的权限后再附加。"
        ),
        _ => anyhow::anyhow!(
            "agent.attachment_unreadable: 无法读取「{name}」，它可能正被其他程序占用，关闭后再附加。"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::unopened;
    use std::io::{Error, ErrorKind};

    /// The composer shows Core's sentence as it is, so the system's own
    /// English error text must not be part of it; the reason is said in the user's words.
    #[test]
    fn an_unopened_file_is_explained_without_the_system_error_text() {
        for (error, reason) in [
            (Error::from_raw_os_error(5), "没有读取「clip.mp4」的权限"),
            (Error::from_raw_os_error(32), "正被其他程序占用"),
            (Error::new(ErrorKind::NotFound, "gone"), "找不到「clip.mp4」"),
        ] {
            let text = unopened("clip.mp4", &error).to_string();
            assert!(text.contains(reason), "{text}");
            assert!(!text.contains("os error") && !text.contains("gone"), "{text}");
        }
    }
}
