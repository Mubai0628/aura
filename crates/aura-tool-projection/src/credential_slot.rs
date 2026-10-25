use super::ToolBoundCredentialSummaryV1;
use aura_tool_contract::CredentialSlotKey;
use aura_tool_domain::ToolCredentialPlacementV1;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCredentialSlotBindingSummaryV1 {
    pub slot_key: CredentialSlotKey,
    pub placement: ToolCredentialPlacementV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rebind_required_reason: Option<String>,
    pub bound_credential: Option<ToolBoundCredentialSummaryV1>,
}
