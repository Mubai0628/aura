use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub cumulative: u64,
    pub task: u64,
    pub request_input: u64,
    pub request_output: u64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cached_input: u64,
    pub cached_input_reported: bool,
    pub reasoning_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct RequestRecord {
    pub id: i64,
    pub task: String,
    pub kind: String,
    pub reserved: u64,
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cached_input: Option<u64>,
    pub state: String,
    pub tool_definition_bytes: u64,
    pub request_body_bytes: u64,
    pub provider_status: Option<u16>,
    pub reasoning_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
}

#[derive(Debug)]
pub(super) enum FlowStop {
    TaskBudget,
    RequestBudget,
    TaskStopped,
    CumulativeBudget,
    GlobalHalt,
    PolicyWidening,
}
impl std::fmt::Display for FlowStop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TaskBudget => "eval.task_budget_exhausted",
            Self::RequestBudget => "eval.request_budget_exhausted",
            Self::TaskStopped => "eval.task_stopped",
            Self::CumulativeBudget => "eval.budget_exhausted",
            Self::GlobalHalt => "eval.ledger_halted",
            Self::PolicyWidening => "eval.task_policy_widening_refused",
        })
    }
}
impl std::error::Error for FlowStop {}
