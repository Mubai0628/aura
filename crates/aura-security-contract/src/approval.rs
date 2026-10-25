use crate::PermissionGrantLifetime;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    Pending,
    Approved,
    Denied,
    Expired,
    Cancelled,
    Invalidated,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecisionKind {
    AllowOnce,
    AllowSession,
    AllowWorkspace,
    AllowForCurrentTurn,
    Deny,
}

impl ApprovalDecisionKind {
    #[must_use]
    pub const fn grant_lifetime(self) -> Option<PermissionGrantLifetime> {
        match self {
            Self::AllowOnce => Some(PermissionGrantLifetime::Once),
            Self::AllowSession => Some(PermissionGrantLifetime::Session),
            Self::AllowWorkspace => Some(PermissionGrantLifetime::Workspace),
            Self::AllowForCurrentTurn => Some(PermissionGrantLifetime::TurnTask),
            Self::Deny => None,
        }
    }
}
