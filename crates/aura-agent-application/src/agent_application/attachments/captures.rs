//! What a terminal printed or a page of the built-in browser showed, sent with a message, or an
//! MCP resource Core read for it. Core keeps it as a file in Aura's data next to the database and
//! attaches it as it attaches a file from outside the workspace: read only, for this
//! conversation, bound to the file it wrote. The model reads it by its path under that
//! authorization; without it, a read in Aura's data asks every time. The folder is named by the
//! conversation's handle, never its id: deleting a
//! conversation looks for its id in every record, and a path another conversation carries must
//! not make that conversation's records look like its own.
use super::*;
use aura_core_broker::broker;
use aura_security_service::external_grants;
use aura_windows_platform::{NativeFileAccess, open_verified_existing};
use std::io::Write as _;

/// Where the captures of every conversation are kept: next to the V2 database.
fn root(journal: &broker::BrokerJournal) -> anyhow::Result<std::path::PathBuf> {
    let database = journal.database_path()?;
    let data = database
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or_else(|| anyhow::anyhow!("agent.captures_root_unavailable"))?;
    Ok(data.join("v2-message-captures"))
}

fn folder(
    journal: &broker::BrokerJournal,
    session: SessionId,
) -> anyhow::Result<std::path::PathBuf> {
    Ok(root(journal)?.join(ConversationHandle::of(session).as_str()))
}

/// A deleted conversation's captures go with it.
pub(in crate::agent_application) fn forget(
    journal: &broker::BrokerJournal,
    session: SessionId,
) -> anyhow::Result<()> {
    match std::fs::remove_dir_all(folder(journal, session)?) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

/// The capture's name as a file of Windows can be called.
fn file_name(name: &str) -> String {
    let plain: String =
        name.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect();
    let plain = plain.trim_end_matches(['.', ' ']).to_owned();
    if plain.is_empty() { "capture.txt".to_owned() } else { plain }
}

/// The message and draft a capture belongs to.
pub(super) struct Owner {
    pub session: SessionId,
    pub workspace: Option<WorkspaceId>,
    pub command: CommandInvocationId,
    pub index: usize,
}

type Kept = (PreparedAttachment, external_grants::AttachmentGrant);

/// Writes the capture once for its command and draft (a repeated command finds the same file)
/// and authorizes reading it for this conversation.
pub(super) fn keep(
    journal: &broker::BrokerJournal,
    external: &external_grants::ExternalGrantService,
    owner: Owner,
    name: &str,
    text: &str,
) -> anyhow::Result<Kept> {
    let (id, place) = place(journal, &owner, name)?;
    write_once(&place, text, true)?;
    attach(external, &owner, id, place, name)
}

/// [`keep`] for contents Core reads itself: `read` runs the first time the command is accepted
/// only, and a repeated command keeps what it found then.
pub(super) fn keep_read(
    journal: &broker::BrokerJournal,
    external: &external_grants::ExternalGrantService,
    owner: Owner,
    name: &str,
    read: impl FnOnce() -> anyhow::Result<String>,
) -> anyhow::Result<Kept> {
    let (id, place) = place(journal, &owner, name)?;
    if !place.exists() {
        write_once(&place, &read()?, false)?;
    }
    attach(external, &owner, id, place, name)
}

/// The file a draft's capture is kept in and the id of its authorization, both the same for a
/// repeated command.
fn place(
    journal: &broker::BrokerJournal,
    owner: &Owner,
    name: &str,
) -> anyhow::Result<(ExternalGrantId, std::path::PathBuf)> {
    let workspace = owner.workspace.ok_or_else(|| anyhow::anyhow!("agent.workspace_missing"))?;
    let mut hash = blake3::Hasher::new();
    hash.update(b"aura/l16/message-capture/v1");
    hash.update(&owner.command.to_bytes());
    hash.update(&owner.session.to_bytes());
    hash.update(&workspace.to_bytes());
    hash.update(&(owner.index as u64).to_le_bytes());
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&hash.finalize().as_bytes()[..16]);
    let id = ExternalGrantId::from_bytes(bytes);
    let digits: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok((id, folder(journal, owner.session)?.join(digits).join(file_name(name))))
}

/// Writes the capture unless an earlier send of the same command did. Text the composer sent
/// must then be the same as before.
fn write_once(place: &std::path::Path, text: &str, sent: bool) -> anyhow::Result<()> {
    std::fs::create_dir_all(place.parent().expect("a capture's folder"))?;
    match std::fs::OpenOptions::new().write(true).create_new(true).open(place) {
        Ok(mut file) => {
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => anyhow::ensure!(
            !sent || std::fs::read(place)? == text.as_bytes(),
            "agent.capture_conflict: 这条消息的内容和第一次发送时不同，请重新发送。"
        ),
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn attach(
    external: &external_grants::ExternalGrantService,
    owner: &Owner,
    id: ExternalGrantId,
    place: std::path::PathBuf,
    name: &str,
) -> anyhow::Result<Kept> {
    let workspace = owner.workspace.ok_or_else(|| anyhow::anyhow!("agent.workspace_missing"))?;
    let byte_length = std::fs::metadata(&place)?.len();
    let file = open_verified_existing(&place, NativeFileAccess::Read)?;
    let path = aura_agent_execution::agent_execution::protected_paths::strip_verbatim_prefix(
        &file.info().final_path.display().to_string(),
    );
    let grant =
        external.prepare_attachment_read(id, owner.session, workspace, path.clone(), file)?;
    Ok((
        PreparedAttachment::ExternalFile {
            name: name.to_owned(),
            path,
            byte_length,
            external_grant_id: id,
            listing: None,
        },
        grant,
    ))
}
