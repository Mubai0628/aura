//! The rule: a request is compacted only when this input and the next output do not
//! fit the window. What "the next output" means is refined: not the model's whole output limit, which
//! most steps never come near, but at least [`OUTPUT_RESERVE_FLOOR_TOKENS`]; a request that fits is
//! then sent with all the room the window has left for its output, up to the limit.
use aura_context_domain::{ContextBudgetError, ContextBudgetSnapshot, request_input_budget_v1};

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

#[cfg(test)]
mod tests {
    use super::*;
    use aura_context_domain::CONTEXT_BUDGET_POLICY_V1;

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

    #[test]
    fn a_request_without_the_floor_left_does_not_fit() {
        let budget = fitting(256_000, 220_000, 32_000);
        assert_eq!(
            with_room_for_output(budget, 64_000),
            Err(ContextBudgetError::RequestBudgetExceeded)
        );
    }
}
