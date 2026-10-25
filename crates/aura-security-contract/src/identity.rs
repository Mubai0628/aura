use aura_domain_common::{
    AgentRunId, CorrelationId, PermissionSnapshotId, PrincipalId, SessionId, TurnTaskId,
    WorkspaceId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationActor {
    User,
    ParentAgent,
    ChildAgent,
    SystemRuntime,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationIdentity {
    pub principal_id: PrincipalId,
    pub actor: InvocationActor,
    pub workspace_id: Option<WorkspaceId>,
    pub session_id: Option<SessionId>,
    pub turn_task_id: Option<TurnTaskId>,
    pub agent_run_id: Option<AgentRunId>,
    pub parent_agent_run_id: Option<AgentRunId>,
    pub correlation_id: CorrelationId,
    pub permission_snapshot_id: Option<PermissionSnapshotId>,
}

impl InvocationIdentity {
    #[must_use]
    pub fn runtime_user(
        workspace_id: WorkspaceId,
        session_id: SessionId,
        correlation_id: CorrelationId,
    ) -> Self {
        Self {
            principal_id: PrincipalId::new(),
            actor: InvocationActor::User,
            workspace_id: Some(workspace_id),
            session_id: Some(session_id),
            turn_task_id: None,
            agent_run_id: None,
            parent_agent_run_id: None,
            correlation_id,
            permission_snapshot_id: None,
        }
    }

    #[must_use]
    pub const fn requires_model_purpose(&self) -> bool {
        matches!(self.actor, InvocationActor::ParentAgent | InvocationActor::ChildAgent)
    }
}
