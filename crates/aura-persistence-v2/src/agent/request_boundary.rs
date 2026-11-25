//! The small part of a sent request not already recoverable from its context snapshot.
use super::facts;
use aura_domain_common::{
    ContentHash, EventId, ModelStepId, UtcTimestamp, canonical_json_v1_sha256,
};
use aura_model_domain::LogicalModelRequest;
use aura_persistence_core::{PersistenceCoreError, StoreResult};
use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct RequestBoundary {
    pub fixed_tools: ContentHash,
    pub closing_question: Option<String>,
    pub serializer_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_prefix: Option<ContentHash>,
}

impl RequestBoundary {
    pub fn of(
        request: &LogicalModelRequest,
        native_prefix: Option<ContentHash>,
    ) -> StoreResult<Option<Self>> {
        if request.provider_continuation.is_some() || !request.closing_exchange.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self {
            fixed_tools: canonical_json_v1_sha256(&request.fixed_tool_ids)
                .map_err(|_| PersistenceCoreError::Invalid("fixed tool identity"))?,
            closing_question: request.closing_question.clone(),
            serializer_version: request.instruction_envelope.serializer_version,
            // The closing question changes what generated the response, but has no historical
            // item in subsequent ordinary requests. Such an answer has no reusable origin.
            native_prefix: native_prefix.filter(|_| request.closing_question.is_none()),
        }))
    }
    pub fn freeze(
        &self,
        tx: &Transaction<'_>,
        step: ModelStepId,
        at: UtcTimestamp,
    ) -> StoreResult<()> {
        facts::append(tx, EventId::new(), &step.to_string(), "agent.request_boundary", self, at)
    }
    pub fn load(db: &Connection, step: ModelStepId) -> StoreResult<Option<Self>> {
        facts::latest(db, &step.to_string(), "agent.request_boundary")
    }
}
