use super::*;
use std::collections::BTreeSet;
pub(super) struct Sources {
    pub record: store::Compaction,
    pub content: String,
}

pub(super) struct AdoptedHistory {
    pub cutoff: SessionSequence,
    pub events: BTreeSet<EventId>,
    pub items_for_read_gap: Option<Vec<LogicalConversationItem>>,
}

/// The rate the request's own estimate counts its bytes at: one token a byte, or the provider's
/// rate for the parts it was sent before. History units are measured in bytes, so they
/// are planned at the same rate as the budget they are planned against.
fn planning_rate(budget: &ContextBudgetSnapshot) -> (u64, u64) {
    let bytes = budget.item_estimates.iter().map(|item| item.byte_length).sum::<u64>();
    if bytes == 0 || budget.estimated_input_tokens >= bytes {
        (1, 1)
    } else {
        (budget.estimated_input_tokens, bytes)
    }
}
pub(in crate::agent_application) fn event_id(item: &LogicalConversationItem) -> EventId {
    match item {
        LogicalConversationItem::User { source_event_id, .. }
        | LogicalConversationItem::Assistant { source_event_id, .. }
        | LogicalConversationItem::ToolResult { source_event_id, .. } => *source_event_id,
    }
}

/// Chooses which history units this compaction may replace.
///
/// Boundary: the whole body held `journal.lock()` across its scans, its `events` lookup and
/// the sequence allocation, so it is ONE synchronous critical section. It used to run on the
/// reactive task that polls the compaction future; it now runs **whole** on the one bounded Agent
/// Store owner, over the same journal, the same statements and the same order. Nothing inside was
/// rewritten: only ownership of the inputs, and the thread that executes it, changed.
pub(super) async fn select(
    app: Arc<AgentApplication>,
    input: Arc<ContextPreparationInput>,
    non_history_tokens: u64,
    budget: ContextBudgetSnapshot,
    automatic: bool,
    admitted: Option<AdoptedHistory>,
    must_cover: BTreeSet<EventId>,
) -> anyhow::Result<Option<Sources>> {
    let run_id = input.request.agent_run_id;
    let store = Arc::clone(&app.store);
    store
        .run_critical(
            StoreStep::run(run_id, "agent.context.compaction_select"),
            StoreAccess::Read,
            move || {
                let journal = app.execution.journal.lock()?;
                let db = journal.database.connection();
    let run = aura_persistence_v2::agent::turns::load_run(db, input.request.agent_run_id)?;
    let turn = aura_persistence_v2::agent::turns::load(db, run.turn_task_id)?;
    let groups = aura_persistence_v2::context::history(db, input.request.session_id)?;
    let previous = input.inspector.compaction.source_context_snapshot_id
        .map(|id| store::load(db, id)).transpose()?;
    // History a branch carried keeps its place from the conversation it came from, which
    // may no longer exist to ask.
    let carried =
        aura_persistence_v2::agent::branch::history_sequences(db, input.request.session_id)?;
    let cutoff = admitted.as_ref().map_or(input.request.history_cutoff, |history| history.cutoff);
    let frozen = admitted.as_ref().and_then(|history| history.items_for_read_gap.as_ref());
    let history = frozen.unwrap_or(&input.recent_history_items);
    let mut units = vec![];
    let mut members: Vec<Vec<EventId>> = vec![];
    let (rate_tokens, rate_bytes) = planning_rate(&budget);
    let mut index = 0;
    while index < history.len() {
        let item = &history[index];
        let event = event_id(item);
        let group = groups.iter().find(|g| g.assistant_response_event_id == event);
        let count = match item {
            LogicalConversationItem::Assistant { tool_calls, .. } => 1 + tool_calls.len(),
            _ => 1,
        };
        anyhow::ensure!(
            index + count <= history.len(),
            "context.compaction_incomplete_group"
        );
        let values = &history[index..index + count];
        if let LogicalConversationItem::Assistant { tool_calls, .. } = item {
            for (call, result) in tool_calls.iter().zip(&values[1..]) {
                anyhow::ensure!(
                    matches!(result,LogicalConversationItem::ToolResult{tool_call_id,..} if *tool_call_id==call.tool_call_id),
                    "context.compaction_tool_pair"
                );
            }
            anyhow::ensure!(
                tool_calls.is_empty() || group.is_some(),
                "context.compaction_unsealed_group"
            );
        } else {
            anyhow::ensure!(
                !matches!(item, LogicalConversationItem::ToolResult { .. }),
                "context.compaction_orphan_result"
            );
        }
        let sequence: i64 = match carried.get(&event) {
            Some(sequence) => i64::try_from(*sequence)?,
            None => db.query_row(
                "SELECT sequence FROM events WHERE event_id=?1",
                [event.to_string()],
                |r| r.get(0),
            )?,
        };
        // A sealed assistant + all tool results is one indivisible planner unit.
        // Root user and latest continuation remain verbatim protocol facts.
        let sealed = event != turn.root_user_event_id
            && (index + count < history.len() || frozen.is_some());
        let unit_bytes = serde_json::to_vec(values)?.len() as u64;
        units.push(HistoryEventUnit {
            event_id: event,
            sequence: SessionSequence::new(sequence.try_into()?),
            turn_segment_id: turn.turn_task_id.to_string(),
            history_atomic_group_ref: group.map(|g| g.group_ref()).transpose()?,
            atomic_group_id: None,
            kind: if matches!(item, LogicalConversationItem::User { .. }) {
                HistoryEventKind::UserMessage
            } else {
                HistoryEventKind::AssistantMessage
            },
            estimated_tokens: unit_bytes.saturating_mul(rate_tokens).div_ceil(rate_bytes),
            sealed,
        });
        members.push(values.iter().map(event_id).collect::<Vec<_>>());
        index += count;
    }
    // The latest assistant/tool group is the continuation this request still
    // has to validate. A trailing addition is its own unsealed unit and must
    // not make that group eligible to compact.
    if frozen.is_none() {
        retain_current_continuation(&mut units);
        if let Some(admitted) = &admitted {
            (units, members) = units.into_iter().zip(members)
                .filter(|(_, ids)| ids.iter().all(|id| admitted.events.contains(id)))
                .unzip();
        }
    }
    // What no compaction removes — the fixed content and the unsealed current groups. When
    // that alone does not fit, Core does not compact: the user decides.
    let unsealed = units.iter().filter(|u| !u.sealed).map(|u| u.estimated_tokens).sum::<u64>();
    if automatic
        && non_history_tokens.saturating_add(unsealed) > budget.request_input_budget_tokens
    {
        return Ok(None);
    }
    let plan = plan_recent_history(
        input.request.runtime_effective_window_tokens, budget.request_input_budget_tokens,
        non_history_tokens, &units,
    )?;
    let earlier = previous.as_ref().and_then(|p| p.result.as_ref()).map(super::summary::text);
    let mut taken: BTreeSet<_> = plan.compact_event_ids.iter().copied().collect();
    taken.extend(units.iter().zip(&members).filter_map(|(unit, ids)| {
        (unit.sealed && ids.iter().any(|id| must_cover.contains(id))).then_some(unit.event_id)
    }));
    let selected = units
        .iter()
        .zip(&members)
        .filter(|(u, _)| taken.contains(&u.event_id))
        .flat_map(|(_, m)| m.iter().copied())
        .collect::<BTreeSet<_>>();
    if selected.is_empty() || !must_cover.is_subset(&selected) {
        return Ok(None);
    }
    // Core keeps every source it replaces; the model is given the earlier summary and the
    // history itself.
    let mut drafts = vec![];
    let mut contents = vec![];
    for item in history {
        if selected.contains(&event_id(item)) {
            let source_ref = ContextSourceRef::Event { event_id: event_id(item) };
            let content = serde_json::to_string(item)?;
            drafts.push(CompactionSourceDraft {
                kind: CompactionAnchorKind::Event,
                source_ref: source_ref.clone(),
                source_hash: conversation_item_anchor_hash(item)?,
                preview: content.chars().take(128).collect(),
            });
            contents.push(serde_json::to_value(item)?);
        }
    }
    if let Some(p) = &previous {
        let id = p
            .result_artifact_id
            .ok_or_else(|| anyhow::anyhow!("context.compaction_artifact_missing"))?;
        drafts.push(CompactionSourceDraft {
            kind: CompactionAnchorKind::Artifact,
            source_ref: ContextSourceRef::Artifact { artifact_id: id },
            source_hash: compaction_result_anchor_hash(
                p.result
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("context.compaction_result_missing"))?,
            )?,
            preview: "Previous verified compaction; follow its anchors to original sources".into(),
        });
    }
    let anchors = build_anchor_manifest(drafts)?;
    let source_span = CompactionSourceSpan {
        start_sequence: units
            .iter()
            .filter(|u| taken.contains(&u.event_id))
            .map(|u| u.sequence)
            .min()
            .ok_or_else(|| anyhow::anyhow!("context.compaction_empty"))?,
        end_sequence: cutoff,
        source_refs: anchors.anchors.iter().map(|a| a.source_ref.clone()).collect(),
    };
    let background_sources = super::dependencies::collect(&input, previous.as_ref())?;
    let mut excluded =
        previous.map_or_else(BTreeSet::new, |p| p.excluded_event_ids.into_iter().collect());
    excluded.extend(selected);
    let content = serde_json::to_string(
        &serde_json::json!({"earlier_summary": earlier, "history": contents}),
    )?;
    let record = store::Compaction {
        id: ContextProjectionSnapshotId::new(),
        session_id: input.request.session_id,
        agent_run_id: input.request.agent_run_id,
        source_step_id: input.request.model_step_id,
        sequence: store::next_sequence(db, input.request.session_id)?,
        revision: Revision::INITIAL,
        state: "running".into(),
        source_span,
        anchors,
        source_artifact_id: ArtifactId::new(),
        source_hash: ContentHash::sha256(content.as_bytes()),
        excluded_event_ids: excluded.into_iter().collect(),
        background_sources,
        retained_recent_tokens: plan.retained_recent_tokens,
        retention_exceptions: plan.retention_exceptions,
        attempts: vec![],
        result: None,
        result_artifact_id: None,
        created_at: UtcTimestamp::now(),
    };
    Ok(Some(Sources { record, content }))
            },
        )
        .await
}

pub(super) fn retain_current_continuation(units: &mut [HistoryEventUnit]) {
    if let Some(index) =
        units.iter().rposition(|unit| unit.kind == HistoryEventKind::AssistantMessage)
    {
        units[index].sealed = false;
    }
}

#[cfg(test)]
fn continuation_unit(
    sequence: u64,
    kind: HistoryEventKind,
    tokens: u64,
    sealed: bool,
) -> HistoryEventUnit {
    HistoryEventUnit {
        event_id: EventId::new(),
        sequence: SessionSequence::new(sequence),
        turn_segment_id: "segment".to_owned(),
        history_atomic_group_ref: None,
        atomic_group_id: None,
        kind,
        estimated_tokens: tokens,
        sealed,
    }
}

#[cfg(test)]
#[test]
fn a_trailing_addition_keeps_the_current_assistant_group() {
    let root = continuation_unit(1, HistoryEventKind::UserMessage, 10, false);
    let earlier = continuation_unit(2, HistoryEventKind::UserMessage, 100, true);
    let assistant = continuation_unit(3, HistoryEventKind::AssistantMessage, 100, true);
    let addition = continuation_unit(4, HistoryEventKind::UserMessage, 10, false);
    let mut units = vec![root.clone(), earlier.clone(), assistant.clone(), addition.clone()];
    retain_current_continuation(&mut units);
    assert!(!units[2].sealed, "the current assistant/tool group stays");
    assert!(units[1].sealed, "an earlier sealed unit can still compact");
    let plan = plan_recent_history(100, 100, 0, &units).expect("plan");
    assert!(plan.compact_event_ids.contains(&earlier.event_id));
    assert!(!plan.compact_event_ids.contains(&assistant.event_id));
    assert!(!plan.compact_event_ids.contains(&addition.event_id));
    assert!(!plan.compact_event_ids.contains(&root.event_id));
}
