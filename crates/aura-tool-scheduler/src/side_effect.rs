use aura_domain_common::ArtifactId;
use aura_domain_common::ToolExecutionAttemptId;
use aura_execution_contract::SideEffectStateV1;
use aura_tool_contract::ToolExecutionAttemptOutcomeRefV1;
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconciledAttemptSideEffectV1 {
    pub resolved_state: SideEffectStateV1,
    pub evidence_ref: ArtifactId,
    pub covers_attempt: bool,
    pub covers_resource_claims: bool,
    pub covers_effect_scope: bool,
    pub deterministic: bool,
}

impl ReconciledAttemptSideEffectV1 {
    fn safely_resolves_unknown(&self) -> bool {
        !matches!(self.resolved_state, SideEffectStateV1::Unknown | SideEffectStateV1::NotStarted)
            && self.covers_attempt
            && self.covers_resource_claims
            && self.covers_effect_scope
            && self.deterministic
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttemptSideEffectFactV1 {
    pub attempt: ToolExecutionAttemptOutcomeRefV1,
    pub reconciliation: Option<ReconciledAttemptSideEffectV1>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CallSideEffectEvidenceV1 {
    /// A deterministic postcondition proves the intended final state and
    /// covers every ResourceClaim and effect scope touched by the call.
    pub final_postcondition_verified: bool,
    /// Every earlier non-compensated effect is part of that verified final
    /// state rather than an uncovered extra effect.
    pub all_effects_covered_by_final_state: bool,
    /// Every effect was compensated and the complete pre-call baseline was
    /// deterministically restored.
    pub baseline_restoration_verified: bool,
    /// Multiple successful effect applications were idempotent, deduplicated,
    /// or otherwise proven not to have created duplicate side effects.
    pub duplicate_effects_absent_or_deduplicated: bool,
    /// Internal-only evidence for the call-level aggregation decision.
    pub aggregate_evidence_ref: Option<ArtifactId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SideEffectAggregationV1 {
    pub state: SideEffectStateV1,
    pub aggregate_evidence_ref: Option<ArtifactId>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SideEffectAggregationError {
    #[error("attempt facts must be uniquely ordered from one through N")]
    InvalidAttemptOrder,
    #[error("an undispatched attempt cannot report a dispatched side-effect state")]
    UndispatchedAttemptHasEffect,
    #[error("a dispatched attempt cannot report not_started")]
    DispatchedAttemptNotStarted,
}

/// Conservatively reconciles per-attempt side-effect facts into the single
/// ToolCall state. The caller supplies only trusted Core evidence; Runtime
/// claims alone must never set the call-level state.
pub fn aggregate_side_effect_state(
    attempts: &[AttemptSideEffectFactV1],
    evidence: &CallSideEffectEvidenceV1,
) -> Result<SideEffectAggregationV1, SideEffectAggregationError> {
    validate_attempts(attempts)?;
    if attempts.is_empty() {
        return Ok(aggregation(SideEffectStateV1::NotStarted, evidence));
    }

    let states = attempts.iter().map(effective_attempt_state).collect::<Vec<_>>();

    if states.contains(&SideEffectStateV1::Unknown) {
        return Ok(aggregation(SideEffectStateV1::Unknown, evidence));
    }

    let has_partial = states.contains(&SideEffectStateV1::PartiallyApplied);
    let has_fully_applied = states.contains(&SideEffectStateV1::FullyApplied);
    let has_compensated = states.contains(&SideEffectStateV1::Compensated);
    let effectful_attempts = states
        .iter()
        .filter(|state| {
            matches!(
                state,
                SideEffectStateV1::FullyApplied
                    | SideEffectStateV1::PartiallyApplied
                    | SideEffectStateV1::Compensated
            )
        })
        .count();

    if (evidence.final_postcondition_verified
        || evidence.all_effects_covered_by_final_state
        || evidence.baseline_restoration_verified
        || evidence.duplicate_effects_absent_or_deduplicated)
        && evidence.aggregate_evidence_ref.is_none()
    {
        return Ok(aggregation(SideEffectStateV1::Unknown, evidence));
    }

    if has_partial
        && !(evidence.final_postcondition_verified && evidence.all_effects_covered_by_final_state)
        && !evidence.baseline_restoration_verified
    {
        return Ok(aggregation(SideEffectStateV1::PartiallyApplied, evidence));
    }

    if evidence.final_postcondition_verified && evidence.all_effects_covered_by_final_state {
        if effectful_attempts > 1 && !evidence.duplicate_effects_absent_or_deduplicated {
            return Ok(aggregation(SideEffectStateV1::Unknown, evidence));
        }
        return Ok(aggregation(SideEffectStateV1::FullyApplied, evidence));
    }

    if (has_fully_applied || has_partial || has_compensated)
        && evidence.baseline_restoration_verified
    {
        return Ok(aggregation(SideEffectStateV1::Compensated, evidence));
    }

    if has_partial {
        return Ok(aggregation(SideEffectStateV1::PartiallyApplied, evidence));
    }
    if has_fully_applied {
        if effectful_attempts > 1 && !evidence.duplicate_effects_absent_or_deduplicated {
            return Ok(aggregation(SideEffectStateV1::Unknown, evidence));
        }
        return Ok(aggregation(SideEffectStateV1::FullyApplied, evidence));
    }
    if has_compensated {
        return Ok(aggregation(SideEffectStateV1::Compensated, evidence));
    }
    if attempts.iter().any(|fact| fact.attempt.dispatched) {
        return Ok(aggregation(SideEffectStateV1::NoEffect, evidence));
    }
    Ok(aggregation(SideEffectStateV1::NotStarted, evidence))
}

fn effective_attempt_state(fact: &AttemptSideEffectFactV1) -> SideEffectStateV1 {
    if fact.attempt.dispatched
        && fact.attempt.side_effect_evidence_ref.is_none()
        && fact.attempt.side_effect_state != SideEffectStateV1::Unknown
    {
        return SideEffectStateV1::Unknown;
    }
    if fact.attempt.side_effect_state != SideEffectStateV1::Unknown {
        return fact.attempt.side_effect_state;
    }
    fact.reconciliation
        .as_ref()
        .filter(|reconciliation| reconciliation.safely_resolves_unknown())
        .map_or(SideEffectStateV1::Unknown, |reconciliation| reconciliation.resolved_state)
}

fn validate_attempts(
    attempts: &[AttemptSideEffectFactV1],
) -> Result<(), SideEffectAggregationError> {
    for (index, fact) in attempts.iter().enumerate() {
        if fact.attempt.attempt_number != u8::try_from(index + 1).unwrap_or(0)
            || index > 0
                && fact.attempt.tool_execution_attempt_id
                    == attempts[index - 1].attempt.tool_execution_attempt_id
        {
            return Err(SideEffectAggregationError::InvalidAttemptOrder);
        }
        match (fact.attempt.dispatched, fact.attempt.side_effect_state) {
            (false, SideEffectStateV1::NotStarted) => {}
            (false, _) => return Err(SideEffectAggregationError::UndispatchedAttemptHasEffect),
            (true, SideEffectStateV1::NotStarted) => {
                return Err(SideEffectAggregationError::DispatchedAttemptNotStarted);
            }
            (true, _) => {}
        }
    }
    let mut ids = attempts
        .iter()
        .map(|fact| fact.attempt.tool_execution_attempt_id)
        .collect::<Vec<ToolExecutionAttemptId>>();
    ids.sort_unstable();
    if ids.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(SideEffectAggregationError::InvalidAttemptOrder);
    }
    Ok(())
}

fn aggregation(
    state: SideEffectStateV1,
    evidence: &CallSideEffectEvidenceV1,
) -> SideEffectAggregationV1 {
    SideEffectAggregationV1 { state, aggregate_evidence_ref: evidence.aggregate_evidence_ref }
}

#[cfg(test)]
mod tests {
    use aura_domain_common::ArtifactId;
    use aura_domain_common::ToolExecutionAttemptId;

    use super::*;

    fn fact(
        attempt_number: u8,
        state: SideEffectStateV1,
        dispatched: bool,
    ) -> AttemptSideEffectFactV1 {
        AttemptSideEffectFactV1 {
            attempt: ToolExecutionAttemptOutcomeRefV1 {
                tool_execution_attempt_id: ToolExecutionAttemptId::new(),
                attempt_number,
                side_effect_state: state,
                side_effect_evidence_ref: dispatched.then(ArtifactId::new),
                dispatched,
            },
            reconciliation: None,
        }
    }

    fn final_state_evidence(deduplicated: bool) -> CallSideEffectEvidenceV1 {
        CallSideEffectEvidenceV1 {
            final_postcondition_verified: true,
            all_effects_covered_by_final_state: true,
            baseline_restoration_verified: false,
            duplicate_effects_absent_or_deduplicated: deduplicated,
            aggregate_evidence_ref: Some(ArtifactId::new()),
        }
    }

    #[test]
    fn all_undispatched_is_not_started_and_all_dispatched_no_effect_is_no_effect() {
        let not_started = aggregate_side_effect_state(
            &[fact(1, SideEffectStateV1::NotStarted, false)],
            &CallSideEffectEvidenceV1::default(),
        )
        .expect("aggregate");
        assert_eq!(not_started.state, SideEffectStateV1::NotStarted);

        let no_effect = aggregate_side_effect_state(
            &[
                fact(1, SideEffectStateV1::NoEffect, true),
                fact(2, SideEffectStateV1::NoEffect, true),
            ],
            &CallSideEffectEvidenceV1::default(),
        )
        .expect("aggregate");
        assert_eq!(no_effect.state, SideEffectStateV1::NoEffect);
    }

    #[test]
    fn unresolved_unknown_survives_a_later_success() {
        let aggregate = aggregate_side_effect_state(
            &[
                fact(1, SideEffectStateV1::Unknown, true),
                fact(2, SideEffectStateV1::FullyApplied, true),
            ],
            &final_state_evidence(true),
        )
        .expect("aggregate");
        assert_eq!(aggregate.state, SideEffectStateV1::Unknown);
    }

    #[test]
    fn missing_attempt_or_call_level_evidence_fails_closed_to_unknown() {
        let mut applied = fact(1, SideEffectStateV1::FullyApplied, true);
        applied.attempt.side_effect_evidence_ref = None;
        let aggregate =
            aggregate_side_effect_state(&[applied], &CallSideEffectEvidenceV1::default())
                .expect("aggregate");
        assert_eq!(aggregate.state, SideEffectStateV1::Unknown);

        let evidence_without_ref = CallSideEffectEvidenceV1 {
            final_postcondition_verified: true,
            all_effects_covered_by_final_state: true,
            duplicate_effects_absent_or_deduplicated: true,
            ..CallSideEffectEvidenceV1::default()
        };
        let aggregate = aggregate_side_effect_state(
            &[fact(1, SideEffectStateV1::PartiallyApplied, true)],
            &evidence_without_ref,
        )
        .expect("aggregate");
        assert_eq!(aggregate.state, SideEffectStateV1::Unknown);
    }

    #[test]
    fn unknown_requires_deterministic_attempt_and_scope_reconciliation() {
        let mut unknown = fact(1, SideEffectStateV1::Unknown, true);
        unknown.reconciliation = Some(ReconciledAttemptSideEffectV1 {
            resolved_state: SideEffectStateV1::NoEffect,
            evidence_ref: ArtifactId::new(),
            covers_attempt: true,
            covers_resource_claims: true,
            covers_effect_scope: false,
            deterministic: true,
        });
        let aggregate = aggregate_side_effect_state(
            std::slice::from_ref(&unknown),
            &CallSideEffectEvidenceV1::default(),
        )
        .expect("aggregate");
        assert_eq!(aggregate.state, SideEffectStateV1::Unknown);

        unknown.reconciliation.as_mut().expect("reconciliation").covers_effect_scope = true;
        let aggregate = aggregate_side_effect_state(
            &[unknown, fact(2, SideEffectStateV1::FullyApplied, true)],
            &final_state_evidence(true),
        )
        .expect("aggregate");
        assert_eq!(aggregate.state, SideEffectStateV1::FullyApplied);
    }

    #[test]
    fn uncovered_partial_is_partial_but_verified_final_state_can_cover_it() {
        let partial = fact(1, SideEffectStateV1::PartiallyApplied, true);
        let aggregate = aggregate_side_effect_state(
            std::slice::from_ref(&partial),
            &CallSideEffectEvidenceV1::default(),
        )
        .expect("aggregate");
        assert_eq!(aggregate.state, SideEffectStateV1::PartiallyApplied);

        let aggregate = aggregate_side_effect_state(&[partial], &final_state_evidence(true))
            .expect("aggregate");
        assert_eq!(aggregate.state, SideEffectStateV1::FullyApplied);
    }

    #[test]
    fn verified_baseline_restoration_is_compensated_not_no_effect() {
        let evidence = CallSideEffectEvidenceV1 {
            baseline_restoration_verified: true,
            aggregate_evidence_ref: Some(ArtifactId::new()),
            ..CallSideEffectEvidenceV1::default()
        };
        let aggregate = aggregate_side_effect_state(
            &[
                fact(1, SideEffectStateV1::PartiallyApplied, true),
                fact(2, SideEffectStateV1::Compensated, true),
            ],
            &evidence,
        )
        .expect("aggregate");
        assert_eq!(aggregate.state, SideEffectStateV1::Compensated);
    }

    #[test]
    fn repeated_fully_applied_attempts_require_deduplication_evidence() {
        let attempts = [
            fact(1, SideEffectStateV1::FullyApplied, true),
            fact(2, SideEffectStateV1::FullyApplied, true),
        ];
        let aggregate = aggregate_side_effect_state(&attempts, &final_state_evidence(false))
            .expect("aggregate");
        assert_eq!(aggregate.state, SideEffectStateV1::Unknown);

        let aggregate =
            aggregate_side_effect_state(&attempts, &final_state_evidence(true)).expect("aggregate");
        assert_eq!(aggregate.state, SideEffectStateV1::FullyApplied);
    }

    #[test]
    fn dispatch_and_attempt_order_invariants_fail_closed() {
        assert_eq!(
            aggregate_side_effect_state(
                &[fact(2, SideEffectStateV1::NoEffect, true)],
                &CallSideEffectEvidenceV1::default(),
            ),
            Err(SideEffectAggregationError::InvalidAttemptOrder)
        );
        assert_eq!(
            aggregate_side_effect_state(
                &[fact(1, SideEffectStateV1::FullyApplied, false)],
                &CallSideEffectEvidenceV1::default(),
            ),
            Err(SideEffectAggregationError::UndispatchedAttemptHasEffect)
        );
        assert_eq!(
            aggregate_side_effect_state(
                &[fact(1, SideEffectStateV1::NotStarted, true)],
                &CallSideEffectEvidenceV1::default(),
            ),
            Err(SideEffectAggregationError::DispatchedAttemptNotStarted)
        );
    }
}
