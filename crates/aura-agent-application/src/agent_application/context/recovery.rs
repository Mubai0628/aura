//! Fit a recoverable view of the session without marking omitted events as summarized.
use super::*;
use std::collections::BTreeSet;

pub(super) fn has_gap(input: &ContextPreparationInput) -> bool {
    input.section_inputs.iter().flat_map(|s| &s.fragments)
        .any(|f| matches!(&f.source_ref, ContextSourceRef::RuntimeState { state_kind,.. } if state_kind == ON_DEMAND_HISTORY_STATE))
}

pub(in crate::agent_application) fn record_gap(input: &mut ContextPreparationInput, note: &str) {
    let section =
        input.section_inputs.iter_mut().find(|s| s.kind == ContextSectionKind::CurrentTaskState);
    if let Some(section) = section {
        let source_ref = ContextSourceRef::RuntimeState {
            state_kind: ON_DEMAND_HISTORY_STATE.into(),
            state_revision: Revision::INITIAL,
        };
        if let Some(previous) = section.fragments.iter_mut().find(|f| f.source_ref == source_ref) {
            if !previous.content.contains(note) {
                previous.content.push('\n');
                previous.content.push_str(note);
            }
        } else {
            section.fragments.push(ContextFragment {
                source_ref,
                source_kind: ModelEgressSourceKind::AuraCore,
                data_class: NetworkDataClass::WorkspaceMetadata,
                authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
                content: note.into(),
            });
        }
    }
}

pub(in crate::agent_application) fn clear_continuation(input: &mut ContextPreparationInput) {
    if let ContextPreparationTriggerV1::ToolContinuation { provider_continuation, .. } =
        &mut input.trigger
    {
        *provider_continuation = None;
    }
    if let Some(facts) = &mut input.tool_continuation_facts {
        facts.verified_provider_continuation = None;
    }
}

/// Local history could not be carried forward or completely summarized at this boundary.
pub(super) fn adopted_prefix_unavailable(
    budget: &ContextBudgetSnapshot,
    non_compressible_tokens: u64,
) -> ContextRemediationPlan {
    let actions = vec!["verify_context_window".to_owned()];
    ContextRemediationPlan {
        overflow: ContextOverflowDiagnosticView {
            request_input_budget_tokens: budget.request_input_budget_tokens,
            estimated_input_tokens: budget.estimated_input_tokens,
            non_compressible_tokens,
            compressible_history_tokens: budget
                .estimated_input_tokens
                .saturating_sub(non_compressible_tokens),
            user_reducible_tokens: 0,
            overflow_tokens: budget
                .estimated_input_tokens
                .saturating_sub(budget.request_input_budget_tokens),
            error_code: "context.adopted_history_prefix_unavailable".to_owned(),
            remediation: actions.clone(),
        },
        actions,
    }
}

/// First bound tool payloads. Then omit complete older groups, retaining the continuation and
/// its accepted input suffix. No omitted event is claimed in a new compaction's coverage.
pub(super) fn fit(
    input: &mut ContextPreparationInput,
    budget: &ContextBudgetSnapshot,
    deferred_images: &mut BTreeSet<ArtifactId>,
) -> anyhow::Result<bool> {
    let extra = budget.estimated_input_tokens.saturating_sub(budget.request_input_budget_tokens);
    let byte_total = budget.item_estimates.iter().map(|i| i.byte_length).sum::<u64>();
    let needed = extra
        .saturating_add(256)
        .saturating_mul(byte_total)
        .div_ceil(budget.estimated_input_tokens.max(1))
        .max(256);
    let mut results = BTreeSet::new();
    for item in &input.recent_history_items {
        if let LogicalConversationItem::ToolResult { tool_call_id, content, .. } = item
            && !is_cleared_tool_result(content)
            && content.len() as u64 >= needed.min(4096)
        {
            results.insert(*tool_call_id);
        }
    }
    let mut changed = clear_tool_results(&mut input.recent_history_items, &results) > 0;
    if !changed {
        let mut keep = BTreeSet::new();
        if let Some(LogicalConversationItem::User { source_event_id, content, .. }) = input
            .recent_history_items
            .iter()
            .rev()
            .find(|i| matches!(i, LogicalConversationItem::User { .. }))
            && content.len() as u64 <= budget.request_input_budget_tokens / 8
        {
            keep.insert(*source_event_id);
        }
        match &input.trigger {
            ContextPreparationTriggerV1::CoreContinuation {
                originating_user_event_id,
                assistant_response_event_id,
                ..
            }
            | ContextPreparationTriggerV1::ToolContinuation {
                originating_user_event_id,
                assistant_response_event_id,
                ..
            } => {
                keep.extend([*originating_user_event_id, *assistant_response_event_id]);
            }
            _ => {}
        }
        for addition in input
            .tool_continuation_facts
            .iter()
            .flat_map(|f| &f.accepted_additions)
            .chain(input.core_continuation_facts.iter().flat_map(|f| &f.accepted_additions))
        {
            keep.insert(addition.user_event_id);
        }
        let mut removed = BTreeSet::new();
        let mut freed = 0;
        let mut items = input.recent_history_items.as_slice();
        while let Some(first) = items.first() {
            let count = match first {
                LogicalConversationItem::Assistant { tool_calls, .. } => 1 + tool_calls.len(),
                _ => 1,
            };
            if count > items.len() {
                break;
            }
            let (group, rest) = items.split_at(count);
            items = rest;
            if keep.contains(&crate::agent_application::compaction::event_id(first)) {
                continue;
            }
            for item in group {
                removed.insert(crate::agent_application::compaction::event_id(item));
                freed += match item {
                    LogicalConversationItem::User { content, .. }
                    | LogicalConversationItem::ToolResult { content, .. } => content.len() as u64,
                    LogicalConversationItem::Assistant { content, tool_calls, .. } => {
                        content.as_ref().map_or(0, |s| s.len()) as u64
                            + tool_calls.iter().map(|c| c.arguments_json.len() as u64).sum::<u64>()
                    }
                } + 16;
            }
            if freed >= needed {
                break;
            }
        }
        input.recent_history_items.retain(|item| {
            !removed.contains(&crate::agent_application::compaction::event_id(item))
        });
        changed = !removed.is_empty();
    }
    if !changed {
        // A summary may exceed the *physical request* in a smaller window. Keep the stored
        // summary intact and expose a navigable excerpt for this request.
        if let Some(f) = input
            .section_inputs
            .iter_mut()
            .find(|s| s.kind == ContextSectionKind::HistoricalSnapshotSummary)
            .and_then(|s| s.fragments.first_mut())
            && let ContextSourceRef::Compaction { context_snapshot_id } = f.source_ref
            && f.content.len() > 512
        {
            let cut = f.content.len().saturating_sub(needed as usize).min(f.content.len() / 2);
            let prefix = f.content.floor_char_boundary(cut);
            f.content.truncate(prefix);
            f.content.push_str(&format!(
                "\n[Full historical summary: context.read_compaction {context_snapshot_id}.]"
            ));
            changed = true;
        }
    }
    if !changed {
        changed = project_user_input(input);
    }
    if !changed
        && let Some(image) = input
            .attachments
            .iter()
            .filter(|image| !deferred_images.contains(&image.artifact_id))
            .max_by_key(|image| image.byte_length)
    {
        let id = image.artifact_id;
        let name = serde_json::to_string(&image.display_name)?;
        deferred_images.insert(id);
        record_gap(
            input,
            &format!(
                "Image {name} ({id}) remains in the session but did not fit in this request. Its pixels were not delivered. Continue other work and request a specific crop or page only if its contents are needed."
            ),
        );
        changed = true;
    }
    if changed {
        super::clearing::rebuild(input)?;
        clear_continuation(input);
        record_gap(
            input,
            "Older details remain in the session. Use history.list_turns, history.search, history.read or context.read_tool_result for exact material; continue from the current request and recorded outcomes.",
        );
    }
    Ok(changed)
}

fn project_user_input(input: &mut ContextPreparationInput) -> bool {
    let mut changed = false;
    for fragment in input
        .section_inputs
        .iter_mut()
        .filter(|s| s.kind == ContextSectionKind::CurrentTurnWorkingSet)
        .flat_map(|s| &mut s.fragments)
    {
        if let ContextSourceRef::Event { event_id } = fragment.source_ref
            && fragment.source_kind == ModelEgressSourceKind::UserEvent
            && let Some(excerpt) = user_event_excerpt(event_id, &fragment.content)
        {
            fragment.content = excerpt;
            changed = true;
        }
    }
    let root = match &input.trigger {
        ContextPreparationTriggerV1::CoreContinuation { originating_user_event_id, .. }
        | ContextPreparationTriggerV1::ToolContinuation { originating_user_event_id, .. } => {
            Some(*originating_user_event_id)
        }
        _ => None,
    };
    let additions: BTreeSet<_> = input
        .tool_continuation_facts
        .iter()
        .flat_map(|f| &f.accepted_additions)
        .chain(input.core_continuation_facts.iter().flat_map(|f| &f.accepted_additions))
        .filter(|addition| addition.excerpt_hash.is_some())
        .map(|addition| addition.user_event_id)
        .collect();
    for item in &mut input.recent_history_items {
        if let LogicalConversationItem::User { source_event_id, content, content_hash } = item
            && (Some(*source_event_id) == root || additions.contains(source_event_id))
            && let Some(excerpt) = user_event_excerpt(*source_event_id, content)
        {
            *content_hash = ContentHash::sha256(excerpt.as_bytes());
            *content = excerpt;
            changed = true;
        }
    }
    changed
}

/// A retry following a provider window correction must reduce the actual request.
pub(super) fn no_progress(
    trial: Option<&aura_persistence_v2::model::context_window::WindowCorrection>,
    snapshot: &ContextProjectionSnapshot,
) -> Option<ContextRemediationPlan> {
    let trial = trial?;
    let aura_model_domain::ContextWindowResolution::P06RebuildRequired {
        local_estimated_input_tokens,
        ..
    } = trial.view.correction.resolution
    else {
        return None;
    };
    if snapshot.budget.estimated_input_tokens < local_estimated_input_tokens {
        return None;
    }
    let mut non_compressible = 0u64;
    let mut reducible = 0u64;
    for section in &snapshot.sections {
        if matches!(
            section.compressibility,
            ContextCompressibility::NeverCompress | ContextCompressibility::UserReducibleOnly
        ) {
            non_compressible = non_compressible.saturating_add(section.estimated_tokens);
        }
        if section.compressibility == ContextCompressibility::UserReducibleOnly {
            reducible = reducible.saturating_add(section.estimated_tokens);
        }
    }
    let actions = vec!["reduce_user_managed_context".into()];
    Some(ContextRemediationPlan {
        overflow: ContextOverflowDiagnosticView {
            request_input_budget_tokens: snapshot.budget.request_input_budget_tokens,
            estimated_input_tokens: snapshot.budget.estimated_input_tokens,
            non_compressible_tokens: non_compressible,
            compressible_history_tokens: snapshot
                .budget
                .estimated_input_tokens
                .saturating_sub(non_compressible),
            user_reducible_tokens: reducible,
            overflow_tokens: snapshot
                .budget
                .estimated_input_tokens
                .saturating_sub(snapshot.budget.request_input_budget_tokens),
            error_code: "context.provider_rebuild_no_progress".into(),
            remediation: actions.clone(),
        },
        actions,
    })
}
