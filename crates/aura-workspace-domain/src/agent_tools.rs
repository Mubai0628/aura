//! Model-facing file inputs. Core supplies the authenticated workspace/session.
//!
//! Every path field uses [`AgentWorkspacePath`], so a model may write either a
//! workspace-relative path or an absolute path inside the session's workspace
//! and mean the same file. Core normalizes the spelling against the authorized
//! workspace root before the operation is built; this boundary only ever sees
//! the validated relative value.
use crate::{
    AgentWorkspacePath, FsExpectedState, FsExternalListTarget, FsSearchPattern, TextEncoding,
    TextReadRange,
};
use aura_domain_common::{ContentHash, ExternalGrantId, Revision, WorkspacePath};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The default and largest page of the listing, search and find tools.
const DEFAULT_PAGE: u32 = 100;
fn default_page() -> u32 {
    DEFAULT_PAGE
}
fn whole_file() -> TextReadRange {
    TextReadRange::Full
}
fn recursive() -> bool {
    true
}

/// What a model sees for a page cursor: a value it copies back, never one it builds.
///
/// The real cursor binds a page to its directory, query and scope revision, and Core still
/// deserializes and checks every field. Describing all of that to the model only made each
/// request longer (the find cursor alone was 7 KB of schema) without changing what it does.
struct OpaquePageCursor;

impl JsonSchema for OpaquePageCursor {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "OpaquePageCursor".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "object",
            "description": "Pass next_cursor from the previous page unchanged to continue. Leave it out to start."
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentFileReadInput {
    pub path: AgentWorkspacePath,
    /// Which part to read. Default: the whole file.
    #[serde(default = "whole_file")]
    pub range: TextReadRange,
    /// Leave out to detect the encoding.
    pub explicit_encoding: Option<TextEncoding>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentFileSearchInput {
    pub pattern: FsSearchPattern,
    /// Directory to search. Default: the workspace root.
    #[serde(default = "AgentWorkspacePath::root")]
    pub root: AgentWorkspacePath,
    pub path_glob: Option<String>,
    /// Default false.
    #[serde(default)]
    pub include_ignored: bool,
    /// Default false.
    #[serde(default)]
    pub include_generated: bool,
    /// Maximum matches returned, 1..=100 (default 100). This searches text, not filenames.
    #[schemars(range(min = 1, max = 100))]
    #[serde(default = "default_page")]
    pub limit: u32,
}

/// Lists immediate children, including empty directories. Use `.` for the root.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentFileListInput {
    pub path: AgentWorkspacePath,
    /// Entries per page, 1..=100 (default 100).
    #[schemars(range(min = 1, max = 100))]
    #[serde(default = "default_page")]
    pub limit: u32,
    #[schemars(with = "Option<OpaquePageCursor>")]
    pub cursor: Option<AgentFileListCursor>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentFileListCursor {
    pub directory: AgentWorkspacePath,
    /// The version of what was listed: the workspace index's directory revision, or
    /// the digest of an authorized external directory's live contents. A change to
    /// that directory invalidates the cursor instead of silently skipping entries.
    pub catalog_revision: Revision,
    pub after_path: AgentWorkspacePath,
    /// Present when this page listed an authorized external directory: the grant it
    /// was listed through. A cursor from one grant never continues another.
    #[serde(default)]
    pub external_grant_id: Option<ExternalGrantId>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentFileListResult {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped_scopes: Vec<crate::FsSkippedScope>,
    pub entries: Vec<crate::FsListEntry>,
    pub catalog_revision: Revision,
    pub next_cursor: Option<AgentFileListCursor>,
    pub truncated: bool,
    /// Present when this listing used an authorized external directory.
    #[serde(default)]
    pub external: Option<FsExternalListTarget>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentFileWriteInput {
    pub path: AgentWorkspacePath,
    pub text: String,
    /// Use {expected_kind:"absent"} for a new file. For an existing file, use the
    /// latest logical_hash object ({algorithm,hex}) or {expected_kind:"file_hash",hash}.
    #[serde(deserialize_with = "crate::agent_write::deserialize_expected")]
    #[schemars(with = "crate::agent_write::AgentFileWriteExpectation")]
    pub expected: FsExpectedState,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentFilePatchInput {
    pub path: AgentWorkspacePath,
    pub unified_diff: String,
    pub expected_hash: ContentHash,
}

/// Finds files by name under one directory. This is filename lookup, not a text
/// search; use `fs.search` to find text inside files.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentFileFindInput {
    /// The directory to search. Default: the workspace root.
    #[serde(default = "AgentWorkspacePath::root")]
    pub path: AgentWorkspacePath,
    /// One or more file names to look for. A single call may carry several.
    #[schemars(length(min = 1, max = 16))]
    pub file_names: Vec<String>,
    /// Descend into subdirectories (default true). Use false for direct children only.
    #[serde(default = "recursive")]
    pub recursive: bool,
    /// Default exact.
    #[serde(default)]
    pub match_mode: AgentFileFindMatchMode,
    /// Default false (case-insensitive). Set true only when the exact case matters.
    #[serde(default)]
    pub case_sensitive: bool,
    /// Matches per page, 1..=100 (default 100).
    #[schemars(range(min = 1, max = 100))]
    #[serde(default = "default_page")]
    pub limit: u32,
    #[schemars(with = "Option<OpaquePageCursor>")]
    pub cursor: Option<AgentFileFindCursor>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentFileFindMatchMode {
    /// The whole file name must equal a requested name.
    #[default]
    Exact,
    /// The whole file name must match a pattern: `*` is any run of characters, `?` one.
    Glob,
}

/// Binds one page to the directory, query, mode, scope and scope revision that
/// produced it, so a changed scope invalidates the cursor instead of silently
/// skipping or repeating a file.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentFileFindCursor {
    pub directory: AgentWorkspacePath,
    pub file_names: Vec<String>,
    /// Whether the query that produced this cursor searched the whole subtree.
    /// A page and its continuation have to be the same walk: continuing a shallow
    /// page as a deep one would skip exactly the descendants the first page could
    /// not see, and continuing a deep page as a shallow one would repeat paths.
    pub recursive: bool,
    pub match_mode: AgentFileFindMatchMode,
    pub case_sensitive: bool,
    /// The version of the queried scope: the directory's own children for a
    /// shallow query, and the directory plus its subtree for a recursive one. A
    /// change anywhere else in the workspace leaves it untouched, so unrelated
    /// work cannot invalidate a page in progress.
    pub scope_revision: Revision,
    /// The last path the previous page examined. The continuation resumes there,
    /// which is what keeps a page from re-walking what was already read.
    pub after_path: AgentWorkspacePath,
    /// Present when this cursor continues a lookup in an authorized external
    /// directory. A cursor from one grant never continues another, and an external
    /// cursor never continues a workspace query.
    #[serde(default)]
    pub external_grant_id: Option<ExternalGrantId>,
    /// Names already taken from [`Self::listing_directory`]. Older cursors omit it.
    /// Continuation skips these names rather than every path up to `after_path`, so a
    /// directory listing that is not in dictionary order cannot drop a smaller unread name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consumed_names: Vec<String>,
    /// The directory those names were read from. Absent means the query root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listing_directory: Option<AgentWorkspacePath>,
    /// Listed entries the previous page did not examine yet.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending: Vec<AgentFileFindPending>,
    /// Directories whose children have all been listed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub finished_directories: Vec<AgentWorkspacePath>,
    /// Directories whose listing stopped before every child name was read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub partial_listings: Vec<AgentFileFindPartialListing>,
    /// Identity of the external root this cursor was issued for. A replaced directory
    /// does not continue under the old cursor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_identity: Option<String>,
}

/// Names already read from one external directory whose listing has not finished.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentFileFindPartialListing {
    pub directory: AgentWorkspacePath,
    pub consumed_names: Vec<String>,
}

/// One listed external entry waiting to be examined on the next page.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentFileFindPending {
    pub relative_path: AgentWorkspacePath,
    pub name: String,
    pub kind: crate::CatalogEntryKind,
    pub byte_length: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentFileFindResult {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped_scopes: Vec<crate::FsSkippedScope>,
    pub matches: Vec<AgentFileFindMatch>,
    /// The version of the queried scope, not of the workspace. A caller that
    /// repeats the same query with the same scope version is asking the same
    /// question about the same facts.
    pub scope_revision: Revision,
    pub next_cursor: Option<AgentFileFindCursor>,
    pub truncated: bool,
    /// Index entries examined inside the scope for this page. It is bounded by the
    /// scan budget even when the page is empty, so a page never walks an unbounded
    /// index.
    pub scanned_entries: u64,
    /// True only when the walk reached the end of the scope and every match is in
    /// this page. A model may conclude that an absent name does not exist only when
    /// this is true; when it is false the page is a prefix of a larger result.
    pub complete: bool,
    /// Present when this lookup walked an authorized external directory instead of
    /// the workspace index.
    #[serde(default)]
    pub external: Option<FsExternalFindTarget>,
}

/// The authorized external directory one filename lookup walked.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FsExternalFindTarget {
    /// The session grant that authorized the target, when a grant did.
    #[serde(default)]
    pub grant_id: Option<ExternalGrantId>,
    /// The absolute directory path as the user sees it. Each match's `path` is
    /// relative to this grant's root.
    pub display_path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentFileFindMatch {
    pub path: WorkspacePath,
    pub name: String,
    pub kind: crate::CatalogEntryKind,
    pub byte_length: Option<u64>,
    pub ignored: bool,
    pub generated: bool,
}
