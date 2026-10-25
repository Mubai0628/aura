use crate::{AttemptRef, DispatchBinding, ExecutionContractError as Error, SideEffectStateV1};
use aura_domain_common::jcs::CanonicalSha256HashV1;
use aura_domain_common::{ArtifactId, UtcTimestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionReceipt {
    pub attempt: AttemptRef,
    pub evidence_ref: ArtifactId,
    pub postcondition_fingerprint: Option<CanonicalSha256HashV1>,
    pub rollback: RollbackEvidence,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(tag = "rollback", rename_all = "snake_case", deny_unknown_fields)]
pub enum RollbackEvidence {
    NotApplicable,
    Available { evidence_ref: ArtifactId },
    Completed { evidence_ref: ArtifactId },
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(tag = "termination", rename_all = "snake_case", deny_unknown_fields)]
pub enum TerminationEvidence {
    Confirmed {
        evidence_ref: ArtifactId,
    },
    /// The direct process exited; descendants remain in Aura-owned Jobs.
    RuntimeManaged {
        evidence_ref: ArtifactId,
    },
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionOutcome {
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionResult {
    pub outcome: ExecutionOutcome,
    pub side_effect: SideEffectStateV1,
    pub receipt: ExecutionReceipt,
    pub termination: TerminationEvidence,
    pub completed_at: UtcTimestamp,
    pub warnings: Vec<String>,
}

impl ExecutionResult {
    pub fn validate(&self) -> Result<(), Error> {
        if self.side_effect == SideEffectStateV1::NotStarted
            || matches!(self.outcome, ExecutionOutcome::Cancelled | ExecutionOutcome::TimedOut)
                && !matches!(self.termination, TerminationEvidence::Confirmed { .. })
            || self.outcome == ExecutionOutcome::Succeeded
                && !matches!(
                    self.side_effect,
                    SideEffectStateV1::NoEffect
                        | SideEffectStateV1::FullyApplied
                        | SideEffectStateV1::Unknown
                        | SideEffectStateV1::Unassessed
                )
            || self.outcome == ExecutionOutcome::Succeeded
                && self.side_effect == SideEffectStateV1::Unknown
                && !self.warnings.iter().any(|w| w == "tool.side_effect.unverified")
        {
            return Err(Error::InvalidResult);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttemptState {
    Prepared,
    Running,
    WaitingExternal,
    CancelRequested,
    Terminal { result: Box<ExecutionResult> },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionAttempt {
    pub binding: DispatchBinding,
    pub prepared_at: UtcTimestamp,
    pub dispatched_at: Option<UtcTimestamp>,
    pub state: AttemptState,
}

impl ExecutionAttempt {
    pub fn mark_dispatched(&mut self, at: UtcTimestamp) -> Result<(), Error> {
        if self.state != AttemptState::Prepared
            || self.dispatched_at.is_some()
            || at < self.prepared_at
        {
            return Err(Error::AttemptMismatch);
        }
        self.dispatched_at = Some(at);
        self.state = AttemptState::Running;
        Ok(())
    }

    pub fn complete(&mut self, result: ExecutionResult) -> Result<(), Error> {
        if matches!(self.state, AttemptState::Terminal { .. }) {
            return Err(Error::AttemptMismatch);
        }
        let candidate =
            Self { state: AttemptState::Terminal { result: Box::new(result) }, ..self.clone() };
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    pub fn validate(&self) -> Result<(), Error> {
        if self.dispatched_at.is_some_and(|t| t < self.prepared_at) {
            return Err(Error::AttemptMismatch);
        }
        match &self.state {
            AttemptState::Prepared if self.dispatched_at.is_none() => Ok(()),
            AttemptState::Terminal { result } => {
                result.validate()?;
                let before_dispatch = self.dispatched_at.is_none()
                    && matches!(
                        result.outcome,
                        ExecutionOutcome::Failed | ExecutionOutcome::Cancelled
                    )
                    && result.side_effect == SideEffectStateV1::NoEffect
                    && matches!(result.termination, TerminationEvidence::Confirmed { .. })
                    && result.receipt.rollback == RollbackEvidence::NotApplicable
                    && result.warnings.iter().any(|v| v == "execution.not_dispatched");
                if result.receipt.attempt != self.binding.attempt
                    || result.completed_at < self.prepared_at
                    || self.dispatched_at.is_none() && !before_dispatch
                    || self.dispatched_at.is_some_and(|t| result.completed_at < t)
                {
                    return Err(Error::AttemptMismatch);
                }
                Ok(())
            }
            AttemptState::Running
            | AttemptState::WaitingExternal
            | AttemptState::CancelRequested
                if self.dispatched_at.is_some() =>
            {
                Ok(())
            }
            _ => Err(Error::AttemptMismatch),
        }
    }
}
