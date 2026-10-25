use aura_domain_common::ErrorEnvelope;
use aura_execution_contract::SideEffectStateV1;
use aura_tool_contract::MAX_TOOL_EXECUTION_ATTEMPTS;
use aura_tool_domain::ToolExecutionSemanticsV1;
use thiserror::Error;

#[derive(Clone, Copy, Debug)]
pub struct ToolRetryAssessmentV1<'a> {
    pub error: &'a ErrorEnvelope,
    /// Core-only classification derived from trusted typed P04/Gateway
    /// evidence. The ErrorEnvelope flag alone is never authority to retry.
    pub trusted_source_retryable: bool,
    pub execution_semantics: &'a ToolExecutionSemanticsV1,
    pub side_effect_state: SideEffectStateV1,
    pub attempt_number: u8,
    pub max_retries: u8,
    pub cancellation_requested: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolRetryDecisionV1 {
    Retry { next_attempt_number: u8 },
    Finalize { reason: ToolRetryStopReasonV1 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolRetryStopReasonV1 {
    Cancelled,
    ErrorNotRetryable,
    ContractDisallowsAutomaticRetry,
    SideEffectNotSafelyRepeatable,
    RetryBudgetExhausted,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ToolRetryAssessmentError {
    #[error("retry assessment attempt number or frozen max_retries is invalid")]
    InvalidFrozenPlan,
}

/// Applies every frozen retry gate to a deterministic failed Attempt. This
/// function never treats an unknown or partial side effect as retryable.
pub fn assess_failed_attempt_retry(
    assessment: ToolRetryAssessmentV1<'_>,
) -> Result<ToolRetryDecisionV1, ToolRetryAssessmentError> {
    if assessment.attempt_number == 0
        || assessment.attempt_number > MAX_TOOL_EXECUTION_ATTEMPTS
        || assessment.max_retries >= MAX_TOOL_EXECUTION_ATTEMPTS
        || assessment.attempt_number.saturating_sub(1) > assessment.max_retries
    {
        return Err(ToolRetryAssessmentError::InvalidFrozenPlan);
    }
    if assessment.cancellation_requested {
        return Ok(finalize(ToolRetryStopReasonV1::Cancelled));
    }
    if !assessment.trusted_source_retryable || !assessment.error.retryable {
        return Ok(finalize(ToolRetryStopReasonV1::ErrorNotRetryable));
    }
    if !assessment.execution_semantics.automatic_retry_allowed {
        return Ok(finalize(ToolRetryStopReasonV1::ContractDisallowsAutomaticRetry));
    }
    if !side_effect_is_safely_repeatable(
        assessment.side_effect_state,
        assessment.execution_semantics,
    ) {
        return Ok(finalize(ToolRetryStopReasonV1::SideEffectNotSafelyRepeatable));
    }
    if assessment.attempt_number > assessment.max_retries {
        return Ok(finalize(ToolRetryStopReasonV1::RetryBudgetExhausted));
    }
    Ok(ToolRetryDecisionV1::Retry { next_attempt_number: assessment.attempt_number + 1 })
}

const fn side_effect_is_safely_repeatable(
    side_effect_state: SideEffectStateV1,
    semantics: &ToolExecutionSemanticsV1,
) -> bool {
    match side_effect_state {
        SideEffectStateV1::NoEffect | SideEffectStateV1::Compensated => true,
        SideEffectStateV1::FullyApplied => semantics.repeat_is_idempotent,
        SideEffectStateV1::NotStarted
        | SideEffectStateV1::PartiallyApplied
        | SideEffectStateV1::Unknown
        | SideEffectStateV1::Unassessed => false,
    }
}

const fn finalize(reason: ToolRetryStopReasonV1) -> ToolRetryDecisionV1 {
    ToolRetryDecisionV1::Finalize { reason }
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{CorrelationId, ErrorCategory, ErrorCode, ErrorEnvelope};
    use aura_tool_domain::ToolExecutionSemanticsV1;

    use super::*;

    fn error(retryable: bool) -> ErrorEnvelope {
        ErrorEnvelope::new(
            ErrorCode::new("tool.runtime.dispatch_failed").expect("error code"),
            ErrorCategory::Unavailable,
            "The tool runtime dispatch failed.",
            retryable,
            CorrelationId::new(),
            None,
        )
    }

    fn semantics(
        automatic_retry_allowed: bool,
        repeat_is_idempotent: bool,
    ) -> ToolExecutionSemanticsV1 {
        ToolExecutionSemanticsV1 {
            automatic_retry_allowed,
            repeat_is_idempotent,
            may_create_persistent_effects: true,
        }
    }

    #[test]
    fn max_retries_three_allows_attempts_two_through_four_only() {
        let retryable = error(true);
        let contract = semantics(true, false);
        for (attempt_number, expected) in [
            (1, ToolRetryDecisionV1::Retry { next_attempt_number: 2 }),
            (2, ToolRetryDecisionV1::Retry { next_attempt_number: 3 }),
            (3, ToolRetryDecisionV1::Retry { next_attempt_number: 4 }),
            (
                4,
                ToolRetryDecisionV1::Finalize {
                    reason: ToolRetryStopReasonV1::RetryBudgetExhausted,
                },
            ),
        ] {
            assert_eq!(
                assess_failed_attempt_retry(ToolRetryAssessmentV1 {
                    error: &retryable,
                    trusted_source_retryable: true,
                    execution_semantics: &contract,
                    side_effect_state: SideEffectStateV1::NoEffect,
                    attempt_number,
                    max_retries: 3,
                    cancellation_requested: false,
                })
                .expect("valid frozen plan"),
                expected
            );
        }
    }

    #[test]
    fn every_retry_gate_fails_closed() {
        let retryable = error(true);
        let not_retryable = error(false);
        let allowed = semantics(true, true);
        let denied = semantics(false, true);
        let assessment = |error, execution_semantics, side_effect_state, cancellation_requested| {
            assess_failed_attempt_retry(ToolRetryAssessmentV1 {
                error,
                trusted_source_retryable: error.retryable,
                execution_semantics,
                side_effect_state,
                attempt_number: 1,
                max_retries: 3,
                cancellation_requested,
            })
            .expect("valid frozen plan")
        };
        assert_eq!(
            assessment(&retryable, &allowed, SideEffectStateV1::NoEffect, true),
            finalize(ToolRetryStopReasonV1::Cancelled)
        );
        assert_eq!(
            assessment(&not_retryable, &allowed, SideEffectStateV1::NoEffect, false),
            finalize(ToolRetryStopReasonV1::ErrorNotRetryable)
        );
        assert_eq!(
            assess_failed_attempt_retry(ToolRetryAssessmentV1 {
                error: &retryable,
                trusted_source_retryable: false,
                execution_semantics: &allowed,
                side_effect_state: SideEffectStateV1::NoEffect,
                attempt_number: 1,
                max_retries: 3,
                cancellation_requested: false,
            })
            .expect("valid plan"),
            finalize(ToolRetryStopReasonV1::ErrorNotRetryable)
        );
        assert_eq!(
            assessment(&retryable, &denied, SideEffectStateV1::NoEffect, false),
            finalize(ToolRetryStopReasonV1::ContractDisallowsAutomaticRetry)
        );
        for unsafe_state in [
            SideEffectStateV1::NotStarted,
            SideEffectStateV1::PartiallyApplied,
            SideEffectStateV1::Unknown,
            SideEffectStateV1::Unassessed,
        ] {
            assert_eq!(
                assessment(&retryable, &allowed, unsafe_state, false),
                finalize(ToolRetryStopReasonV1::SideEffectNotSafelyRepeatable)
            );
        }
    }

    #[test]
    fn fully_applied_requires_the_frozen_idempotency_contract() {
        let retryable = error(true);
        assert_eq!(
            assess_failed_attempt_retry(ToolRetryAssessmentV1 {
                error: &retryable,
                trusted_source_retryable: true,
                execution_semantics: &semantics(true, false),
                side_effect_state: SideEffectStateV1::FullyApplied,
                attempt_number: 1,
                max_retries: 1,
                cancellation_requested: false,
            })
            .expect("valid plan"),
            finalize(ToolRetryStopReasonV1::SideEffectNotSafelyRepeatable)
        );
        assert_eq!(
            assess_failed_attempt_retry(ToolRetryAssessmentV1 {
                error: &retryable,
                trusted_source_retryable: true,
                execution_semantics: &semantics(true, true),
                side_effect_state: SideEffectStateV1::FullyApplied,
                attempt_number: 1,
                max_retries: 1,
                cancellation_requested: false,
            })
            .expect("valid plan"),
            ToolRetryDecisionV1::Retry { next_attempt_number: 2 }
        );
    }
}
