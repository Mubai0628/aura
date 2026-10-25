use crate::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBrokerInvokeRequest {
    pub registry_snapshot_ref: ToolRegistrySnapshotRef,
    pub action: ToolBrokerAction,
}
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "kind", content = "request", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolBrokerAction {
    Search(ToolSearchRequestV1),
    Describe(ToolDescribeRequestV1),
    ListLoaded,
    Load(ToolLoadRequestV1),
    Unload(ToolLoadRequestV1),
}
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBrokerInvokeResult {
    pub value: serde_json::Value,
    pub resulting_registry_snapshot_ref: Option<ToolRegistrySnapshotRef>,
    pub changed: Option<bool>,
}
