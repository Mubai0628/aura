//! Provider-owned history stays bound to its original endpoint and model.
//! It is protocol data, never assistant-visible prose or a tool argument.
use crate::{ModelExecutionSnapshot, ProviderToolCallReuseDomain};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogicalProviderHistory {
    pub reuse_domain: ProviderToolCallReuseDomain,
    pub model_name: String,
    pub data: ProviderHistoryData,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderHistoryData {
    Chat {
        reasoning_content: Option<String>,
        /// Exact provider call ID -> Google thought signature, when supplied.
        thought_signatures: BTreeMap<String, String>,
    },
    Responses {
        reasoning_items: Vec<Value>,
    },
    Anthropic {
        thinking_blocks: Vec<Value>,
    },
}

impl LogicalProviderHistory {
    #[must_use]
    pub fn for_snapshot(&self, snapshot: &ModelExecutionSnapshot) -> Option<&ProviderHistoryData> {
        (self.reuse_domain.endpoint_id == snapshot.endpoint_id
            && self.reuse_domain.endpoint_security_fingerprint == snapshot.endpoint_fingerprint
            && self.reuse_domain.api_format == snapshot.api_format
            && self.model_name == snapshot.model_name)
            .then_some(&self.data)
    }
}
