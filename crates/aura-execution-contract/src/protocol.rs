use crate::{AttemptRef, ExecutionResult, SecuredOperation, TerminationEvidence};
use aura_domain_common::jcs::CanonicalSha256HashV1;
use aura_security_contract::BackendRequirement;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PrepareRequest {
    pub operation: SecuredOperation,
    pub requirement: BackendRequirement,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionHandle {
    pub attempt: AttemptRef,
    pub dispatch_fingerprint: CanonicalSha256HashV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CancelReceipt {
    pub handle: ExecutionHandle,
    pub termination: TerminationEvidence,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(tag = "reconciliation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReconcileResult {
    Running {
        handle: ExecutionHandle,
    },
    Completed {
        result: Box<ExecutionResult>,
    },
    /// Dispatch status or effect is not known. This never authorizes retry on
    /// another backend, even when that backend is currently ready.
    Unknown {
        attempt: AttemptRef,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize, thiserror::Error)]
#[serde(tag = "error", rename_all = "snake_case", deny_unknown_fields)]
pub enum BackendError {
    #[error("backend is unavailable: {reason}")]
    Unavailable { reason: String },
    #[error("backend rejected preparation: {reason}")]
    PreparationRejected { reason: String },
    #[error("dispatch was rejected before execution: {reason}")]
    DispatchRejected { reason: String },
    #[error("dispatch outcome is unknown")]
    DispatchUnknown { attempt: AttemptRef },
}
