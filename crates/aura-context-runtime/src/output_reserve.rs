//! The rule: a request is compacted only when this input and the next output do not
//! fit the window. What "the next output" means is refined: not the model's whole output limit, which
//! most steps never come near, but at least [`OUTPUT_RESERVE_FLOOR_TOKENS`]; a request that fits is
//! then sent with all the room the window has left for its output, up to the limit.
use aura_context_domain::{
    ContextBudgetError, ContextBudgetSnapshot, TokenEstimateExactness, request_input_budget_v1,
};

/// The output every request keeps room for. A model whose limit is lower keeps its own limit.
pub const OUTPUT_RESERVE_FLOOR_TOKENS: u64 = 32_000;

/// The output a request must leave room for before it may be sent without compacting.
#[must_use]
pub fn trigger_output_reserve(configured_max_output_tokens: u64) -> u64 {
    configured_max_output_tokens.min(OUTPUT_RESERVE_FLOOR_TOKENS)
}

/// A budget that fits with the trigger reserve, given the room the window has left for the
/// output: the request's output limit becomes that room, never above the configured limit.
pub fn with_room_for_output(
    budget: ContextBudgetSnapshot,
    configured_max_output_tokens: u64,
) -> Result<ContextBudgetSnapshot, ContextBudgetError> {
    let room = budget
        .runtime_effective_window_tokens
        .checked_sub(budget.safety_reserve_tokens)
        .and_then(|left| left.checked_sub(budget.estimated_input_tokens))
        .ok_or(ContextBudgetError::RequestBudgetExceeded)?;
    let output = configured_max_output_tokens.min(room);
    if output < budget.output_reserve_tokens {
        return Err(ContextBudgetError::RequestBudgetExceeded);
    }
    // Parts counted by the rule of thumb can be longer than counted, and a provider refuses a
    // request whose input and output limit together pass the window: the output asks for no more
    // than would fit were those parts twice as long, never for less than the reserve it fits with.
    let output = output
        .min(room.saturating_sub(approximate_slack(&budget)))
        .max(budget.output_reserve_tokens);
    let (safety_reserve_tokens, request_input_budget_tokens) =
        request_input_budget_v1(budget.runtime_effective_window_tokens, output)?;
    Ok(ContextBudgetSnapshot {
        effective_max_output_tokens: output,
        output_reserve_tokens: output,
        safety_reserve_tokens,
        request_input_budget_tokens,
        ..budget
    })
}

/// How much longer the parts counted by the rule of thumb may be: as long again, never past
/// their bytes.
fn approximate_slack(budget: &ContextBudgetSnapshot) -> u64 {
    budget
        .item_estimates
        .iter()
        .filter(|item| item.estimator.exactness == TokenEstimateExactness::Approximate)
        .map(|item| {
            item.estimated_tokens.min(item.byte_length.saturating_sub(item.estimated_tokens))
        })
        .fold(0, u64::saturating_add)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_context_domain::{
        CHARACTER_CLASS_ESTIMATOR_ID, CONTEXT_BUDGET_POLICY_V1, ContextItemEstimate,
        TokenEstimatorIdentity,
    };
    use aura_domain_common::ContentHash;

    fn fitting(window: u64, input: u64, reserve: u64) -> ContextBudgetSnapshot {
        let (safety, budget) = request_input_budget_v1(window, reserve).expect("budget");
        ContextBudgetSnapshot {
            policy: CONTEXT_BUDGET_POLICY_V1.to_owned(),
            runtime_effective_window_tokens: window,
            estimated_input_tokens: input,
            effective_max_output_tokens: reserve,
            output_reserve_tokens: reserve,
            safety_reserve_tokens: safety,
            request_input_budget_tokens: budget,
            item_estimates: Vec::new(),
        }
    }

    #[test]
    fn the_trigger_keeps_the_floor_or_a_lower_limit() {
        assert_eq!(trigger_output_reserve(64_000), 32_000);
        assert_eq!(trigger_output_reserve(1_024), 1_024);
    }

    #[test]
    fn a_fitting_request_gets_the_room_left_up_to_its_limit() {
        // gpt-6.1-sol on the owner's endpoint: a 256,000 window, a 64,000 limit, 12,800 safety.
        let budget = fitting(256_000, 200_000, trigger_output_reserve(64_000));
        let sent = with_room_for_output(budget, 64_000).expect("fits");
        assert_eq!(sent.output_reserve_tokens, 256_000 - 12_800 - 200_000);
        assert_eq!(sent.effective_max_output_tokens, sent.output_reserve_tokens);
        assert_eq!(sent.request_input_budget_tokens, 200_000);
        let sent = with_room_for_output(fitting(256_000, 100_000, 32_000), 64_000).expect("fits");
        assert_eq!(sent.output_reserve_tokens, 64_000, "never above the limit");
        assert_eq!(sent.request_input_budget_tokens, 256_000 - 12_800 - 64_000);
    }

    fn approximate(tokens: u64, bytes: u64) -> ContextItemEstimate {
        ContextItemEstimate {
            ordinal: 1,
            content_hash: ContentHash::sha256(b"records"),
            byte_length: bytes,
            estimated_tokens: tokens,
            estimator: TokenEstimatorIdentity {
                estimator_id: CHARACTER_CLASS_ESTIMATOR_ID.to_owned(),
                estimator_version: 1,
                exactness: TokenEstimateExactness::Approximate,
            },
        }
    }

    /// A first request counted by the rule asks for output as if it were twice as long, so an
    /// estimate that was low still leaves the provider's window room for input and output.
    #[test]
    fn a_request_counted_by_the_rule_asks_for_output_as_if_twice_as_long() {
        let mut budget = fitting(256_000, 60_000, trigger_output_reserve(200_000));
        budget.item_estimates = vec![approximate(60_000, 180_000)];
        let sent = with_room_for_output(budget, 200_000).expect("fits");
        assert_eq!(sent.effective_max_output_tokens, 256_000 - 12_800 - 120_000);
        // Never less than the reserve it fits with, and never more than its bytes again.
        let mut tight = fitting(256_000, 200_000, 32_000);
        tight.item_estimates = vec![approximate(200_000, 600_000)];
        assert_eq!(
            with_room_for_output(tight, 64_000).expect("fits").effective_max_output_tokens,
            32_000
        );
        let mut dense = fitting(256_000, 60_000, 32_000);
        dense.item_estimates = vec![approximate(60_000, 70_000)];
        let sent = with_room_for_output(dense, 200_000).expect("fits");
        assert_eq!(sent.effective_max_output_tokens, 256_000 - 12_800 - 70_000);
    }

    #[test]
    fn a_request_without_the_floor_left_does_not_fit() {
        let budget = fitting(256_000, 220_000, 32_000);
        assert_eq!(
            with_room_for_output(budget, 64_000),
            Err(ContextBudgetError::RequestBudgetExceeded)
        );
    }
}
