use aura_domain_common::{
    AgentRunId, ArtifactId, CausationId, CorrelationId, EventId, SessionId, TurnTaskId,
    UtcTimestamp, WorkspaceId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventActor {
    User,
    ParentAgent,
    ChildAgent,
    SystemRuntime,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PersistentEventDraft {
    pub event_id: EventId,
    pub workspace_id: Option<WorkspaceId>,
    pub session_id: Option<SessionId>,
    pub turn_task_id: Option<TurnTaskId>,
    pub agent_run_id: Option<AgentRunId>,
    pub event_type: String,
    pub event_version: u32,
    pub actor: EventActor,
    pub correlation_id: CorrelationId,
    pub causation_id: Option<CausationId>,
    pub atomic_group_id: Option<String>,
    pub payload_json: Option<Value>,
    pub payload_artifact_ref: Option<ArtifactId>,
    pub searchable_text: Option<String>,
    pub created_at: UtcTimestamp,
}

impl PersistentEventDraft {
    #[must_use]
    pub fn system(
        event_type: impl Into<String>,
        correlation_id: CorrelationId,
        payload_json: Value,
    ) -> Self {
        Self {
            event_id: EventId::new(),
            workspace_id: None,
            session_id: None,
            turn_task_id: None,
            agent_run_id: None,
            event_type: event_type.into(),
            event_version: 1,
            actor: EventActor::SystemRuntime,
            correlation_id,
            causation_id: None,
            atomic_group_id: None,
            payload_json: Some(payload_json),
            payload_artifact_ref: None,
            searchable_text: None,
            created_at: UtcTimestamp::now(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CommittedProjection<T> {
    pub outbox_id: i64,
    pub source_revision: u64,
    pub payload: T,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DatabasePragmas {
    pub foreign_keys: bool,
    pub journal_mode_wal: bool,
    pub busy_timeout_milliseconds: u64,
}

#[derive(Clone, Debug)]
pub struct MigrationRecoveryRequired {
    pub database_path: std::path::PathBuf,
    pub backup_path: Option<std::path::PathBuf>,
    pub reason: String,
}
