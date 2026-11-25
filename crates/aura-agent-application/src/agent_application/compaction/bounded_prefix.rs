//! The bounded auxiliary request when a whole previously sent prefix cannot be reused.
use super::*;
use aura_model_domain::{ModelAttachmentSnapshot, ResolvedModelAttachment};
use aura_model_transport::{
    ProviderTools, archived_deepseek_group, historical_tool_call_wire_bindings,
    provider_history_replay, provider_reasoning_content, provider_tool_call,
    uses_deepseek_history_archive,
};
use std::collections::BTreeSet;

pub(super) fn through_coverage(
    original: &RequestPrefix,
    anchors: &CompactionAnchorManifest,
    snapshot: &ModelExecutionSnapshot,
) -> anyhow::Result<RequestPrefix> {
    let covered: BTreeSet<_> = anchors
        .anchors
        .iter()
        .filter_map(|anchor| match anchor.source_ref {
            ContextSourceRef::Event { event_id } => Some(event_id),
            _ => None,
        })
        .collect();
    let end = original
        .conversation_items
        .iter()
        .rposition(|item| covered.contains(&event_id(item)))
        .ok_or_else(|| anyhow::anyhow!("context.compaction_prefix_covers_nothing"))?;
    let items = &original.conversation_items[..=end];
    let retained: BTreeSet<_> = items.iter().map(event_id).collect();
    anyhow::ensure!(covered.is_subset(&retained), "context.compaction_prefix_missing_anchor");

    let attachments = original
        .attachments
        .iter()
        .filter(|item| retained.contains(&item.source_event_id))
        .cloned()
        .collect::<Vec<_>>();
    let omitted_media: BTreeSet<_> = original
        .attachments
        .iter()
        .filter(|item| !retained.contains(&item.source_event_id))
        .map(|item| item.artifact_id)
        .collect();
    let egress_sources = original
        .egress_sources
        .iter()
        .filter(|source| match source.source_ref {
            ContextSourceRef::Event { event_id } => retained.contains(&event_id),
            ContextSourceRef::Artifact { artifact_id } => !omitted_media.contains(&artifact_id),
            _ => true,
        })
        .cloned()
        .collect();
    let mut prefix = RequestPrefix {
        closing_question: None,
        instruction_envelope: original.instruction_envelope.clone(),
        conversation_items: items.to_vec(),
        tool_schema_set: original.tool_schema_set.clone(),
        attachments,
        provider_continuation: None,
        egress_sources,
        estimated_input_tokens: 0,
    };
    prefix.estimated_input_tokens = measure(&prefix, snapshot)?;
    Ok(prefix)
}

pub(super) fn media(
    prefix: &RequestPrefix,
    resolved: &[ResolvedModelAttachment],
) -> Arc<[ResolvedModelAttachment]> {
    prefix
        .attachments
        .iter()
        .filter_map(|snapshot| resolved.iter().find(|item| &item.snapshot == snapshot).cloned())
        .collect()
}

fn measure(prefix: &RequestPrefix, snapshot: &ModelExecutionSnapshot) -> anyhow::Result<u64> {
    let mut text = Vec::new();
    let mut push = |canonical_text: String, kind: BudgetTextKind| -> anyhow::Result<()> {
        text.push(CanonicalTextBudgetItem {
            ordinal: u32::try_from(text.len() + 1)?,
            content_hash: ContentHash::sha256(canonical_text.as_bytes()),
            canonical_text,
            kind,
        });
        Ok(())
    };
    for block in &prefix.instruction_envelope.ordered_blocks {
        push(block.content.clone(), BudgetTextKind::Text)?;
    }
    let tools = ProviderTools::compile(&prefix.tool_schema_set, snapshot.api_format)?;
    for definition in tools.definitions() {
        push(serde_json::to_string(definition)?, BudgetTextKind::Text)?;
    }
    let bindings = historical_tool_call_wire_bindings(snapshot);
    let archive = uses_deepseek_history_archive(
        snapshot,
        &prefix.instruction_envelope,
        &prefix.tool_schema_set,
    );
    let mut rest = prefix.conversation_items.as_slice();
    while let Some((item, tail)) = rest.split_first() {
        if archive && let Some((count, text)) = archived_deepseek_group(rest, snapshot) {
            push(text, BudgetTextKind::Text)?;
            rest = &rest[count..];
            continue;
        }
        match item {
            LogicalConversationItem::User { content, .. }
            | LogicalConversationItem::ToolResult { content, .. } => {
                push(content.clone(), BudgetTextKind::Text)?;
            }
            LogicalConversationItem::Assistant {
                content, tool_calls, provider_history, ..
            } => {
                if let Some(history) = provider_history
                    && let Some(replay) = provider_history_replay(history, snapshot)
                {
                    let (reasoning, opaque) = provider_reasoning_content(replay.as_ref());
                    let tokens = history.reasoning_tokens.unwrap_or_else(|| {
                        reasoning
                            .iter()
                            .map(|text| reference_text_tokens(text))
                            .sum::<u64>()
                            .saturating_add(opaque.div_ceil(8))
                    });
                    push(
                        serde_json::to_string(replay.as_ref())?,
                        BudgetTextKind::ProviderHistory { tokens },
                    )?;
                }
                if let Some(content) = content {
                    push(content.clone(), BudgetTextKind::Text)?;
                }
                for call in tool_calls {
                    let wire = provider_tool_call(snapshot.api_format, call, &bindings)?;
                    push(serde_json::to_string(&wire)?, BudgetTextKind::Text)?;
                }
            }
        }
        rest = tail;
    }
    let media = media_budget(&prefix.attachments, text.len())?;
    let budget = super::super::estimators::for_model(snapshot.model_profile_id).estimate_budget(
        &ModelEstimatorKey {
            provider_id: snapshot.provider_id,
            provider_model_name: snapshot.model_name.clone(),
        },
        snapshot.runtime_effective_window_tokens,
        1,
        &text,
        &media,
    )?;
    Ok(budget.estimated_input_tokens)
}

fn media_budget(
    attachments: &[ModelAttachmentSnapshot],
    text_count: usize,
) -> anyhow::Result<Vec<NativeAttachmentBudgetItem>> {
    attachments
        .iter()
        .enumerate()
        .map(|(i, attachment)| {
            Ok(NativeAttachmentBudgetItem {
                ordinal: u32::try_from(text_count + i + 1)?,
                attachment: attachment.clone(),
            })
        })
        .collect()
}
