use aura_domain_common::ToolId;
use aura_domain_common::{
    ArtifactId, ContentHash, ContextProjectionSnapshotId, EventId, Revision, TaskLineageId,
    WorkspaceId,
};
use aura_tool_contract::ToolVersion;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextSourceRef {
    Event { event_id: EventId },
    Artifact { artifact_id: ArtifactId },
    Compaction { context_snapshot_id: ContextProjectionSnapshotId },
    ToolSchema { tool_id: ToolId, tool_version: ToolVersion },
    PromptModule { module_id: String, content_hash: ContentHash },
    WorkspaceFile { workspace_id: WorkspaceId, relative_path: String, content_hash: ContentHash },
    TaskLineage { lineage_id: TaskLineageId, revision: Revision },
    RuntimeState { state_kind: String, state_revision: Revision },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPrivacyClass {
    ModelVisible,
    UserOnly,
    InternalOnly,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextCompressibility {
    NeverCompress,
    CompressibleHistory,
    CompressibleSealedActiveSegment,
    UserReducibleOnly,
}

pub const ON_DEMAND_HISTORY_STATE: &str = "agent.history_recovery";
pub const TOOL_RESULT_REFERENCE_PREFIX: &str = "[Tool result stored by reference:";

#[must_use]
pub fn is_tool_result_reference(text: &str) -> bool {
    text.starts_with(TOOL_RESULT_REFERENCE_PREFIX)
        || text.starts_with("[Earlier tool result cleared to save context:")
}

/// The only abbreviated representation accepted for an authenticated long user message.
/// Byte boundaries are reproducible from the durable original on every provider path.
pub fn user_event_excerpt(event: EventId, text: &str) -> Option<String> {
    const EDGE: usize = 1024;
    if text.len() <= EDGE * 3 {
        return None;
    }
    let first = text.floor_char_boundary(EDGE);
    let last = text.ceil_char_boundary(text.len() - EDGE);
    Some(format!(
        "{}\n[Aura excerpt of {event}; {} bytes omitted; original digest {}. Use history.search and history.read for the original.]\n{}",
        &text[..first],
        last - first,
        ContentHash::sha256(text.as_bytes()).hex,
        &text[last..]
    ))
}
