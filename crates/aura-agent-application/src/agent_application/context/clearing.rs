//! Preserve previously projected tool results and shorten completed tool inputs.
use super::*;
use std::collections::BTreeMap;

/// What the previous request showed cleared stays cleared, so the requests keep one prefix.
pub(super) fn keep_cleared(input: &mut ContextPreparationInput) -> anyhow::Result<()> {
    let kept: BTreeMap<_, _> = input
        .previous_snapshot
        .iter()
        .flat_map(|snapshot| &snapshot.conversation_items)
        .filter_map(|item| match item {
            LogicalConversationItem::ToolResult { source_event_id, content, .. }
                if is_tool_result_reference(content) =>
            {
                Some((*source_event_id, item))
            }
            _ => None,
        })
        .collect();
    let mut changed = false;
    for item in &mut input.recent_history_items {
        if let LogicalConversationItem::ToolResult { source_event_id, .. } = item
            && let Some(prior) = kept.get(source_event_id)
            && item != *prior
        {
            *item = (*prior).clone();
            changed = true;
        }
    }
    if changed {
        rebuild(input)?;
    }
    Ok(())
}

/// A call's long text is not repeated once it has run
/// ([`aura_context_runtime::shorten_long_arguments`]).
pub(super) fn shorten_arguments(input: &mut ContextPreparationInput) -> anyhow::Result<()> {
    if aura_context_runtime::shorten_long_arguments(&mut input.recent_history_items) == 0 {
        return Ok(());
    }
    rebuild(input)
}

/// The history section, rebuilt from the items as the request will show them.
pub(in crate::agent_application) fn rebuild(
    input: &mut ContextPreparationInput,
) -> anyhow::Result<()> {
    let history = input
        .section_inputs
        .iter_mut()
        .find(|section| section.kind == ContextSectionKind::RecentHistory)
        .ok_or_else(|| anyhow::anyhow!("context.history_section_missing"))?;
    let peer = history.fragments.iter().find(|f| f.source_kind == ModelEgressSourceKind::PeerTask);
    history.fragments = input
        .recent_history_items
        .iter()
        .map(|item| {
            let mut fragment = history_fragment(item)?;
            // A peer envelope uses a User conversation slot, with its original source authority.
            if let Some(peer) = peer.filter(|peer| peer.source_ref == fragment.source_ref) {
                fragment.source_kind = peer.source_kind;
                fragment.authorization_basis = peer.authorization_basis.clone();
                fragment.data_class = peer.data_class;
            }
            Ok(fragment)
        })
        .collect::<anyhow::Result<_>>()?;
    Ok(())
}
