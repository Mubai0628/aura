//! Applies a model's semantic decision to authenticated User Events.
//! Interrupted and topic-switched lineages cannot become Active again.
use super::{facts, lineage, session, turns};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::Transaction;

fn revision(value: Revision) -> StoreResult<Revision> {
    Ok(Revision::new(value.value().checked_add(1).ok_or(Error::Conflict)?))
}
fn invalid(code: &'static str) -> Error {
    Error::Invalid(code)
}
pub fn apply(
    tx: &Transaction<'_>,
    turn_id: TurnTaskId,
    input: &TaskLineageReconcileInput,
    at: UtcTimestamp,
) -> StoreResult<TaskLineage> {
    let turn = turns::load(tx, turn_id)?;
    let run = turns::load_run(tx, turn.agent_run_id)?;
    let current = session::load(tx, turn.session_id)?;
    if turn.state.terminal()
        || run.parent_agent_run_id.is_some()
        || current.active_turn != Some(turn_id)
        || input.reason.trim().is_empty()
        || (input.current_user_event_id != turn.root_user_event_id
            && !current.pending_user_event_ids.contains(&input.current_user_event_id))
    {
        return Err(invalid("task_lineage.identity_mismatch"));
    }
    match &input.decision {
        TaskLineageReconcileDecision::ResumeSuspended { .. } => {
            Err(invalid("task_lineage.disposition_not_allowed"))
        }
        TaskLineageReconcileDecision::ExpirePrior { lineage_id } => {
            expire(tx, &turn, &current, input, *lineage_id, at)
        }
        TaskLineageReconcileDecision::ReferencePrior { lineage_id } => {
            reference(tx, &turn, &current, input, *lineage_id, at)
        }
        TaskLineageReconcileDecision::SuspendAndStartNew => {
            switch_topic(tx, &turn, &current, input, at)
        }
        _ => apply_current(tx, &turn, &current, input, at),
    }
}

fn expire(
    tx: &Transaction<'_>,
    turn: &TurnTask,
    current: &super::session::AgentSessionState,
    input: &TaskLineageReconcileInput,
    target: TaskLineageId,
    at: UtcTimestamp,
) -> StoreResult<TaskLineage> {
    if input.completion_policy_update.is_some() {
        return Err(invalid("task_lineage.constraint.expire_forbids_policy"));
    }
    if !input.relations.is_empty() {
        return Err(invalid("task_lineage.constraint.expire_forbids_relations"));
    }
    if Some(target) == current.active_lineage || target == turn.lineage_id {
        return Err(invalid("task_lineage.disposition_not_allowed"));
    }
    let old = lineage::load(tx, target)?;
    if old.session_id != turn.session_id {
        return Err(invalid("task_lineage.target_not_in_session"));
    }
    if Some(old.revision) != input.expected_lineage_revision {
        return Err(invalid("task_lineage.revision_stale"));
    }
    if !matches!(
        old.state,
        TaskLineageState::InterruptedResumable | TaskLineageState::SuspendedByTopicSwitch
    ) {
        return Err(invalid("task_lineage.disposition_not_allowed"));
    }
    let mut next = old.clone();
    next.state = TaskLineageState::Expired;
    next.advance_revision(old.revision, at).map_err(|_| invalid("task_lineage.revision_stale"))?;
    lineage::save(tx, &old, &next)?;
    let mut state = current.clone();
    state.open_lineages.retain(|id| *id != target);
    state.revision = revision(current.revision)?;
    session::save(tx, current, &state, at)?;
    record(tx, turn.turn_task_id, input, at)?;
    Ok(next)
}

fn reference(
    tx: &Transaction<'_>,
    turn: &TurnTask,
    current: &super::session::AgentSessionState,
    input: &TaskLineageReconcileInput,
    target: TaskLineageId,
    at: UtcTimestamp,
) -> StoreResult<TaskLineage> {
    if input.completion_policy_update.is_some() {
        return Err(invalid("task_lineage.constraint.reference_forbids_policy"));
    }
    let active_id = current.active_lineage.ok_or(invalid("task_lineage.current_required"))?;
    if active_id == target || turn.lineage_resolution_required {
        return Err(invalid("task_lineage.disposition_not_allowed"));
    }
    let prior = lineage::load(tx, target)?;
    if prior.session_id != turn.session_id {
        return Err(invalid("task_lineage.target_not_in_session"));
    }
    if prior.state.may_activate() && Some(target) == current.active_lineage {
        return Err(invalid("task_lineage.disposition_not_allowed"));
    }
    let current_lineage = lineage::load(tx, active_id)?;
    if Some(current_lineage.revision) != input.expected_lineage_revision {
        return Err(invalid("task_lineage.revision_stale"));
    }
    let user = session::user_message(tx, turn.session_id, input.current_user_event_id)?;
    // The link to the prior task is the reference
    // itself, `derived_from_lineage_id`. A relation from the current message to the prior task's
    // own message says the same, so it is covered by it; a task's stored relations stay among its
    // own messages.
    let covered = |r: &TaskLineageRelationInput| {
        r.later_user_event_id == user.event_id
            && r.earlier_user_event_id
                .is_some_and(|id| prior.sources.iter().any(|s| s.user_event_id == id))
    };
    let within = TaskLineageReconcileInput {
        relations: input.relations.iter().filter(|r| !covered(r)).cloned().collect(),
        ..input.clone()
    };
    let relations = resolve_relations(tx, &current_lineage, &user, turn, &within, at)?;
    let mut selected = current_lineage.clone();
    if selected.derived_from_lineage_id.is_none() {
        selected.derived_from_lineage_id = Some(target);
    }
    selected
        .advance_revision(current_lineage.revision, at)
        .map_err(|_| invalid("task_lineage.revision_stale"))?;
    selected.relations.extend(relations);
    lineage::save(tx, &current_lineage, &selected)?;
    record(tx, turn.turn_task_id, input, at)?;
    Ok(selected)
}

fn switch_topic(
    tx: &Transaction<'_>,
    turn: &TurnTask,
    current: &super::session::AgentSessionState,
    input: &TaskLineageReconcileInput,
    at: UtcTimestamp,
) -> StoreResult<TaskLineage> {
    // Idle starts already own an Active lineage. Keep it, and leave any prior
    // interrupted record untouched and non-Active.
    if input.completion_policy_update.is_some() {
        return Err(invalid("task_lineage.constraint.expire_forbids_policy"));
    }
    if !turn.lineage_resolution_required {
        let active_id = current.active_lineage.ok_or(invalid("task_lineage.current_required"))?;
        let selected = lineage::load(tx, active_id)?;
        if Some(selected.revision) != input.expected_lineage_revision {
            return Err(invalid("task_lineage.revision_stale"));
        }
        record(tx, turn.turn_task_id, input, at)?;
        return Ok(selected);
    }
    let target = input.active_lineage_id.ok_or(invalid("task_lineage.identity_mismatch"))?;
    if !current.open_lineages.contains(&target) {
        return Err(invalid("task_lineage.target_not_in_session"));
    }
    let old = lineage::load(tx, target)?;
    if Some(old.revision) != input.expected_lineage_revision {
        return Err(invalid("task_lineage.revision_stale"));
    }
    if old.state.may_activate() {
        let mut suspended = old.clone();
        suspended.state = TaskLineageState::SuspendedByTopicSwitch;
        suspended
            .advance_revision(old.revision, at)
            .map_err(|_| invalid("task_lineage.revision_stale"))?;
        lineage::save(tx, &old, &suspended)?;
    }
    let user = session::user_message(tx, turn.session_id, input.current_user_event_id)?;
    let new = TaskLineage::from_root(turn.lineage_id, &user, turn.turn_task_id)
        .map_err(|_| invalid("task_lineage.arguments_invalid"))?;
    lineage::insert(tx, &new)?;
    finish_turn(tx, turn, current, &new, at)?;
    record(tx, turn.turn_task_id, input, at)?;
    Ok(new)
}

fn apply_current(
    tx: &Transaction<'_>,
    turn: &TurnTask,
    current: &super::session::AgentSessionState,
    input: &TaskLineageReconcileInput,
    at: UtcTimestamp,
) -> StoreResult<TaskLineage> {
    let target = current.active_lineage.ok_or(invalid("task_lineage.current_required"))?;
    if input.active_lineage_id.is_some_and(|id| id != target) {
        return Err(invalid("task_lineage.identity_mismatch"));
    }
    let old = lineage::load(tx, target)?;
    if old.session_id != turn.session_id {
        return Err(invalid("task_lineage.identity_mismatch"));
    }
    if Some(old.revision) != input.expected_lineage_revision {
        return Err(invalid("task_lineage.revision_stale"));
    }
    if !old.state.may_activate() {
        return Err(invalid("task_lineage.disposition_not_allowed"));
    }
    let user = session::user_message(tx, turn.session_id, input.current_user_event_id)?;
    let relations = resolve_relations(tx, &old, &user, turn, input, at)?;
    let mixed = input.decision == TaskLineageReconcileDecision::Mixed;
    let (related, unrelated) = mixed_ranges(&user, &relations, mixed)?;
    let mut selected = old.clone();
    if !selected.sources.iter().any(|s| s.user_event_id == user.event_id) {
        selected
            .attach_source(&user, turn.turn_task_id, related, old.revision, at)
            .map_err(|_| invalid("task_lineage.arguments_invalid"))?;
    } else {
        selected
            .advance_revision(old.revision, at)
            .map_err(|_| invalid("task_lineage.revision_stale"))?;
    }
    selected.source_recheck.crossed_turn |= old.last_active_turn_task_id != turn.turn_task_id;
    selected.last_active_turn_task_id = turn.turn_task_id;
    selected.relations.extend(relations);
    selected.completion_gate =
        CompletionGate::Invalidated { reason: CompletionInvalidation::RelationChanged };
    apply_policy(&mut selected, &user, input)?;
    if input.decision == TaskLineageReconcileDecision::CancelExisting {
        if !selected.relations.iter().any(|r| {
            r.later_user_event_id == user.event_id
                && r.kind == TaskLineageRelationKind::CancelsTask
                && r.later_excerpt.is_some()
        }) {
            return Err(invalid("task_lineage.constraint.cancel_requires_excerpt"));
        }
        selected.state = TaskLineageState::CancelledByUser;
    }
    lineage::save(tx, &old, &selected)?;
    let mut state = current.clone();
    if !unrelated.is_empty() {
        let mut sibling = TaskLineage::from_root(TaskLineageId::new(), &user, turn.turn_task_id)
            .map_err(|_| invalid("task_lineage.arguments_invalid"))?;
        sibling.sources[0].related_ranges = unrelated.clone();
        sibling.sources[0].immutable_preview = source_preview(&range_text(&user.text, &unrelated)?);
        sibling.derived_from_lineage_id = Some(selected.lineage_id);
        lineage::insert(tx, &sibling)?;
        let mut suspended = sibling.clone();
        suspended.state = TaskLineageState::SuspendedByTopicSwitch;
        suspended
            .advance_revision(sibling.revision, at)
            .map_err(|_| invalid("task_lineage.revision_stale"))?;
        lineage::save(tx, &sibling, &suspended)?;
        state.open_lineages.push(sibling.lineage_id);
    }
    let mut changed_turn = turn.clone();
    changed_turn.lineage_id = selected.lineage_id;
    changed_turn.lineage_resolution_required = false;
    changed_turn.revision = revision(turn.revision)?;
    changed_turn.updated_at = at;
    if selected.state == TaskLineageState::CancelledByUser {
        crate::context::manual_compaction::cancel_run(
            tx,
            turn.agent_run_id,
            "context.task_cancelled_before_compaction",
            at,
        )?;
        changed_turn.state = TurnTaskState::Cancelled;
        state.open_lineages.retain(|id| *id != selected.lineage_id);
        state.active_lineage = None;
        state.active_turn = None;
    }
    state.pending_user_event_ids.retain(|id| *id != user.event_id);
    state.revision = revision(current.revision)?;
    turns::save(tx, turn, &changed_turn)?;
    session::save(tx, current, &state, at)?;
    if !changed_turn.state.terminal() {
        super::input_consumption::attach_consumed(tx, &changed_turn, at)?;
    }
    record(tx, turn.turn_task_id, input, at)?;
    Ok(selected)
}

fn finish_turn(
    tx: &Transaction<'_>,
    turn: &TurnTask,
    current: &super::session::AgentSessionState,
    result: &TaskLineage,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let mut changed_turn = turn.clone();
    changed_turn.lineage_id = result.lineage_id;
    changed_turn.lineage_resolution_required = false;
    changed_turn.revision = revision(turn.revision)?;
    changed_turn.updated_at = at;
    let mut state = current.clone();
    if !state.open_lineages.contains(&result.lineage_id) {
        state.open_lineages.push(result.lineage_id);
    }
    state.active_lineage = Some(result.lineage_id);
    state.pending_user_event_ids.retain(|id| *id != turn.root_user_event_id);
    state.revision = revision(current.revision)?;
    turns::save(tx, turn, &changed_turn)?;
    session::save(tx, current, &state, at)
}

fn record(
    tx: &Transaction<'_>,
    turn_id: TurnTaskId,
    input: &TaskLineageReconcileInput,
    at: UtcTimestamp,
) -> StoreResult<()> {
    facts::append(tx, EventId::new(), &turn_id.to_string(), "agent.lineage_reconciled", input, at)
}

fn resolve_relations(
    tx: &Transaction<'_>,
    old: &TaskLineage,
    user: &OriginalUserMessage,
    turn: &TurnTask,
    input: &TaskLineageReconcileInput,
    at: UtcTimestamp,
) -> StoreResult<Vec<TaskLineageRelation>> {
    let optional = matches!(
        input.decision,
        TaskLineageReconcileDecision::ReferencePrior { .. }
            | TaskLineageReconcileDecision::SuspendAndStartNew
    );
    if !optional && input.relations.is_empty() {
        return Err(invalid("task_lineage.arguments_invalid"));
    }
    input
        .relations
        .iter()
        .map(|r| {
            if r.later_user_event_id != user.event_id
                || r.kind == TaskLineageRelationKind::Root
                || r.earlier_user_event_id
                    .is_some_and(|id| !old.sources.iter().any(|s| s.user_event_id == id))
            {
                return Err(invalid("task_lineage.identity_mismatch"));
            }
            let earlier_excerpt = if let Some(selector) = &r.earlier_excerpt {
                let earlier = session::user_message(
                    tx,
                    user.session_id,
                    r.earlier_user_event_id.ok_or(invalid("task_lineage.identity_mismatch"))?,
                )?;
                Some(
                    selector
                        .resolve(&earlier.text)
                        .map_err(|_| invalid("task_lineage.arguments_invalid"))?,
                )
            } else {
                None
            };
            let later_excerpt = r
                .later_excerpt
                .as_ref()
                .map(|s| {
                    s.resolve(&user.text).map_err(|_| invalid("task_lineage.arguments_invalid"))
                })
                .transpose()?;
            Ok(TaskLineageRelation {
                relation_id: TaskLineageRelationId::new(),
                earlier_user_event_id: r.earlier_user_event_id,
                later_user_event_id: user.event_id,
                kind: r.kind,
                earlier_excerpt,
                later_excerpt,
                created_by_turn_task_id: turn.turn_task_id,
                created_at: at,
            })
        })
        .collect()
}
fn apply_policy(
    lineage: &mut TaskLineage,
    user: &OriginalUserMessage,
    input: &TaskLineageReconcileInput,
) -> StoreResult<()> {
    if let Some(change) = &input.completion_policy_update {
        if change.authority_event_id != user.event_id {
            return Err(invalid("task_lineage.identity_mismatch"));
        }
        change
            .authority_excerpt
            .resolve(&user.text)
            .map_err(|_| invalid("task_lineage.arguments_invalid"))?;
        if let CompletionPolicy::StrictCompletionRequired { source_event_ids } = &change.policy
            && (source_event_ids.is_empty()
                || !source_event_ids.contains(&user.event_id)
                || source_event_ids
                    .iter()
                    .any(|id| !lineage.sources.iter().any(|s| s.user_event_id == *id)))
        {
            return Err(invalid("task_lineage.arguments_invalid"));
        }
        lineage.completion_policy = change.policy.clone();
        lineage.completion_gate =
            CompletionGate::Invalidated { reason: CompletionInvalidation::CompletionPolicyChanged };
    }
    Ok(())
}
fn range_text(text: &str, ranges: &[UserMessageTextRange]) -> StoreResult<String> {
    ranges
        .iter()
        .map(|r| {
            text.get(r.start_utf8_byte as usize..r.end_utf8_byte as usize)
                .ok_or(invalid("task_lineage.arguments_invalid"))
        })
        .collect::<StoreResult<Vec<_>>>()
        .map(|v| v.join("\n"))
}
fn mixed_ranges(
    user: &OriginalUserMessage,
    relations: &[TaskLineageRelation],
    mixed: bool,
) -> StoreResult<(Vec<UserMessageTextRange>, Vec<UserMessageTextRange>)> {
    if !mixed {
        return Ok((vec![], vec![]));
    }
    let mut related = vec![];
    let mut unrelated = vec![];
    for relation in relations {
        let range =
            relation.later_excerpt.clone().ok_or(invalid("task_lineage.arguments_invalid"))?;
        if relation.kind == TaskLineageRelationKind::ReclassifiesUnrelatedPart {
            unrelated.push(range);
        } else {
            related.push(range);
        }
    }
    related.sort_by_key(|r| r.start_utf8_byte);
    related.dedup();
    unrelated.sort_by_key(|r| r.start_utf8_byte);
    unrelated.dedup();
    let mut all: Vec<_> = related.iter().chain(&unrelated).collect();
    all.sort_by_key(|r| r.start_utf8_byte);
    if related.is_empty()
        || all.windows(2).any(|p| p[0].end_utf8_byte > p[1].start_utf8_byte)
        || user.text.char_indices().any(|(i, c)| {
            !c.is_whitespace()
                && !all
                    .iter()
                    .any(|r| i as u64 >= r.start_utf8_byte && (i as u64) < r.end_utf8_byte)
        })
    {
        return Err(invalid("task_lineage.arguments_invalid"));
    }
    Ok((related, unrelated))
}
