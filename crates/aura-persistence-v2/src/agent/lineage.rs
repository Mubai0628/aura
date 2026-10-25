use super::{facts, session};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, Transaction};

pub fn load(db: &Connection, id: TaskLineageId) -> StoreResult<TaskLineage> {
    let lineage: TaskLineage =
        facts::latest(db, &id.to_string(), "agent.lineage_state")?.ok_or(Error::Conflict)?;
    if lineage.lineage_id != id || lineage.revision.value() == 0 {
        return Err(Error::Conflict);
    }
    Ok(lineage)
}
pub fn insert(tx: &Transaction<'_>, lineage: &TaskLineage) -> StoreResult<()> {
    if facts::latest::<TaskLineage>(tx, &lineage.lineage_id.to_string(), "agent.lineage_state")?
        .is_some()
        || lineage.revision != Revision::INITIAL
        || lineage.state != TaskLineageState::Active
        || lineage.sources.len() != 1
        || lineage.sources[0].user_event_id != lineage.root_user_event_id
    {
        return Err(Error::Conflict);
    }
    check_sources(tx, lineage)?;
    facts::append(
        tx,
        EventId::new(),
        &lineage.lineage_id.to_string(),
        "agent.lineage_state",
        lineage,
        lineage.updated_at,
    )
}
pub fn save(tx: &Transaction<'_>, previous: &TaskLineage, next: &TaskLineage) -> StoreResult<()> {
    if load(tx, previous.lineage_id)? != *previous
        || matches!(
            previous.state,
            TaskLineageState::Completed
                | TaskLineageState::CancelledByUser
                | TaskLineageState::Expired
        )
        || (next.state == TaskLineageState::Active && !previous.state.may_activate())
        || previous.lineage_id != next.lineage_id
        || previous.session_id != next.session_id
        || previous.root_user_event_id != next.root_user_event_id
        || previous.created_by_turn_task_id != next.created_by_turn_task_id
        || previous.created_at != next.created_at
        || next.updated_at < previous.updated_at
        || previous.relations.iter().any(|old| !next.relations.contains(old))
        || next.revision.value()
            != previous.revision.value().checked_add(1).ok_or(Error::Conflict)?
        || previous.sources.iter().any(|old| {
            !next.sources.iter().any(|new| {
                new.user_event_id == old.user_event_id
                    && new.content_hash == old.content_hash
                    && new.immutable_preview == old.immutable_preview
                    && new.source_sequence == old.source_sequence
                    && new.related_ranges == old.related_ranges
                    && new.attached_by_turn_task_id == old.attached_by_turn_task_id
                    && new.attached_at == old.attached_at
            })
        })
    {
        return Err(Error::Conflict);
    }
    check_sources(tx, next)?;
    facts::append(
        tx,
        EventId::new(),
        &next.lineage_id.to_string(),
        "agent.lineage_state",
        next,
        next.updated_at,
    )
}
fn check_sources(db: &Connection, lineage: &TaskLineage) -> StoreResult<()> {
    let mut seen = std::collections::BTreeSet::new();
    for (index, source) in lineage.sources.iter().enumerate() {
        let message = session::user_message(db, lineage.session_id, source.user_event_id)?;
        if !seen.insert(source.user_event_id)
            || source.source_sequence as usize != index + 1
            || message.content_hash != source.content_hash
        {
            return Err(Error::Conflict);
        }
        let mut end = 0;
        let mut excerpts = Vec::new();
        for range in &source.related_ranges {
            let start = usize::try_from(range.start_utf8_byte).map_err(|_| Error::Conflict)?;
            let next = usize::try_from(range.end_utf8_byte).map_err(|_| Error::Conflict)?;
            if start < end || start >= next {
                return Err(Error::Conflict);
            }
            excerpts.push(message.text.get(start..next).ok_or(Error::Conflict)?);
            end = next;
        }
        let selected = if excerpts.is_empty() { message.text.clone() } else { excerpts.join("\n") };
        if source.immutable_preview != source_preview(&selected) {
            return Err(Error::Conflict);
        }
    }
    let mut relation_ids = std::collections::BTreeSet::new();
    for relation in &lineage.relations {
        if !relation_ids.insert(relation.relation_id)
            || !seen.contains(&relation.later_user_event_id)
            || relation.earlier_user_event_id.is_some_and(|id| !seen.contains(&id))
        {
            return Err(Error::Conflict);
        }
        for (id, range) in [
            (relation.earlier_user_event_id, relation.earlier_excerpt.as_ref()),
            (Some(relation.later_user_event_id), relation.later_excerpt.as_ref()),
        ] {
            if let Some(range) = range {
                let message =
                    session::user_message(db, lineage.session_id, id.ok_or(Error::Conflict)?)?;
                let start = usize::try_from(range.start_utf8_byte).map_err(|_| Error::Conflict)?;
                let end = usize::try_from(range.end_utf8_byte).map_err(|_| Error::Conflict)?;
                if start >= end || message.text.get(start..end).is_none() {
                    return Err(Error::Conflict);
                }
            }
        }
    }
    if let CompletionPolicy::StrictCompletionRequired { source_event_ids } =
        &lineage.completion_policy
        && (source_event_ids.is_empty() || source_event_ids.iter().any(|id| !seen.contains(id)))
    {
        return Err(Error::Conflict);
    }
    Ok(())
}
pub fn save_receipt(tx: &Transaction<'_>, receipt: &SourceReadReceipt) -> StoreResult<()> {
    let current = load(tx, receipt.lineage_id)?;
    if current.revision != receipt.lineage_revision || current.session_id != receipt.session_id {
        return Err(Error::Conflict);
    }
    if !session::load(tx, current.session_id)?.open_lineages.contains(&current.lineage_id) {
        return Err(Error::Conflict);
    }
    let turn = active_step(tx, receipt.model_step_id, receipt.turn_task_id, receipt.agent_run_id)?;
    let messages = receipt
        .source_event_hashes
        .iter()
        .map(|(id, _)| session::user_message(tx, current.session_id, *id))
        .collect::<StoreResult<Vec<_>>>()?;
    let expected = current
        .source_read_receipt(
            &messages,
            &turn,
            receipt.model_step_id,
            receipt.receipt_id,
            receipt.created_at,
        )
        .map_err(|_| Error::Conflict)?;
    if expected != *receipt {
        return Err(Error::Conflict);
    }
    facts::append(
        tx,
        EventId::from_bytes(receipt.receipt_id.to_bytes()),
        &receipt.lineage_id.to_string(),
        "agent.source_read",
        receipt,
        receipt.created_at,
    )
}
pub fn receipt(
    db: &Connection,
    lineage: TaskLineageId,
    id: SourceReadReceiptId,
) -> StoreResult<SourceReadReceipt> {
    let receipt: SourceReadReceipt = facts::exact(
        db,
        EventId::from_bytes(id.to_bytes()),
        &lineage.to_string(),
        "agent.source_read",
    )?;
    if receipt.receipt_id != id || receipt.lineage_id != lineage {
        return Err(Error::Conflict);
    }
    Ok(receipt)
}
pub fn save_review(tx: &Transaction<'_>, review: &CompletionReview) -> StoreResult<()> {
    let current = load(tx, review.lineage_id)?;
    let turn = active_step(tx, review.model_step_id, review.turn_task_id, review.agent_run_id)?;
    if current.revision != review.lineage_revision
        || turn.lineage_id != current.lineage_id
        || !matches!(current.completion_gate, CompletionGate::ReviewedReady { review_id, lineage_revision, .. } | CompletionGate::ReviewedNotReady { review_id, lineage_revision } if review_id == review.review_id && lineage_revision == review.lineage_revision)
    {
        return Err(Error::Conflict);
    }
    super::evidence::verify(
        tx,
        review
            .input
            .evidence_refs
            .iter()
            .chain(review.input.reviewed_user_intents.iter().flat_map(|i| &i.evidence_refs)),
        &review.agent_run_id.to_string(),
    )?;
    facts::append(
        tx,
        EventId::from_bytes(review.review_id.to_bytes()),
        &review.lineage_id.to_string(),
        "agent.completion_review",
        review,
        review.created_at,
    )
}
pub fn review(
    db: &Connection,
    lineage: TaskLineageId,
    id: CompletionReviewId,
) -> StoreResult<CompletionReview> {
    let review: CompletionReview = facts::exact(
        db,
        EventId::from_bytes(id.to_bytes()),
        &lineage.to_string(),
        "agent.completion_review",
    )?;
    if review.review_id != id || review.lineage_id != lineage {
        return Err(Error::Conflict);
    }
    Ok(review)
}
fn active_step(
    db: &Connection,
    step: ModelStepId,
    turn: TurnTaskId,
    run: AgentRunId,
) -> StoreResult<TurnTask> {
    let step = super::turns::step(db, step)?;
    let current = super::turns::load(db, turn)?;
    let running = super::turns::load_run(db, run)?;
    if step.agent_run_id != run
        || current.agent_run_id != run
        || running.turn_task_id != turn
        || running.parent_agent_run_id.is_some()
        || current.state.terminal()
        || !matches!(step.state, ModelStepState::Completed { tool_batch_id: Some(batch), .. } if running.state == AgentRunState::ToolsRunning { tool_batch_id: batch })
    {
        return Err(Error::Conflict);
    }
    Ok(current)
}
