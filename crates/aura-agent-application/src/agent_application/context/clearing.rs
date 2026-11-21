//! Preserve previously projected tool results and shorten completed tool inputs.
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
