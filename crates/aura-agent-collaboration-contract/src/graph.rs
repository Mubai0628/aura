use crate::{AgentProfile, PeerDeliveryState};
use aura_agent_team_contract::AgentTeamView;
use aura_domain_common::{AgentCollaborationGraphId, SessionId, WorkspaceId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentGraphRequest {
    pub session_id: SessionId,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentGraphRevision {
    /// None is the existing conversation's as-yet-unpersisted singleton canvas.
    pub graph_id: Option<AgentCollaborationGraphId>,
    pub revision: u64,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(deny_unknown_fields)]
pub struct AgentGraphEdge {
    pub source_session_id: SessionId,
    pub target_session_id: SessionId,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentGraphPosition {
    pub session_id: SessionId,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentGraph {
    pub graph_id: Option<AgentCollaborationGraphId>,
    pub workspace_id: WorkspaceId,
    pub revision: u64,
    pub sessions: Vec<SessionId>,
    pub edges: Vec<AgentGraphEdge>,
    pub positions: Vec<AgentGraphPosition>,
}
impl AgentGraph {
    pub fn expected(&self) -> AgentGraphRevision {
        AgentGraphRevision { graph_id: self.graph_id, revision: self.revision }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentGraphAction {
    Merge {
        other_session_id: SessionId,
        expected_other: AgentGraphRevision,
        confirmed: bool,
        #[serde(default)]
        positions: Vec<AgentGraphPosition>,
    },
    Remove {
        session_id: SessionId,
    },
    Connect {
        source_session_id: SessionId,
        target_session_id: SessionId,
    },
    Disconnect {
        source_session_id: SessionId,
        target_session_id: SessionId,
    },
    Place {
        positions: Vec<AgentGraphPosition>,
    },
    ResetLayout,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentGraphMutationRequest {
    pub session_id: SessionId,
    pub expected_graph: AgentGraphRevision,
    pub action: AgentGraphAction,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentGraphTeam {
    pub parent_session_id: SessionId,
    pub team: AgentTeamView,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentGraphView {
    pub graph: AgentGraph,
    pub profiles: Vec<AgentProfile>,
    /// Handles are parent-local; the parent binding is explicit and independent of list order.
    pub teams: Vec<AgentGraphTeam>,
    pub candidates: Vec<AgentProfile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub peer_activity: Vec<PeerEdgeActivity>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PeerEdgeActivity {
    // A running question can use the direction opposite the saved connection.
    pub edge: AgentGraphEdge,
    pub state: PeerDeliveryState,
    pub paused_by_global_switch: bool,
}
