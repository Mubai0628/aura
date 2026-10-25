use aura_domain_common::WorkspacePath;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditorApprovalFollowUp {
    pub path: WorkspacePath,
    pub requires_reapproval: bool,
    pub message: String,
}
