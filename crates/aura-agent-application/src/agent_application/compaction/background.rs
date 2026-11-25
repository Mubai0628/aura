//! Apply a verified session checkpoint and retain the currently visible scope.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::agent_application) enum BackgroundUse {
    Absent,
    Applied,
    SourcesUnavailable,
}

pub(in crate::agent_application) struct AuthorizedCheckpoint {
    pub(crate) record: Option<Box<store::Compaction>>,
    skipped: bool,
}

pub(in crate::agent_application) fn authorized_checkpoint(
    db: &aura_persistence_core::Connection,
    session: SessionId,
    run: AgentRunId,
    access: &dependencies::Access,
) -> aura_persistence_core::StoreResult<AuthorizedCheckpoint> {
    let (record, skipped) = store::usable_checkpoint(db, session, Some(run), |sources| {
        dependencies::visible(sources, access)
    })?;
    Ok(AuthorizedCheckpoint { record: record.map(Box::new), skipped })
}

/// Select an available checkpoint and its source proofs in the same store read.
pub(in crate::agent_application) async fn apply(
    app: &Arc<AgentApplication>,
    input: &mut ContextPreparationInput,
    checkpoint: AuthorizedCheckpoint,
) -> anyhow::Result<BackgroundUse> {
    let section_input = input.clone();
    let run_id = input.request.agent_run_id;
    let AuthorizedCheckpoint { record, skipped } = checkpoint;
    let applied = {
        let journal = Arc::clone(&app.execution.journal);
        app.store
            .run_critical(
                StoreStep::run(run_id, "agent.compaction.apply"),
                StoreAccess::Read,
                move || -> anyhow::Result<_> {
                    let journal = journal.lock()?;
                    let db = journal.database.connection();
                    let Some(record) = record else {
                        return Ok(None);
                    };
                    let demanded =
                        events_that_need_coverage(&section_input, &record.excluded_event_ids);
                    let hops = aura_persistence_v2::agent::coverage_hops_for_events(
                        db, &record, &demanded,
                    )?;
                    let proofs = compacted_source_proofs(db, &section_input, &hops)?;
                    let recent = aura_persistence_v2::agent::rounds::recent(
                        db,
                        section_input.request.session_id,
                        summary::RECENT_ROUNDS,
                        Some(record.created_at),
                    )?;
                    Ok(Some((record, hops, proofs, recent)))
                },
            )
            .await?
    };
    if skipped {
        crate::agent_application::context::recovery::record_gap(
            input,
            "An earlier history checkpoint is unavailable. Use history.list_turns, history.search or history.read for details outside the visible range.",
        );
    }
    let Some((record, hops, proofs, recent)) = applied else {
        if skipped {
            input
                .section_inputs
                .retain(|s| s.kind != ContextSectionKind::HistoricalSnapshotSummary);
            input.inspector.compaction.source_context_snapshot_id = None;
            crate::agent_application::context::recovery::clear_continuation(input);
        }
        input.compacted_source_proofs.clear();
        input.coverage_hops.clear();
        return Ok(if skipped { BackgroundUse::SourcesUnavailable } else { BackgroundUse::Absent });
    };
    let same_background = input.previous_snapshot.as_ref().is_some_and(|snapshot|
        snapshot.sections.iter().flat_map(|section| &section.source_refs).any(|source|
            matches!(source, ContextSourceRef::Compaction { context_snapshot_id } if *context_snapshot_id == record.id)));
    if !same_background || skipped {
        crate::agent_application::context::recovery::clear_continuation(input);
    }
    let result = record
        .result
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("context.compaction_result_missing"))?;
    let covered: std::collections::BTreeSet<_> =
        record.excluded_event_ids.iter().copied().collect();
    input.recent_history_items.retain(|i| !covered.contains(&selection::event_id(i)));
    crate::agent_application::context::rebuild_history_fragments(input)?;
    let summary = ContextSectionInput {
        kind: ContextSectionKind::HistoricalSnapshotSummary,
        compressibility: ContextCompressibility::CompressibleHistory,
        privacy_class: ContextPrivacyClass::ModelVisible,
        fragments: vec![ContextFragment {
            source_ref: ContextSourceRef::Compaction { context_snapshot_id: record.id },
            source_kind: ModelEgressSourceKind::Compaction,
            data_class: aura_security_contract::NetworkDataClass::WorkspaceSource,
            authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
            content: summary::section(result, &recent),
        }],
    };
    if let Some(old) = input.section_inputs.iter_mut().find(|s| s.kind == summary.kind) {
        *old = summary;
    } else {
        input.section_inputs.push(summary);
    }
    input.inspector.compaction = PromptInspectorCompactionView {
        state: ContextCompactionStateView::Completed,
        source_context_snapshot_id: Some(record.id),
        retained_recent_tokens: record.retained_recent_tokens,
        retention_exceptions: record.retention_exceptions,
    };
    input.compacted_source_proofs = proofs;
    input.coverage_hops = hops;
    Ok(BackgroundUse::Applied)
}

/// Accepted additions whose text this compaction removed. A turn with none of
/// them does not walk older summaries.
fn events_that_need_coverage(
    input: &ContextPreparationInput,
    excluded: &[aura_domain_common::EventId],
) -> Vec<aura_domain_common::EventId> {
    let mut events = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut collect = |additions: &[AcceptedTurnAdditionV1]| {
        for addition in additions {
            if excluded.contains(&addition.user_event_id) && seen.insert(addition.user_event_id) {
                events.push(addition.user_event_id);
            }
        }
    };
    if let Some(facts) = &input.tool_continuation_facts {
        collect(&facts.accepted_additions);
    }
    if let Some(facts) = &input.core_continuation_facts {
        collect(&facts.accepted_additions);
    }
    events
}

/// Binds proofs while the stored user text is still known. The summary that
/// replaces that text is not a proof and is not written into one.
fn compacted_source_proofs(
    db: &aura_persistence_core::Connection,
    input: &ContextPreparationInput,
    hops: &[CoverageHop],
) -> anyhow::Result<Vec<UserSourceProof>> {
    let excluded = hops
        .iter()
        .flat_map(|hop| hop.excluded_event_ids.iter().copied())
        .collect::<std::collections::BTreeSet<_>>();
    let mut additions = Vec::new();
    if let Some(facts) = &input.tool_continuation_facts {
        additions.extend(facts.accepted_additions.iter().cloned());
    }
    if let Some(facts) = &input.core_continuation_facts {
        additions.extend(facts.accepted_additions.iter().cloned());
    }
    let mut proofs = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for addition in additions {
        if !excluded.contains(&addition.user_event_id) || !seen.insert(addition.user_event_id) {
            continue;
        }
        let Some(text) = stored_addition_text(db, input, &addition)? else {
            continue;
        };
        if emitted_text_hash(&text) != addition.content_hash {
            continue;
        }
        let Ok(anchor) = user_history_anchor_hash(addition.user_event_id, &text) else {
            continue;
        };
        if let Some(proof) = prove_user_source(addition.user_event_id, &text, &anchor) {
            proofs.push(proof);
        }
    }
    Ok(proofs)
}

fn stored_addition_text(
    db: &aura_persistence_core::Connection,
    input: &ContextPreparationInput,
    addition: &AcceptedTurnAdditionV1,
) -> anyhow::Result<Option<String>> {
    if let Some(text) = input.recent_history_items.iter().find_map(|item| {
        let LogicalConversationItem::User { source_event_id, content, content_hash } = item else {
            return None;
        };
        (*source_event_id == addition.user_event_id && *content_hash == addition.content_hash)
            .then(|| content.clone())
    }) {
        return Ok(Some(text));
    }
    aura_persistence_v2::agent::session::user_text(
        db,
        input.request.session_id,
        addition.user_event_id,
    )
    .map_err(|error| anyhow::anyhow!(error))
}
