use std::{borrow::Cow, collections::BTreeSet};

use aura_domain_common::jcs::{CanonicalSha256HashV1, canonical_sha256};
use aura_domain_common::{
    AgentRunId, ArtifactId, ContentHash, EventId, ModelStepId, SessionId, TurnTaskId,
};
use aura_domain_common::{ToolBatchId, ToolCallId, ToolWireVersionV1};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HistoryAtomicGroupKindV1;

impl Serialize for HistoryAtomicGroupKindV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str("aura.p06.history_atomic_group")
    }
}

impl<'de> Deserialize<'de> for HistoryAtomicGroupKindV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        (value == "aura.p06.history_atomic_group")
            .then_some(Self)
            .ok_or_else(|| D::Error::custom("invalid P06 history atomic-group domain"))
    }
}

impl JsonSchema for HistoryAtomicGroupKindV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("HistoryAtomicGroupKindV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "string", "const": "aura.p06.history_atomic_group" })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryToolCallResultEntryV1 {
    pub ordinal: u32,
    pub tool_call_id: ToolCallId,
    pub tool_call_fact_hash: CanonicalSha256HashV1,
    pub tool_result_fact_hash: CanonicalSha256HashV1,
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryAtomicGroupPreimageV1 {
    pub kind: HistoryAtomicGroupKindV1,
    pub preimage_version: ToolWireVersionV1,
    pub session_id: SessionId,
    pub turn_task_id: TurnTaskId,
    pub agent_run_id: AgentRunId,
    pub model_step_id: ModelStepId,
    pub assistant_response_event_id: EventId,
    pub assistant_response_content_hash: CanonicalSha256HashV1,
    pub tool_batch_id: ToolBatchId,
    pub calls: Vec<HistoryToolCallResultEntryV1>,
    pub batch_summary_hash: CanonicalSha256HashV1,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum HistoryAtomicGroupContractError {
    #[error("history Tool Call entries must be non-empty and strictly ordered by ordinal")]
    InvalidCallOrder,
    #[error("history atomic-group hash is invalid")]
    HashInvalid,
}

impl HistoryAtomicGroupPreimageV1 {
    pub fn validate(&self) -> Result<(), HistoryAtomicGroupContractError> {
        let unique_call_count =
            self.calls.iter().map(|entry| entry.tool_call_id).collect::<BTreeSet<_>>().len();
        if self.calls.is_empty()
            || !self.calls.windows(2).all(|pair| pair[0].ordinal < pair[1].ordinal)
            || unique_call_count != self.calls.len()
        {
            return Err(HistoryAtomicGroupContractError::InvalidCallOrder);
        }
        Ok(())
    }

    pub fn group_ref(&self) -> Result<HistoryAtomicGroupRefV1, HistoryAtomicGroupContractError> {
        self.validate()?;
        Ok(HistoryAtomicGroupRefV1 {
            assistant_response_event_id: self.assistant_response_event_id,
            group_hash: canonical_sha256(self)
                .map_err(|_| HistoryAtomicGroupContractError::HashInvalid)?,
        })
    }
}

impl<'de> Deserialize<'de> for HistoryAtomicGroupPreimageV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            kind: HistoryAtomicGroupKindV1,
            preimage_version: ToolWireVersionV1,
            session_id: SessionId,
            turn_task_id: TurnTaskId,
            agent_run_id: AgentRunId,
            model_step_id: ModelStepId,
            assistant_response_event_id: EventId,
            assistant_response_content_hash: CanonicalSha256HashV1,
            tool_batch_id: ToolBatchId,
            calls: Vec<HistoryToolCallResultEntryV1>,
            batch_summary_hash: CanonicalSha256HashV1,
        }
        let wire = Wire::deserialize(deserializer)?;
        let value = Self {
            kind: wire.kind,
            preimage_version: wire.preimage_version,
            session_id: wire.session_id,
            turn_task_id: wire.turn_task_id,
            agent_run_id: wire.agent_run_id,
            model_step_id: wire.model_step_id,
            assistant_response_event_id: wire.assistant_response_event_id,
            assistant_response_content_hash: wire.assistant_response_content_hash,
            tool_batch_id: wire.tool_batch_id,
            calls: wire.calls,
            batch_summary_hash: wire.batch_summary_hash,
        };
        value.validate().map_err(D::Error::custom)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryAtomicGroupRefV1 {
    pub assistant_response_event_id: EventId,
    pub group_hash: CanonicalSha256HashV1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyHistoryAtomicGroupVerificationV1 {
    VerifiedSideMapping,
    LegacyUnverified,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolResultContextProjectionV1 {
    pub full_result_ref: ArtifactId,
    pub full_content_hash: ContentHash,
    pub full_byte_length: u64,
    pub preview: String,
    pub preview_truncated: bool,
}

#[cfg(test)]
mod tests {
    use aura_domain_common::jcs::CanonicalSha256HashV1;
    use aura_domain_common::{AgentRunId, EventId, ModelStepId, SessionId, TurnTaskId};
    use aura_domain_common::{ToolBatchId, ToolCallId};

    use super::{
        HistoryAtomicGroupContractError, HistoryAtomicGroupKindV1, HistoryAtomicGroupPreimageV1,
        HistoryToolCallResultEntryV1,
    };

    fn hash(value: &[u8]) -> CanonicalSha256HashV1 {
        CanonicalSha256HashV1::sha256(value)
    }

    #[test]
    fn history_group_hash_requires_provider_ordinal_order() {
        let entry = |ordinal| HistoryToolCallResultEntryV1 {
            ordinal,
            tool_call_id: ToolCallId::new(),
            tool_call_fact_hash: hash(b"call"),
            tool_result_fact_hash: hash(b"result"),
        };
        let value = HistoryAtomicGroupPreimageV1 {
            kind: HistoryAtomicGroupKindV1,
            preimage_version: aura_domain_common::ToolWireVersionV1,
            session_id: SessionId::new(),
            turn_task_id: TurnTaskId::new(),
            agent_run_id: AgentRunId::new(),
            model_step_id: ModelStepId::new(),
            assistant_response_event_id: EventId::new(),
            assistant_response_content_hash: hash(b"assistant"),
            tool_batch_id: ToolBatchId::new(),
            calls: vec![entry(2), entry(1)],
            batch_summary_hash: hash(b"batch"),
        };
        assert_eq!(value.validate(), Err(HistoryAtomicGroupContractError::InvalidCallOrder));
    }
}
