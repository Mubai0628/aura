//! Old tool results give their room back before history
//! is summarized ([`aura_context_runtime::next_clearing_batch`]). This applies it to a step's
//! history and to the section the request is built from.
use super::*;
use std::collections::BTreeSet;

/// What the previous request showed cleared stays cleared, so the requests keep one prefix.
pub(super) fn keep_cleared(input: &mut ContextPreparationInput) -> anyhow::Result<()> {
    let kept = input
        .previous_snapshot
        .as_ref()
        .map(|snapshot| cleared_tool_results(&snapshot.conversation_items))
        .unwrap_or_default();
    apply(input, &kept)
}

/// Clears the next batch of a request that did not fit; how many results it held (0: none).
/// `oversized`: a compaction pass has not made it fit, so a seen oversized result may go too.
pub(super) fn clear_next(
    input: &mut ContextPreparationInput,
    budget: &ContextBudgetSnapshot,
    oversized: bool,
) -> anyhow::Result<usize> {
    let batch = next_clearing_batch(&input.recent_history_items, planning_rate(budget), oversized);
    apply(input, &batch)?;
    Ok(batch.len())
}

/// A call's long text is not repeated once it has run
/// ([`aura_context_runtime::shorten_long_arguments`]).
pub(super) fn shorten_arguments(input: &mut ContextPreparationInput) -> anyhow::Result<()> {
    if aura_context_runtime::shorten_long_arguments(&mut input.recent_history_items) == 0 {
        return Ok(());
    }
    rebuild(input)
}

fn apply(input: &mut ContextPreparationInput, ids: &BTreeSet<ToolCallId>) -> anyhow::Result<()> {
    if ids.is_empty() || clear_tool_results(&mut input.recent_history_items, ids) == 0 {
        return Ok(());
    }
    rebuild(input)
}

/// The history section, rebuilt from the items as the request will show them.
fn rebuild(input: &mut ContextPreparationInput) -> anyhow::Result<()> {
    let history = input
        .section_inputs
        .iter_mut()
        .find(|section| section.kind == ContextSectionKind::RecentHistory)
        .ok_or_else(|| anyhow::anyhow!("context.history_section_missing"))?;
    history.fragments =
        input.recent_history_items.iter().map(history_fragment).collect::<anyhow::Result<_>>()?;
    Ok(())
}

/// The rate the request's estimate counted its bytes at, as compaction plans with it: one token a
/// byte, or the provider's rate for the parts it was sent before.
fn planning_rate(budget: &ContextBudgetSnapshot) -> (u64, u64) {
    let bytes = budget.item_estimates.iter().map(|item| item.byte_length).sum::<u64>();
    if bytes == 0 || budget.estimated_input_tokens >= bytes {
        (1, 1)
    } else {
        (budget.estimated_input_tokens, bytes)
    }
}
