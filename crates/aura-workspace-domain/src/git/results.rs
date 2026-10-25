use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitRuntimeStatusV1 {
    pub state: GitRuntimeStateV1,
    pub executable: Option<String>,
    pub version: Option<String>,
    pub message: String,
    pub next_step: String,
    pub installation_url: String,
}
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitRuntimeStateV1 {
    Available,
    Missing,
    TooOld,
    Invalid,
}
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitInstallationResultV1 {
    pub opened: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitChangeV1 {
    /// Present only when an attribute selects an external converter suppressed by isolated reads.
    pub comparison_note: Option<String>,
    pub path: String,
    pub original_path: Option<String>,
    pub index_status: String,
    pub worktree_status: String,
    pub conflict: bool,
    pub untracked: bool,
    pub submodule: bool,
}
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitBranchV1 {
    pub name: String,
    pub commit: String,
    pub remote: bool,
}
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitStatusResultV1 {
    pub branch: Option<String>,
    pub head: Option<String>,
    pub detached: bool,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub operation: Option<String>,
    pub conflicts: Vec<String>,
    pub entries: Vec<GitChangeV1>,
    pub branches: Vec<GitBranchV1>,
    pub total_entries: u32,
    pub next_offset: Option<u32>,
    pub truncated: bool,
    /// Submodule contents are deliberately not traversed or executed.
    pub submodule_contents_checked: bool,
}
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitFileStatV1 {
    pub comparison_note: Option<String>,
    pub path: String,
    pub original_path: Option<String>,
    pub added: Option<u64>,
    pub deleted: Option<u64>,
    pub binary: bool,
}
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitDiffResultV1 {
    pub warnings: Vec<String>,
    pub files: Vec<GitFileStatV1>,
    pub patch: String,
    pub offset_bytes: u64,
    pub next_offset: Option<u64>,
    pub total_bytes: u64,
    pub truncated: bool,
}
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitCommitV1 {
    pub commit: String,
    pub parents: Vec<String>,
    pub author: String,
    pub authored_at: String,
    pub subject: String,
    pub body: String,
    pub files: Vec<GitFileStatV1>,
}
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitLogResultV1 {
    pub commits: Vec<GitCommitV1>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "kind", content = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum GitReadResultV1 {
    Status(GitStatusResultV1),
    Diff(GitDiffResultV1),
    Log(GitLogResultV1),
}
