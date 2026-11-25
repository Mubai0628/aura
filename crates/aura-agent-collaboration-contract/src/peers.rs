use aura_domain_common::{
    AgentCollaborationGraphId, AgentRunId, ContentHash, EventId, Revision, SessionId,
    TaskLineageId, ToolCallId, TurnTaskId, WorkspaceId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const DEFAULT_PEER_BUDGET: u32 = 32;
pub const MAX_PEER_TASK: usize = 8_000;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeersListInput {}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeerDirectory {
    pub graph_id: Option<AgentCollaborationGraphId>,
    pub graph_revision: u64,
    pub peers: Vec<PeerDirectoryEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeerDirectoryEntry {
    pub session_id: SessionId,
    pub name: String,
    pub description: String,
    pub profile_revision: u64,
    pub availability: PeerAvailability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability_reason: Option<String>,
    #[serde(default)]
    pub requires_confirmation: bool,
    /// A direct incoming edge allows a question but does not allow dispatch.
    pub can_dispatch: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PeerAvailability {
    Ready,
    Busy,
    Paused,
    Disabled,
    ModelUnavailable,
}

/// No actor, permissions, source references or idempotency key are model-supplied.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PeerDispatchInput {
    pub graph_id: AgentCollaborationGraphId,
    pub graph_revision: u64,
    #[schemars(length(min = 1, max = 32))]
    pub targets: Vec<SessionId>,
    #[schemars(length(min = 1, max = 8000), regex(pattern = r"\S"))]
    pub task: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PeerAskInput {
    pub graph_id: AgentCollaborationGraphId,
    pub graph_revision: u64,
    pub target: SessionId,
    #[schemars(length(min = 1, max = 8000), regex(pattern = r"\S"))]
    pub question: String,
}

impl From<PeerAskInput> for PeerDispatchInput {
    fn from(input: PeerAskInput) -> Self {
        Self {
            graph_id: input.graph_id,
            graph_revision: input.graph_revision,
            targets: vec![input.target],
            task: input.question,
        }
    }
}

pub const MAX_PEER_SOURCE_CHARS: u32 = 8_000;

/// Read one original user source already authorized for this delivery. The
/// delivery and event ID are the identity; no path, title or actor is accepted.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeerSourceInput {
    pub delivery_id: EventId,
    pub event_id: EventId,
    #[serde(default)]
    #[schemars(range(min = 0))]
    pub offset: u32,
    #[serde(default)]
    #[schemars(range(min = 1, max = 8000))]
    pub max_chars: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeerSourceContent {
    pub delivery_id: EventId,
    pub event_id: EventId,
    pub content_hash: ContentHash,
    pub offset: u32,
    pub total_chars: u32,
    pub text: String,
    pub next_offset: Option<u32>,
    pub complete: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PeerDeliveryKind {
    Dispatch,
    Question,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeerLookup {
    pub delivery_id: Option<EventId>,
    /// The original dispatch call, even if no delivery receipt reached the caller.
    pub tool_call_id: Option<ToolCallId>,
    /// Copy next_source_cursor from a lookup of this same delivery ID. Omit for the first page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_cursor: Option<PeerSourceCursor>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PeerOrigin {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub lineage_id: TaskLineageId,
    pub root_event_id: EventId,
    pub lineage_revision: Revision,
    pub sources: Vec<PeerSource>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PeerSource {
    pub event_id: EventId,
    pub content_hash: ContentHash,
    /// Bounded, authenticated text only; excludes attachment bodies and references.
    pub excerpt: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PeerSender {
    pub session_id: SessionId,
    pub turn_task_id: TurnTaskId,
    pub tool_call_id: ToolCallId,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeerDispatchReceipt {
    pub deliveries: Vec<PeerDeliveryView>,
    pub used: u32,
    pub limit: u32,
    pub nearing_limit: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeerDeliveryView {
    pub delivery_id: EventId,
    pub kind: PeerDeliveryKind,
    pub source_session_id: SessionId,
    pub target_session_id: SessionId,
    pub target_run_id: Option<AgentRunId>,
    pub state: PeerDeliveryState,
    pub reason: Option<String>,
    pub result: Option<String>,
    /// One bounded page of currently authorized source metadata, without text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<PeerSourceEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_source_cursor: Option<PeerSourceCursor>,
    /// Source access can change independently of the durable delivery receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_catalog_issue: Option<PeerSourceCatalogIssue>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PeerSourceCursor {
    pub delivery_id: EventId,
    pub lineage_revision: Revision,
    #[schemars(range(min = 1))]
    pub after_sequence: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PeerSourceCatalogIssue {
    Unavailable,
    RestartFromFirstPage,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PeerSourceEntry {
    pub event_id: EventId,
    pub source_sequence: u32,
    pub content_hash: ContentHash,
    /// Unicode scalar count of the same authorized text that peers.read_source pages.
    pub total_chars: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PeerDeliveryState {
    Queued,
    Started,
    Completed,
    Failed,
    Cancelled,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeerLookupResult {
    /// Empty means no matching durable receipt was visible. It is not permission to replay.
    pub deliveries: Vec<PeerDeliveryView>,
}
