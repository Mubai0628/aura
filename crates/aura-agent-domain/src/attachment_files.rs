//! What a
//! message carried, read again for the conversation's 文件 tab. A text file or a zip's listing is
//! read back from where the message keeps it; an attached file outside the workspace is found
//! again through its read-only authorization, for Main only.
use crate::AgentMessageTextSource;
use aura_domain_common::{ArtifactId, ExternalGrantId, SessionId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The longest text read back for the 文件 tab: a message's text files are at most 256 KB.
pub const MAX_ATTACHMENT_TEXT_BYTES: usize = 1024 * 1024;

/// A text file or a zip's listing one of the conversation's messages carries.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AttachmentTextRequest {
    pub session_id: SessionId,
    pub artifact_id: ArtifactId,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AttachmentText {
    pub text: String,
}

/// An attached file outside the workspace, by the authorization its message gave this
/// conversation. Main asks; the renderer never learns more than the message already shows.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AttachmentLocateRequest {
    pub session_id: SessionId,
    pub external_grant_id: ExternalGrantId,
}

/// One entry of an
/// attached zip outside the workspace, by its path in the zip's listing. Core copies it into Aura's
/// own folder for the conversation and says where, for Main only. Or of a zip in the session's workspace, by its path there — one of the two.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AttachmentEntryRequest {
    pub session_id: SessionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_grant_id: Option<ExternalGrantId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<String>,
    pub entry: String,
}

/// A file outside the workspace that a message of this conversation names by its full path.
/// Core reads that message and finds the path in it before saying where the file is, so only a
/// path the conversation shows can be opened; Aura's own data and the user's credential stores are
/// not. For Main only.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MentionedFileRequest {
    pub session_id: SessionId,
    pub path: String,
    pub mentioned_in: AgentMessageTextSource,
}

/// Where the file is now, after Core opened it and found it is still the file that was attached.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AttachmentLocation {
    pub path: String,
    pub byte_length: u64,
}
