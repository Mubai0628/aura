//! Local Native Git mutations. External programs keep the same authority as terminal Git.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const GIT_NATIVE_EXECUTION_NOTICE: &str = "与在终端运行 git 相同，Aura 不隔离它的文件写入";

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitCommitInput {
    /// Required non-empty commit message. No amend, editor or hook bypass option.
    #[schemars(length(min = 1, max = 65536))]
    pub message: String,
    /// Literal workspace-relative files to stage and commit; excludes unrelated staged files.
    #[serde(default)]
    #[schemars(length(max = 200))]
    pub paths: Vec<String>,
    /// Commit the existing index only. Mutually exclusive with non-empty paths.
    #[serde(default)]
    pub staged_only: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[schemars(extend("type" = "object"))]
pub enum GitBranchInput {
    Create {
        #[schemars(length(min = 1, max = 1024))]
        name: String,
        #[serde(default)]
        start: Option<String>,
        #[serde(default)]
        switch: bool,
    },
    Switch {
        #[schemars(length(min = 1, max = 1024))]
        name: String,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitCommitResultV1 {
    pub commit: super::GitCommitV1,
    pub branch: Option<String>,
    /// Bounded post-operation status, including remaining untracked and staged changes.
    pub status: super::GitStatusResultV1,
    pub execution_notice: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitBranchResultV1 {
    pub previous_branch: Option<String>,
    pub previous_head: Option<String>,
    pub branch: String,
    pub head: Option<String>,
    pub created: bool,
    pub switched: bool,
    pub status: super::GitStatusResultV1,
    pub execution_notice: String,
}
