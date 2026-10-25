//! One entry of a zip a
//! message of the conversation carried from outside the workspace, opened for its 文件 tab. Core
//! reads the zip through the conversation's read-only authorization for that one file, which also
//! proves it is still the zip attached, and copies the entry into Aura's own folder for the
//! conversation; the folder goes when the conversation is deleted.
use super::*;
use aura_agent_domain::{AttachmentEntryRequest, AttachmentLocation};
use aura_domain_common::WorkspacePath;
use aura_permission_domain::{ExternalGrantCapability, ExternalGrantTargetKind};
use aura_windows_platform::NativeFileAccess;
use sha2::Digest as _;
use std::io::{Read as _, Write as _};

/// The largest entry Aura copies out of a zip: copied within the 15 s Main waits for a request.
const LARGEST: u64 = 256 * 1024 * 1024;
/// The longest entry path asked for.
const LONGEST_ENTRY: usize = 1024;

fn refused(code: &str, why: &str) -> anyhow::Error {
    anyhow::anyhow!("agent.attachment_entry_{code}: {why}")
}

/// Where the conversations' opened zip entries are kept: next to the V2 database.
fn entries_root(journal: &crate::broker::BrokerJournal) -> anyhow::Result<std::path::PathBuf> {
    let database = journal.database_path()?;
    let data = database
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or_else(|| anyhow::anyhow!("agent.attachment_entries_root_unavailable"))?;
    Ok(data.join("v2-attachment-entries"))
}

/// A deleted conversation's opened zip entries go with it.
pub(crate) fn forget_entries(
    journal: &crate::broker::BrokerJournal,
    session: SessionId,
) -> anyhow::Result<()> {
    let folder = entries_root(journal)?.join(session.to_string());
    match std::fs::remove_dir_all(&folder) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

/// The entry's own name, as a file of Windows can be called.
fn file_name(entry: &str) -> String {
    let last = entry.rsplit('/').next().unwrap_or(entry);
    let name: String = last
        .chars()
        .map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c })
        .collect();
    let name = name.trim_end_matches(['.', ' ']).to_owned();
    if name.is_empty() { "entry".to_owned() } else { name }
}

impl AgentApplication {
    /// Copies one entry of an attached zip into Aura's folder for the conversation and says
    /// where it is. A copy already made of the same entry is used again.
    pub(crate) async fn attachment_entry(
        &self,
        input: AttachmentEntryRequest,
    ) -> anyhow::Result<AttachmentLocation> {
        anyhow::ensure!(
            !input.entry.is_empty()
                && input.entry.len() <= LONGEST_ENTRY
                && !input.entry.contains('\0'),
            refused("invalid", "压缩包里没有这个文件。")
        );
        let root = entries_root(&self.execution.journal)?.join(input.session_id.to_string());
        match (input.external_grant_id, input.workspace_path.clone()) {
            (Some(grant), None) => {
                let grants = Arc::clone(&self.execution.external_grants);
                tokio::task::spawn_blocking(move || {
                    let zip = granted_zip(&grants, input.session_id, grant)?;
                    copy_entry(&zip, &root.join(grant.to_string()), &input.entry, &[])
                })
                .await?
            }
            // A zip in the session's workspace,
            // read there; its copy is tied to that very file, so a zip replaced at the same place
            // is copied from again.
            (None, Some(path)) => {
                let workspace =
                    self.session_workspace(input.session_id).ok_or_else(missing_attachment)?;
                let resolver = self.execution.workspace.resolver(workspace)?;
                tokio::task::spawn_blocking(move || {
                    let logical = WorkspacePath::parse(&path).map_err(|_| missing_attachment())?;
                    let zip = resolver
                        .open_existing(&logical, NativeFileAccess::Read)
                        .map_err(|_| missing_attachment())?
                        .file;
                    anyhow::ensure!(!zip.info().is_directory, missing_attachment());
                    let modified = zip.file().metadata()?.modified().ok();
                    let made = format!(
                        "{path}|{:?}|{modified:?}|{}",
                        zip.info().identity,
                        zip.file().metadata()?.len()
                    );
                    copy_entry(&zip, &root.join("workspace"), &input.entry, made.as_bytes())
                })
                .await?
            }
            _ => Err(refused("invalid", "压缩包里没有这个文件。")),
        }
    }
}

fn missing_attachment() -> anyhow::Error {
    anyhow::anyhow!("agent.attachment_missing: 找不到这个附件，它可能已被移动、删除或替换。")
}

/// An attached zip outside the workspace, opened through the conversation's read-only
/// authorization for it, which also proves it is still the zip attached.
fn granted_zip(
    grants: &crate::external_grants::ExternalGrantService,
    session: SessionId,
    grant: aura_domain_common::ExternalGrantId,
) -> anyhow::Result<aura_windows_platform::VerifiedFile> {
    let record = grants.record(grant).map_err(|_| missing_attachment())?;
    anyhow::ensure!(
        record.session_id == session && record.target_kind == ExternalGrantTargetKind::File,
        missing_attachment()
    );
    let resolved = grants
        .resolve_existing(
            grant,
            session,
            &WorkspacePath::root(),
            ExternalGrantCapability::Read,
            NativeFileAccess::Read,
            aura_domain_common::CorrelationId::new(),
        )
        .map_err(|_| missing_attachment())?;
    anyhow::ensure!(!resolved.file.info().is_directory, missing_attachment());
    Ok(resolved.file)
}

/// Copies `entry` of the zip into its own folder under `folder` and says where it is. `made`
/// names the zip's version when the folder alone does not.
fn copy_entry(
    zip: &aura_windows_platform::VerifiedFile,
    folder: &std::path::Path,
    entry: &str,
    made: &[u8],
) -> anyhow::Result<AttachmentLocation> {
    let mut archive = ::zip::ZipArchive::new(zip.file())
        .map_err(|_| refused("unreadable", "这个压缩包读不出来。"))?;
    let mut found =
        archive.by_name(entry).map_err(|_| refused("missing", "压缩包里没有这个文件。"))?;
    anyhow::ensure!(found.is_file(), refused("folder", "这是压缩包里的文件夹。"));
    anyhow::ensure!(
        found.size() <= LARGEST,
        refused("large", "这个文件超过 256 MB，请解压后再打开。")
    );
    let size = found.size();
    // The entry's place in the zip (and the zip's version) names its folder: two entries of one
    // name keep their own.
    let mut digest = sha2::Sha256::new();
    digest.update(made);
    digest.update(entry.as_bytes());
    let place: String =
        digest.finalize().iter().take(8).map(|byte| format!("{byte:02x}")).collect();
    let folder = folder.join(place);
    let target = folder.join(file_name(entry));
    if std::fs::metadata(&target).is_ok_and(|kept| kept.is_file() && kept.len() == size) {
        return Ok(located(&target, size));
    }
    std::fs::create_dir_all(&folder)?;
    // Written beside the copy and renamed when whole: an interrupted copy is never taken for one.
    let mut out = tempfile::NamedTempFile::new_in(&folder)?;
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut written = 0_u64;
    loop {
        let read = found
            .read(&mut buffer)
            .map_err(|_| refused("unreadable", "压缩包里的这个文件读不出来。"))?;
        if read == 0 {
            break;
        }
        written += read as u64;
        anyhow::ensure!(written <= size, refused("unreadable", "压缩包里的这个文件读不出来。"));
        out.write_all(&buffer[..read])?;
    }
    out.flush()?;
    anyhow::ensure!(written == size, refused("unreadable", "压缩包里的这个文件读不出来。"));
    out.persist(&target)?;
    Ok(located(&target, size))
}

fn located(path: &std::path::Path, byte_length: u64) -> AttachmentLocation {
    AttachmentLocation {
        path: crate::agent_execution::protected_paths::strip_verbatim_prefix(
            &path.display().to_string(),
        ),
        byte_length,
    }
}

#[cfg(test)]
mod tests {
    use super::file_name;

    #[test]
    fn an_entry_is_copied_under_its_own_name_as_windows_allows_it() {
        assert_eq!(file_name("项目/src/main.c"), "main.c");
        assert_eq!(file_name("说明.md"), "说明.md");
        assert_eq!(file_name("a/b:c?.txt"), "b_c_.txt");
        assert_eq!(file_name("a/trailing. "), "trailing");
        assert_eq!(file_name("a/"), "entry");
    }
}
