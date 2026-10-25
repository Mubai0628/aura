//! Pictures and files added to a
//! message in the composer. A picture goes to the model as an image with its message; a text
//! file's contents go with the message's text; a file of the workspace is named by its path, for
//! the model to read with its tools. The conversation shows what the user typed, with a card for
//! each picture and file.
use aura_domain_common::{ArtifactId, ExternalGrantId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How many pictures and files one message carries at most.
pub const MAX_MESSAGE_ATTACHMENTS: usize = 10;
/// The largest picture, as the composer sends it (it scales larger ones down).
pub const MAX_ATTACHED_IMAGE_BYTES: u64 = 3 * 1024 * 1024;
/// The largest text file sent with its contents.
pub const MAX_ATTACHED_TEXT_BYTES: u64 = 256 * 1024;
/// What one message's pictures and text files come to at most, so that the message stays within
/// what one desktop command carries.
pub const MAX_ATTACHED_BYTES: u64 = 4 * 1024 * 1024;
/// The picture types a message carries; the composer turns other pictures into one of them.
pub const ATTACHED_IMAGE_MEDIA_TYPES: &[&str] = &["image/png", "image/jpeg"];

/// One picture or file the composer sends with a message.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentAttachmentDraft {
    /// A PNG or JPEG picture; the model sees it as an image.
    Image { name: String, media_type: String, data_base64: String },
    /// A text file; its contents go to the model with the message.
    Text { name: String, text: String },
    /// A file selected on this computer. Core classifies workspace files and authorizes an
    /// external file for read access to that one file identity in this conversation.
    File { path: String },
}

/// One picture or file a message carries, as Aura keeps it with the message.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentMessageAttachment {
    /// Read for the conversation with `browser.screenshot.read`.
    Image {
        name: String,
        media_type: String,
        byte_length: u64,
        artifact_id: ArtifactId,
    },
    Text {
        name: String,
        byte_length: u64,
        artifact_id: ArtifactId,
    },
    /// `path` is the file's place in the workspace, its parts joined by `/`.
    File {
        name: String,
        path: String,
        byte_length: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        listing_artifact_id: Option<ArtifactId>,
    },
    /// An external file stays at its absolute location; its authorization is bound to the
    /// selected file identity and this conversation, with read access only.
    ExternalFile {
        name: String,
        path: String,
        byte_length: u64,
        external_grant_id: ExternalGrantId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        listing_artifact_id: Option<ArtifactId>,
    },
}

impl AgentMessageAttachment {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Image { name, .. }
            | Self::Text { name, .. }
            | Self::File { name, .. }
            | Self::ExternalFile { name, .. } => name,
        }
    }
}

/// Why the composer's pictures and files were not taken; the text is what the user reads.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AttachmentError {
    #[error("agent.attachments_too_many: 一条消息最多附加 10 个图片或文件。")]
    TooMany,
    #[error("agent.attachment_name_invalid: 附件的文件名不可用。")]
    NameInvalid,
    #[error("agent.attachment_image_invalid: 「{0}」不是可发送的 PNG 或 JPEG 图片。")]
    ImageInvalid(String),
    #[error("agent.attachment_image_too_large: 图片「{0}」太大（最多 3 MB）。")]
    ImageTooLarge(String),
    #[error(
        "agent.attachment_text_too_large: 文本文件「{0}」太大，超过 256 KB 请改为按文件位置附加。"
    )]
    TextTooLarge(String),
    #[error("agent.attachment_path_invalid: 附件的位置不可用。")]
    PathInvalid,
    #[error(
        "agent.attachments_too_large: 这条消息的图片和文本文件合计太大（最多 4 MB），请分几条发送。"
    )]
    TooLarge,
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.chars().count() <= 255
        && !name.chars().any(|c| c.is_control() || c == '/' || c == '\\')
}

/// The bytes a standard base64 text decodes to, read from its length.
fn decoded_length(data: &str) -> u64 {
    let padding = data.bytes().rev().take_while(|byte| *byte == b'=').count().min(2);
    ((data.len() / 4) * 3).saturating_sub(padding) as u64
}

/// Checks what the composer sent before anything is read or kept: how many, their names, kinds
/// and sizes. Core then decodes the pictures and looks for the workspace files.
pub fn validate_attachment_drafts(drafts: &[AgentAttachmentDraft]) -> Result<(), AttachmentError> {
    if drafts.len() > MAX_MESSAGE_ATTACHMENTS {
        return Err(AttachmentError::TooMany);
    }
    let mut total = 0_u64;
    for draft in drafts {
        match draft {
            AgentAttachmentDraft::Image { name, media_type, data_base64 } => {
                if !valid_name(name) {
                    return Err(AttachmentError::NameInvalid);
                }
                if !ATTACHED_IMAGE_MEDIA_TYPES.contains(&media_type.as_str())
                    || data_base64.is_empty()
                    || data_base64.len() % 4 != 0
                {
                    return Err(AttachmentError::ImageInvalid(name.clone()));
                }
                let bytes = decoded_length(data_base64);
                if bytes > MAX_ATTACHED_IMAGE_BYTES {
                    return Err(AttachmentError::ImageTooLarge(name.clone()));
                }
                total += bytes;
            }
            AgentAttachmentDraft::Text { name, text } => {
                if !valid_name(name) {
                    return Err(AttachmentError::NameInvalid);
                }
                if text.len() as u64 > MAX_ATTACHED_TEXT_BYTES {
                    return Err(AttachmentError::TextTooLarge(name.clone()));
                }
                total += text.len() as u64;
            }
            AgentAttachmentDraft::File { path } => {
                if path.trim().is_empty() || path.len() > 32 * 1024 || path.contains('\0') {
                    return Err(AttachmentError::PathInvalid);
                }
            }
        }
    }
    if total > MAX_ATTACHED_BYTES {
        return Err(AttachmentError::TooLarge);
    }
    Ok(())
}

impl AgentAttachmentDraft {
    /// The draft as a repeated command is compared and kept: a picture's or text's digest in place
    /// of its contents, which Aura keeps once, with the message.
    #[must_use]
    pub fn receipt(&self) -> Self {
        let digest = |bytes: &[u8]| aura_domain_common::ContentHash::sha256(bytes).to_string();
        match self {
            Self::Image { name, media_type, data_base64 } => Self::Image {
                name: name.clone(),
                media_type: media_type.clone(),
                data_base64: digest(data_base64.as_bytes()),
            },
            Self::Text { name, text } => {
                Self::Text { name: name.clone(), text: digest(text.as_bytes()) }
            }
            Self::File { path } => Self::File { path: path.clone() },
        }
    }
}

/// One attachment as the model reads about it in the message.
#[derive(Clone, Copy, Debug)]
pub enum AttachedPart<'a> {
    Image {
        name: &'a str,
        media_type: &'a str,
        byte_length: u64,
    },
    Text {
        name: &'a str,
        text: &'a str,
    },
    File {
        path: &'a str,
        byte_length: u64,
        listing: Option<&'a str>,
    },
    ExternalFile {
        path: &'a str,
        byte_length: u64,
        external_grant_id: ExternalGrantId,
        listing: Option<&'a str>,
    },
}

fn size(bytes: u64) -> String {
    const KB: u64 = 1024;
    if bytes < KB {
        format!("{bytes} B")
    } else if bytes < KB * KB {
        format!("{} KB", bytes.div_ceil(KB))
    } else {
        let tenths = (bytes * 10 + KB * KB / 2) / (KB * KB);
        format!("{}.{} MB", tenths / 10, tenths % 10)
    }
}

/// A fence longer than any run of backticks in the text.
fn fence(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    "`".repeat(longest.max(2) + 1)
}

/// Files remain at their location. The model chooses whether metadata or selected visual frames
/// help the task; a file attachment alone does not give it an audio input or a transcript.
fn media_guidance(path: &str) -> Option<&'static str> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    match extension.as_str() {
        "mp4" | "m4v" | "webm" | "ogv" | "mov" | "mkv" | "avi" | "wmv" | "flv" => Some(
            "; video: use media.probe for metadata or media.frames for selected times, uniform frames, or a contact sheet; choose what the task needs, frames are not extracted automatically and do not transcribe audio",
        ),
        "mp3" | "wav" | "flac" | "ogg" | "oga" | "m4a" | "aac" | "opus" | "wma" => Some(
            "; audio: attached by location only, not transcribed; you cannot listen to it through this attachment",
        ),
        _ => None,
    }
}

/// The message as the model reads it: what the user typed, then a line for each picture and
/// file, then each text file's contents. The pictures themselves follow the text, in the same
/// order. With no attachments it is what the user typed.
#[must_use]
pub fn message_with_attachments(typed: &str, parts: &[AttachedPart<'_>]) -> String {
    if parts.is_empty() {
        return typed.to_owned();
    }
    let mut text = String::new();
    if !typed.trim().is_empty() {
        text.push_str(typed);
        text.push_str("\n\n");
    }
    text.push_str("Attached to this message:");
    for part in parts {
        text.push_str("\n- ");
        match part {
            AttachedPart::Image { name, media_type, byte_length } => {
                text.push_str(&format!(
                    "picture `{name}` ({media_type}, {}), included right after this text as an image you can see",
                    size(*byte_length)
                ));
            }
            AttachedPart::Text { name, text: contents } => {
                let bytes = contents.len() as u64;
                text.push_str(&format!("text file `{name}` ({}), its contents below", size(bytes)));
            }
            AttachedPart::File { path, byte_length, .. } => text.push_str(&format!(
                "workspace file `{path}` ({}), not included: read it from the workspace if the \
                 task needs it",
                size(*byte_length)
            )),
            AttachedPart::ExternalFile { path, byte_length, external_grant_id, .. } => {
                text.push_str(&format!(
                    "external file `{path}` ({}), not included: read this one file with external grant `{external_grant_id}` if the task needs it; read only, for this conversation",
                    size(*byte_length)
                ));
            }
        }
        if let AttachedPart::File { path, .. } | AttachedPart::ExternalFile { path, .. } = part
            && let Some(guidance) = media_guidance(path)
        {
            text.push_str(guidance);
        }
    }
    for part in parts {
        if let AttachedPart::File { path, listing: Some(listing), .. }
        | AttachedPart::ExternalFile { path, listing: Some(listing), .. } = part
        {
            let fence = fence(listing);
            text.push_str(&format!(
                "\n\n`{path}` ZIP 清单（用户附件内容）:\n{fence}\n{listing}\n{fence}"
            ));
        }
        if let AttachedPart::Text { name, text: contents } = part {
            let fence = fence(contents);
            text.push_str(&format!("\n\n`{name}`:\n{fence}\n{contents}"));
            if !contents.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&fence);
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l13_3_video_attachments_discover_media_tools_and_audio_is_honest_about_not_listening() {
        let grant = ExternalGrantId::new();
        for part in [
            AttachedPart::File { path: "clips/VIDEO.MP4", byte_length: 100, listing: None },
            AttachedPart::ExternalFile {
                path: "C:/Downloads/clip.webm",
                byte_length: 100,
                external_grant_id: grant,
                listing: None,
            },
        ] {
            let text = message_with_attachments("看一下", &[part]);
            assert!(text.contains("media.probe") && text.contains("media.frames"), "{text}");
            assert!(text.contains("choose") && text.contains("automatically"), "{text}");
        }
        for part in [
            AttachedPart::File { path: "audio.M4A", byte_length: 100, listing: None },
            AttachedPart::ExternalFile {
                path: "C:/Downloads/audio.wav",
                byte_length: 100,
                external_grant_id: grant,
                listing: None,
            },
        ] {
            let audio = message_with_attachments("听一下", &[part]);
            assert!(
                audio.contains("cannot listen") && audio.contains("not transcribed"),
                "{audio}"
            );
        }
        let document = message_with_attachments(
            "读一下",
            &[AttachedPart::File { path: "notes.pdf", byte_length: 100, listing: None }],
        );
        assert!(!document.contains("media.frames") && !document.contains("cannot listen"));
    }

    fn image(name: &str, data: &str) -> AgentAttachmentDraft {
        AgentAttachmentDraft::Image {
            name: name.into(),
            media_type: "image/png".into(),
            data_base64: data.into(),
        }
    }

    #[test]
    fn drafts_are_checked_for_count_names_kinds_and_sizes() {
        assert_eq!(validate_attachment_drafts(&[image("a.png", "iVBORw==")]), Ok(()));
        let many = vec![image("a.png", "iVBORw=="); 11];
        assert_eq!(validate_attachment_drafts(&many), Err(AttachmentError::TooMany));
        assert_eq!(
            validate_attachment_drafts(&[image("..\\a.png", "iVBORw==")]),
            Err(AttachmentError::NameInvalid)
        );
        let gif = AgentAttachmentDraft::Image {
            name: "a.gif".into(),
            media_type: "image/gif".into(),
            data_base64: "R0lG".into(),
        };
        assert!(matches!(
            validate_attachment_drafts(&[gif]),
            Err(AttachmentError::ImageInvalid(_))
        ));
        let big = "A".repeat(4 * 1024 * 1024 + 4);
        assert!(matches!(
            validate_attachment_drafts(&[image("big.png", &big)]),
            Err(AttachmentError::ImageTooLarge(_))
        ));
        let text =
            AgentAttachmentDraft::Text { name: "a.txt".into(), text: "x".repeat(256 * 1024 + 1) };
        assert!(matches!(
            validate_attachment_drafts(&[text]),
            Err(AttachmentError::TextTooLarge(_))
        ));
        // 1,500,000 bytes each: two fit in the 4 MB a message carries, three do not.
        let part = "A".repeat(2_000_000);
        let two = [image("a.png", &part), image("b.png", &part)];
        assert_eq!(validate_attachment_drafts(&two), Ok(()));
        let three = [image("a.png", &part), image("b.png", &part), image("c.png", &part)];
        assert_eq!(validate_attachment_drafts(&three), Err(AttachmentError::TooLarge));
        assert_eq!(
            validate_attachment_drafts(&[AgentAttachmentDraft::File { path: " ".into() }]),
            Err(AttachmentError::PathInvalid)
        );
    }

    #[test]
    fn a_receipt_keeps_the_shape_with_digests_for_contents() {
        let draft = image("a.png", "iVBORw==");
        let AgentAttachmentDraft::Image { data_base64, name, .. } = draft.receipt() else {
            panic!("an image stays an image");
        };
        assert_eq!(name, "a.png");
        assert!(data_base64.starts_with("sha256:"), "{data_base64}");
        assert_eq!(draft.receipt(), draft.receipt());
    }

    #[test]
    fn the_model_reads_the_typed_text_then_a_line_per_attachment_then_the_text_files() {
        assert_eq!(message_with_attachments("看看", &[]), "看看");
        let text = message_with_attachments(
            "看看这些",
            &[
                AttachedPart::Image {
                    name: "shot.png",
                    media_type: "image/png",
                    byte_length: 186_000,
                },
                AttachedPart::Text { name: "notes.md", text: "a ``` b" },
                AttachedPart::File { path: "docs/spec.pdf", byte_length: 2_411_724, listing: None },
            ],
        );
        assert_eq!(
            text,
            "看看这些\n\nAttached to this message:\n\
             - picture `shot.png` (image/png, 182 KB), included right after this text as an image you can see\n\
             - text file `notes.md` (7 B), its contents below\n\
             - workspace file `docs/spec.pdf` (2.3 MB), not included: read it from the workspace \
             if the task needs it\n\n\
             `notes.md`:\n````\na ``` b\n````"
        );
        let alone = message_with_attachments(
            " ",
            &[AttachedPart::File { path: "a.zip", byte_length: 10, listing: None }],
        );
        assert!(alone.starts_with("Attached to this message:"), "{alone}");
    }
}
