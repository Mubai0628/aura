//! The pictures and files a
//! message carries. Pictures and text files are kept as artifacts of the conversation, so deleting
//! the conversation deletes them; the conversation reads them back to show them and to send them to
//! the model. When the message becomes part of the conversation its text is what the user typed
//! with its attachments as the model reads them, and the attachments are recorded with it, with how
//! much of that text the user typed.
use super::facts;
use aura_agent_domain::{
    AgentMessageAttachment, AgentPendingMessage, AttachedPart, OriginalUserMessage,
    message_with_attachments,
};
use aura_domain_common::{
    ArtifactId, ContentHash, EventId, ExternalGrantId, SessionId, UtcTimestamp,
};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact};
use rusqlite::{Connection, Transaction, params};
use serde::{Deserialize, Serialize};

const KIND: &str = "agent.message_attachments";
const TEXT_MEDIA_TYPE: &str = "text/plain; charset=utf-8";

/// One picture or file as Core read and checked it, ready to keep.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreparedAttachment {
    Image {
        name: String,
        media_type: String,
        bytes: Vec<u8>,
    },
    Text {
        name: String,
        text: String,
    },
    /// `path` is the file's place in the workspace, its parts joined by `/`.
    File {
        name: String,
        path: String,
        byte_length: u64,
        listing: Option<String>,
    },
    ExternalFile {
        name: String,
        path: String,
        byte_length: u64,
        external_grant_id: ExternalGrantId,
        listing: Option<String>,
    },
}

impl PreparedAttachment {
    /// The attachment as the model reads about it.
    #[must_use]
    pub fn part(&self) -> AttachedPart<'_> {
        match self {
            Self::Image { name, media_type, bytes } => {
                AttachedPart::Image { name, media_type, byte_length: bytes.len() as u64 }
            }
            Self::Text { name, text } => AttachedPart::Text { name, text },
            Self::File { path, byte_length, listing, .. } => {
                AttachedPart::File { path, byte_length: *byte_length, listing: listing.as_deref() }
            }
            Self::ExternalFile { path, byte_length, external_grant_id, listing, .. } => {
                AttachedPart::ExternalFile {
                    path,
                    byte_length: *byte_length,
                    external_grant_id: *external_grant_id,
                    listing: listing.as_deref(),
                }
            }
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Attached {
    typed_bytes: u64,
    attachments: Vec<AgentMessageAttachment>,
}

/// What the conversation's attachments are kept under: one of the owners deleting the
/// conversation deletes.
fn owner(session: SessionId) -> String {
    format!("agent-user:{session}")
}

/// Keeps the pictures and text files of a message about to be queued or started.
pub fn keep(
    tx: &Transaction<'_>,
    session: SessionId,
    prepared: &[PreparedAttachment],
    at: UtcTimestamp,
) -> StoreResult<Vec<AgentMessageAttachment>> {
    let owner = owner(session);
    let put = |media_type: &str, bytes: &[u8]| -> StoreResult<ArtifactId> {
        let id = ArtifactId::new();
        artifact::put(tx, id, &owner, media_type, bytes, at)?;
        // Read back by the conversation, as a browser call's screenshot is.
        artifact::grant_access(tx, id, &owner, &session.to_string())?;
        Ok(id)
    };
    prepared
        .iter()
        .map(|attachment| {
            Ok(match attachment {
                PreparedAttachment::Image { name, media_type, bytes } => {
                    AgentMessageAttachment::Image {
                        name: name.clone(),
                        media_type: media_type.clone(),
                        byte_length: bytes.len() as u64,
                        artifact_id: put(media_type, bytes)?,
                    }
                }
                PreparedAttachment::Text { name, text } => AgentMessageAttachment::Text {
                    name: name.clone(),
                    byte_length: text.len() as u64,
                    artifact_id: put(TEXT_MEDIA_TYPE, text.as_bytes())?,
                },
                PreparedAttachment::File { name, path, byte_length, listing } => {
                    AgentMessageAttachment::File {
                        name: name.clone(),
                        path: path.clone(),
                        byte_length: *byte_length,
                        listing_artifact_id: listing
                            .as_ref()
                            .map(|text| put(TEXT_MEDIA_TYPE, text.as_bytes()))
                            .transpose()?,
                    }
                }
                PreparedAttachment::ExternalFile {
                    name,
                    path,
                    byte_length,
                    external_grant_id,
                    listing,
                } => AgentMessageAttachment::ExternalFile {
                    name: name.clone(),
                    path: path.clone(),
                    byte_length: *byte_length,
                    external_grant_id: *external_grant_id,
                    listing_artifact_id: listing
                        .as_ref()
                        .map(|text| put(TEXT_MEDIA_TYPE, text.as_bytes()))
                        .transpose()?,
                },
            })
        })
        .collect()
}

/// The message's text as the model reads it: what the user typed with its attachments, the text
/// files read back from where they are kept.
pub fn message_text(
    db: &Connection,
    session: SessionId,
    typed: &str,
    attachments: &[AgentMessageAttachment],
) -> StoreResult<String> {
    let texts = attachments
        .iter()
        .map(|attachment| {
            let artifact_id = match attachment {
                AgentMessageAttachment::Text { artifact_id, .. } => Some(*artifact_id),
                AgentMessageAttachment::File { listing_artifact_id, .. }
                | AgentMessageAttachment::ExternalFile { listing_artifact_id, .. } => {
                    *listing_artifact_id
                }
                _ => None,
            };
            if let Some(artifact_id) = artifact_id {
                let bytes = artifact::read(db, artifact_id, &session.to_string())?;
                String::from_utf8(bytes).map(Some).map_err(|_| Error::Invalid("attachment text"))
            } else {
                Ok(None)
            }
        })
        .collect::<StoreResult<Vec<_>>>()?;
    let parts = attachments
        .iter()
        .zip(&texts)
        .map(|(attachment, text)| match attachment {
            AgentMessageAttachment::Image { name, media_type, byte_length, .. } => {
                AttachedPart::Image { name, media_type, byte_length: *byte_length }
            }
            AgentMessageAttachment::Text { name, .. } => {
                AttachedPart::Text { name, text: text.as_deref().unwrap_or_default() }
            }
            AgentMessageAttachment::File { path, byte_length, .. } => {
                AttachedPart::File { path, byte_length: *byte_length, listing: text.as_deref() }
            }
            AgentMessageAttachment::ExternalFile {
                path, byte_length, external_grant_id, ..
            } => AttachedPart::ExternalFile {
                path,
                byte_length: *byte_length,
                external_grant_id: *external_grant_id,
                listing: text.as_deref(),
            },
        })
        .collect::<Vec<_>>();
    Ok(message_with_attachments(typed, &parts))
}

/// A queued message as it becomes part of the conversation.
pub fn freeze(
    db: &Connection,
    message: &AgentPendingMessage,
    session: SessionId,
    at: UtcTimestamp,
) -> StoreResult<OriginalUserMessage> {
    let text = message_text(db, session, &message.text, &message.attachments)?;
    message.freeze_as(session, text, at).map_err(|_| Error::Conflict)
}

/// Records a message's attachments as it becomes part of the conversation, with how much of its
/// text the user typed.
pub fn record(
    tx: &Transaction<'_>,
    user_event: EventId,
    typed: &str,
    attachments: &[AgentMessageAttachment],
    at: UtcTimestamp,
) -> StoreResult<()> {
    if attachments.is_empty() {
        return Ok(());
    }
    facts::append(
        tx,
        EventId::new(),
        &user_event.to_string(),
        KIND,
        &Attached { typed_bytes: typed.len() as u64, attachments: attachments.to_vec() },
        at,
    )
}

/// A message's attachments and how many bytes of its text the user typed; none for a message
/// without attachments.
pub fn of_message(
    db: &Connection,
    user_event: EventId,
) -> StoreResult<Option<(u64, Vec<AgentMessageAttachment>)>> {
    Ok(facts::latest::<Attached>(db, &user_event.to_string(), KIND)?
        .map(|attached| (attached.typed_bytes, attached.attachments)))
}

/// Lets go of the pictures and text files of a queued message the user removed.
pub fn release(
    tx: &Transaction<'_>,
    session: SessionId,
    attachments: &[AgentMessageAttachment],
) -> StoreResult<()> {
    let owner = owner(session);
    for attachment in attachments {
        let artifact_id = match attachment {
            AgentMessageAttachment::Image { artifact_id, .. }
            | AgentMessageAttachment::Text { artifact_id, .. } => Some(*artifact_id),
            AgentMessageAttachment::File { listing_artifact_id, .. }
            | AgentMessageAttachment::ExternalFile { listing_artifact_id, .. } => {
                *listing_artifact_id
            }
        };
        if let Some(artifact_id) = artifact_id {
            tx.execute(
                "DELETE FROM artifact_access_links WHERE artifact_id=?1 AND subject=?2",
                params![artifact_id.to_string(), session.to_string()],
            )?;
            artifact::release_owner(tx, artifact_id, &owner)?;
        }
    }
    artifact::collect(tx)?;
    Ok(())
}

/// What a request carries of something the user sent with a message of this conversation: its
/// SHA-256 digest and length. None for any other artifact.
pub fn sent_by_user(
    db: &Connection,
    session: SessionId,
    artifact_id: ArtifactId,
) -> StoreResult<Option<(ContentHash, u64)>> {
    let kept: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM artifact_owners WHERE artifact_id=?1 AND owner=?2)",
        params![artifact_id.to_string(), owner(session)],
        |row| row.get(0),
    )?;
    if !kept {
        return Ok(None);
    }
    let bytes = artifact::read(db, artifact_id, &session.to_string())?;
    Ok(Some((ContentHash::sha256(&bytes), bytes.len() as u64)))
}

/// A text file or a zip's listing one of the conversation's messages carries, read back for
/// its 文件 tab. Only text the conversation itself may read, and only text.
pub fn text(db: &Connection, session: SessionId, artifact_id: ArtifactId) -> StoreResult<String> {
    let bytes = artifact::read(db, artifact_id, &session.to_string())?;
    let media_type: String = db.query_row(
        "SELECT media_type FROM artifacts WHERE id=?1",
        [artifact_id.to_string()],
        |row| row.get(0),
    )?;
    if media_type != TEXT_MEDIA_TYPE || bytes.len() > aura_agent_domain::MAX_ATTACHMENT_TEXT_BYTES {
        return Err(Error::Invalid("attachment text"));
    }
    String::from_utf8(bytes).map_err(|_| Error::Invalid("attachment text"))
}
