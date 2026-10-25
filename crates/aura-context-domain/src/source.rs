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
