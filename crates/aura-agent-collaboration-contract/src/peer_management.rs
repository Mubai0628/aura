use crate::PeerDeliveryView;
use aura_domain_common::{EventId, SessionId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_PEER_BUDGET: u32 = 1_024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeerBudgetDefault {
    pub limit: u32,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeerBudget {
    pub root_event_id: EventId,
    pub used: u32,
    pub limit: u32,
    pub revision: u64,
    pub stopped: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeerMailboxEntry {
    pub delivery: PeerDeliveryView,
    pub budget: PeerBudget,
    /// Only the user at the root session can stop the entire run or change its limit.
    pub manages_run: bool,
    pub review_text: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeerActiveBudget {
    pub delivery_id: EventId,
    pub budget: PeerBudget,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeerReviewPolicy {
    pub required: bool,
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeerMailbox {
    pub collaboration_enabled: bool,
    pub budget_default: PeerBudgetDefault,
    pub entries: Vec<PeerMailboxEntry>,
    pub review_policy: PeerReviewPolicy,
    pub before: Option<EventId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_budget: Option<PeerActiveBudget>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeerMailboxRequest {
    pub session_id: Option<SessionId>,
    pub before: Option<EventId>,
    #[serde(default)]
    pub view: PeerMailboxView,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PeerMailboxView {
    #[default]
    All,
    Active,
    Settings,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeerManageRequest {
    pub session_id: Option<SessionId>,
    pub action: PeerManageAction,
    #[serde(default)]
    pub view: PeerMailboxView,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PeerManageAction {
    SetDefault {
        expected_revision: u64,
        #[schemars(range(min = 1, max = 1024))]
        limit: u32,
    },
    Increase {
        delivery_id: EventId,
        expected_revision: u64,
        #[schemars(range(min = 1, max = 1024))]
        limit: u32,
    },
    StopRun {
        delivery_id: EventId,
        expected_revision: u64,
    },
    Cancel {
        delivery_id: EventId,
    },
    /// The requestor opened this result; it clears only the prompt.
    MarkResultSeen {
        delivery_id: EventId,
    },
    SetReview {
        expected_revision: u64,
        required: bool,
    },
    Release {
        delivery_id: EventId,
        #[schemars(length(min = 1, max = 8000), regex(pattern = r"\S"))]
        revised_text: Option<String>,
    },
    /// Schedule only unsettled outbox entries. Never reopen a bound receiver run.
    WakeQueued,
}
