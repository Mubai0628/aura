use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitStatusInput {
    #[serde(default = "yes")]
    pub include_untracked: bool,
    #[serde(default)]
    pub include_branches: bool,
    #[serde(default)]
    pub offset: u32,
    #[serde(default = "page")]
    #[schemars(range(min = 1, max = 2000))]
    pub limit: u32,
}
impl Default for GitStatusInput {
    fn default() -> Self {
        Self { include_untracked: true, include_branches: false, offset: 0, limit: 200 }
    }
}
fn yes() -> bool {
    true
}
fn page() -> u32 {
    200
}
fn patch_page() -> u32 {
    16384
}
fn log_page() -> u32 {
    20
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GitDiffTargetV1 {
    Unstaged {},
    Staged {},
    Commit { commit: String },
    Range { from: String, to: String },
}
impl Default for GitDiffTargetV1 {
    fn default() -> Self {
        Self::Unstaged {}
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitDiffInput {
    #[serde(default)]
    pub target: GitDiffTargetV1,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub offset_bytes: u64,
    #[serde(default = "patch_page")]
    #[schemars(range(min = 1, max = 65536))]
    pub max_bytes: u32,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitLogInput {
    #[serde(default = "log_page")]
    #[schemars(range(min = 1, max = 200))]
    pub limit: u32,
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub commit: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "kind", content = "input", rename_all = "snake_case", deny_unknown_fields)]
pub enum GitReadInputV1 {
    Status(GitStatusInput),
    Diff(GitDiffInput),
    Log(GitLogInput),
    /// Desktop review only. The model's git.diff schema remains GitDiffInput.
    UntrackedDiff(GitUntrackedDiffInputV1),
    /// Desktop review only: the working tree against HEAD or against where HEAD left a
    /// branch. The model's git.diff schema remains GitDiffInput.
    ReviewDiff(GitReviewDiffInputV1),
}

/// The review page's comparison. Untracked files are not part of it: status lists them and
/// `UntrackedDiff` reads them one by one.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitReviewDiffInputV1 {
    /// None: the uncommitted changes, staged or not, against HEAD (the empty tree before the
    /// first commit). A branch: the working tree against the commit where HEAD left it.
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    /// Lines that differ only in whitespace are not changes; files with nothing else drop out.
    #[serde(default)]
    pub ignore_whitespace: bool,
    /// Only the files and their line counts, with an empty patch. The review page lists
    /// the files this way and reads each file's lines when it is shown.
    #[serde(default)]
    pub stats_only: bool,
    #[serde(default)]
    pub offset_bytes: u64,
    #[serde(default = "patch_page")]
    #[schemars(range(min = 1, max = 65536))]
    pub max_bytes: u32,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitUntrackedDiffInputV1 {
    pub path: String,
    #[serde(default)]
    pub offset_bytes: u64,
    #[serde(default = "patch_page")]
    #[schemars(range(min = 1, max = 65536))]
    pub max_bytes: u32,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitWorkspaceReadRequestV1 {
    pub workspace_id: aura_domain_common::WorkspaceId,
    pub session_id: aura_domain_common::SessionId,
    pub query: GitReadInputV1,
}
