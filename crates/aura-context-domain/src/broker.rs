use crate::*;
use aura_domain_common::{AgentRunId, SessionId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "kind", content = "request", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextBrokerReadRequest {
    Status { session_id: SessionId, agent_run_id: AgentRunId },
    Search(ContextSearchInput),
    ReadEvents(ContextReadEventsInput),
    ReadToolResult(ContextReadToolResultInput),
    ReadCompaction(ContextReadCompactionInput),
    MemorySettings,
}
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextBrokerReadResult {
    pub value: serde_json::Value,
}
