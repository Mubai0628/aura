use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::WorkspaceSummary;
use aura_domain_common::{SessionId, WorkspaceId};

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionSummary {
    pub session_id: SessionId,
    pub workspace_id: WorkspaceId,
    pub title: String,
    pub state: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSessionProjection {
    pub workspaces: Vec<WorkspaceSummary>,
    pub sessions: Vec<SessionSummary>,
}
/// Authenticated user command. Core allocates identity and uses the configured
/// default permission mode; a model tool cannot call this route.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SessionCreateRequest {
    pub workspace_id: aura_domain_common::WorkspaceId,
    pub title: String,
}

/// Rename an existing conversation without overwriting a concurrent rename.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRenameRequest {
    pub session_id: SessionId,
    pub expected_title: String,
    pub title: String,
}

/// Archive, restore or delete one conversation. An authenticated user command; a model
/// tool cannot call it. Archiving and deleting are refused while the conversation has a task
/// that has not finished.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionLifecycleRequest {
    pub session_id: SessionId,
}

/// A deleted conversation. Its conversation, process and audit records are gone; its
/// Workspace's files, `.memories` and global memory are untouched.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionDeleteResult {
    pub session_id: SessionId,
}
