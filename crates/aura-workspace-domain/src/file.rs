use aura_domain_common::{
    ContentHash, ExternalGrantId, FileReferenceId, Revision, SessionId, WorkspacePath,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "target_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FileTargetRef {
    Workspace { path: WorkspacePath },
    SessionFile { file_reference_id: FileReferenceId },
    ExternalPath { path: String },
    ExternalGrant { external_grant_id: ExternalGrantId, relative_path: WorkspacePath },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceFileTarget {
    pub path: WorkspacePath,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogEntryKind {
    File,
    Directory,
    ReparsePoint,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStorageSource {
    EditorBuffer,
    Disk,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextEncoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Windows1252,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LineEnding {
    Lf,
    CrLf,
    Cr,
    Mixed,
    None,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "range_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TextReadRange {
    Full,
    Lines { start_line_index: u64, count: u64 },
    Characters { start_character_offset: u64, count: u64 },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LargeFileReadMode {
    AllowFull,
    RequireRange,
    SmallRangeOnly,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryDisplayFormat {
    Hex,
    Base64,
    HexWithAscii,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogicalFileSnapshot {
    pub path: WorkspacePath,
    pub source: FileStorageSource,
    pub logical_hash: ContentHash,
    pub disk_hash: Option<ContentHash>,
    pub buffer_revision: Option<Revision>,
    pub is_dirty: bool,
    pub active_editor_session: Option<SessionId>,
    pub bytes: Vec<u8>,
    /// Encoding the producing layer already established for `bytes` (an editor
    /// buffer's decoded encoding, or the byte order mark the disk read
    /// detected). Consumers reuse it instead of re-detecting or guessing.
    /// `None` means no reliable declaration is available.
    pub encoding: Option<TextEncoding>,
}

/// One bounded-memory text read of a large file on disk.
///
/// The producer streams the file once: `logical_hash` is the exact whole-file
/// hash, `total_characters`/`total_lines` are exact whole-file counts, and only
/// the bytes needed to reproduce the requested `TextReadRange` are retained.
/// Unlike `LogicalFileSnapshot` this never carries the whole file.
/// `peak_retained_bytes` is the high-water mark of retained text bytes and is the
/// evidence that the whole file was not materialized.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogicalTextReadSnapshot {
    pub path: WorkspacePath,
    pub source: FileStorageSource,
    pub logical_hash: ContentHash,
    pub disk_hash: Option<ContentHash>,
    pub active_editor_session: Option<SessionId>,
    /// Encoding the read established: the caller's `explicit_encoding`, else the
    /// byte order mark, else the UTF-8 fallback.
    pub encoding: TextEncoding,
    pub line_ending: LineEnding,
    pub total_characters: u64,
    pub total_lines: u64,
    /// Text retained for the requested range. `Full` keeps the whole decoded
    /// text, `Lines` keeps a prefix of whole lines followed by the requested
    /// lines, and `Characters` starts exactly at the requested offset; in every
    /// case the caller selects the range with the same helper the whole-file path
    /// uses. It is not the whole file unless the range asked for all of it.
    pub retained_text: String,
    /// Maximum number of bytes `retained_text` held at any point of the read.
    pub peak_retained_bytes: usize,
}
