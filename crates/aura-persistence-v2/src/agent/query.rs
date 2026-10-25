//! Real user answers resume the waiting TurnTask. No new task or synthetic
//! user submission is created by Agent continuation.
use super::{application, facts, session, turns, work};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, Transaction};

mod deadline;
pub use deadline::{deadline, expire, restart_countdown, run_deadline, touch, wait_deadline};

pub fn answered(
    db: &Connection,
    command: CommandInvocationId,
    input: &AgentQueryAnswerRequest,
) -> StoreResult<bool> {
    let previous: Option<AgentQueryAnswerRequest> =
        facts::latest(db, &command.to_string(), "agent.query_answer_command")?;
    match previous {
        Some(v) if v == *input => Ok(true),
        Some(_) => Err(Error::Conflict),
        None => Ok(false),
    }
}

pub fn selected_file(
    db: &Connection,
    command: CommandInvocationId,
    input: &AgentQueryFileRequest,
) -> StoreResult<Option<AgentQueryFileSelection>> {
    let previous: Option<(AgentQueryFileRequest, AgentQueryFileSelection)> =
        facts::latest(db, &command.to_string(), "agent.query_file_command")?;
    match previous {
        Some((request, selected)) if request == *input => Ok(Some(selected)),
        Some(_) => Err(Error::Conflict),
        None => Ok(None),
    }
}
pub fn select_file(
    tx: &Transaction<'_>,
    command: CommandInvocationId,
    input: &AgentQueryFileRequest,
    selected: &AgentQueryFileSelection,
    at: UtcTimestamp,
) -> StoreResult<AgentQueryFileSelection> {
    if let Some(previous) = selected_file(tx, command, input)? {
        return Ok(previous);
    }
    let turn = turns::load(tx, turns::load_run(tx, input.agent_run_id)?.turn_task_id)?;
    let query = work::query(tx, turn.turn_task_id)?.ok_or(Error::Conflict)?;
    if turn.session_id != input.session_id
        || query.query_id != input.query_id
        || query.revision != input.expected_revision
        || query.state != UserQueryState::Pending
    {
        return Err(Error::Conflict);
    }
    facts::append(
        tx,
        EventId::new(),
        &selected.file_reference_id.to_string(),
        "agent.query_file",
        &(input.clone(), selected.clone()),
        at,
    )?;
    facts::append(
        tx,
        EventId::new(),
        &command.to_string(),
        "agent.query_file_command",
        &(input.clone(), selected.clone()),
        at,
    )?;
    Ok(selected.clone())
}
pub fn answer(
    tx: &Transaction<'_>,
    command: CommandInvocationId,
    input: &AgentQueryAnswerRequest,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if answered(tx, command, input)? {
        return Ok(());
    }
    let run = turns::load_run(tx, input.agent_run_id)?;
    let turn = turns::load(tx, run.turn_task_id)?;
    let old = work::query(tx, turn.turn_task_id)?.ok_or(Error::Conflict)?;
    if turn.session_id != input.session_id
        || application::cancellation_requested(tx, run.agent_run_id)?
        || old.query_id != input.query_id
        || old.revision != input.expected_revision
        // "no response" is Core's to record when the question's time runs out.
        || input.answers.iter().any(|answer| answer.value == QueryAnswerValue::NoResponse)
    {
        return Err(Error::Conflict);
    }
    resolve(tx, &run, &turn, &old, &input.answers, at)?;
    facts::append(tx, EventId::new(), &command.to_string(), "agent.query_answer_command", input, at)
}

/// Ends a waiting question with these answers and lets its run go on.
fn resolve(
    tx: &Transaction<'_>,
    run: &AgentRun,
    turn: &TurnTask,
    old: &UserQuery,
    answers: &[QueryAnswer],
    at: UtcTimestamp,
) -> StoreResult<()> {
    if turn.state != (TurnTaskState::AwaitingQuery { query_id: old.query_id })
        || run.state != (AgentRunState::AwaitingQuery { query_id: old.query_id })
        || old.state != UserQueryState::Pending
    {
        return Err(Error::Conflict);
    }
    old.input.validate_answers(answers).map_err(|_| Error::Invalid("query answers"))?;
    // Core resolves FileReference ownership through the Workspace service.
    // This record carries no additional file-access grant.
    let mut files = std::collections::BTreeMap::new();
    for answer in answers {
        if let QueryAnswerValue::FileOrDirectory { file_reference_id } = answer.value
            && let Some((selection, file)) =
                facts::latest::<(AgentQueryFileRequest, AgentQueryFileSelection)>(
                    tx,
                    &file_reference_id.to_string(),
                    "agent.query_file",
                )?
        {
            if selection.session_id != turn.session_id
                || selection.query_id != old.query_id
                || selection.question_id != answer.question_id
            {
                return Err(Error::Conflict);
            }
            files.insert(answer.question_id.clone(), file.relative_path);
        }
    }
    // The model reads the questions and the chosen labels, and how the question ended.
    let text = query_reply(old.query_id, &old.input, answers, &files);
    if text.len() > 256 * 1024 {
        return Err(Error::Invalid("query answer size"));
    }
    let user = OriginalUserMessage::new(
        EventId::new(),
        turn.session_id,
        text,
        UserMessageOrigin::QueryAnswer { query_id: old.query_id },
        at,
    )
    .map_err(|_| Error::Conflict)?;
    session::add_user_message(tx, &user)?;
    facts::append(
        tx,
        EventId::new(),
        &turn.turn_task_id.to_string(),
        "agent.query_answer_event",
        &user.event_id,
        at,
    )?;
    let mut next = old.clone();
    next.revision = Revision::new(old.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    next.state =
        UserQueryState::Answered { user_event_id: user.event_id, answers: answers.to_vec() };
    work::save_query(tx, Some(old), &next, at)?;
    let original = super::lineage::load(tx, turn.lineage_id)?;
    let mut changed = original.clone();
    changed.source_recheck.used_query = true;
    changed.revision =
        Revision::new(original.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    changed.updated_at = at;
    changed.completion_gate =
        CompletionGate::Invalidated { reason: CompletionInvalidation::QueryAnswerAttached };
    super::lineage::save(tx, &original, &changed)?;
    let mut t = turn.clone();
    t.transition(turn.revision, TurnTaskState::Running, at).map_err(|_| Error::Conflict)?;
    turns::save(tx, turn, &t)?;
    let mut r = run.clone();
    r.state = AgentRunState::PreparingContext;
    r.revision = Revision::new(run.revision.value() + 1);
    r.updated_at = at;
    turns::save_run(tx, run, &r)
}
pub fn events(db: &Connection, turn: TurnTaskId) -> StoreResult<Vec<EventId>> {
    let mut q = db.prepare("SELECT payload FROM events WHERE owner=?1 AND kind='agent.query_answer_event' ORDER BY sequence")?;
    q.query_map([turn.to_string()], |r| r.get::<_, String>(0))?
        .map(|v| Ok(serde_json::from_str(&v?)?))
        .collect()
}
