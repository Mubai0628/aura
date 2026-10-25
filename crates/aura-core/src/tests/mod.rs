use aura_domain_common::{CommandInvocationId, CorrelationId, RuntimeInstanceId};
use aura_ipc::JsonRpcResponse;
use aura_ipc::method;
use aura_runtime_contract::{CommandOutcome, IpcRequestTerminal};
use serde_json::json;

use super::{
    CommandOutcomeLedger, CoreOptions, CoreTestMode, is_side_effecting_main_method,
    response_with_size_fallback,
};
use crate::command_claims::MAX_COMMAND_OUTCOMES;

// Behavior tests are real child modules; shared fixtures are imported explicitly.
mod command_outcomes;
mod options;
mod response_limits;
