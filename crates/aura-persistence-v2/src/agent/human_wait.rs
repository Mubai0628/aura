//! The durable deadline of a question or approval displayed to the user.
use super::{application, approval, facts, query, turns, work};
use aura_agent_domain::*;
use aura_domain_common::human_wait::{HumanWait, valid_viewer};
use aura_domain_common::*;
use aura_execution_contract::ExecutionApprovalRequest;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
use aura_security_contract::{ApprovalState, AuthorizationApproval};
use rusqlite::{Connection, Transaction, params};

const EVENT: &str = "agent.human_wait";

fn load(db: &Connection, id: &str) -> StoreResult<Option<HumanWait>> {
    facts::latest(db, id, EVENT)
}

pub fn recorded_deadline(db: &Connection, id: &str) -> StoreResult<Option<UtcTimestamp>> {
    Ok(load(db, id)?.map(|w| w.deadline))
}

pub fn approval_presented(db: &Connection, id: ApprovalRequestId) -> StoreResult<bool> {
    Ok(load(db, &id.to_string())?.is_some())
}

pub fn activity(
    tx: &Transaction<'_>,
    input: &AgentWaitActivityRequest,
    at: UtcTimestamp,
) -> StoreResult<(AgentWaitActivityView, Option<AgentRunId>)> {
    if !valid_viewer(&input.viewer) {
        return Err(Error::Invalid("agent.wait.invalid_viewer"));
    }
    let (run, id, current, mut approved) = match input.target {
        AgentWaitTarget::Query { agent_run_id, query_id } => {
            let run = turns::load_run(tx, agent_run_id)?;
            let turn = turns::load(tx, run.turn_task_id)?;
            let question = work::query(tx, run.turn_task_id)?.ok_or(Error::Conflict)?;
            if turn.session_id != input.session_id
                || question.query_id != query_id
                || run.state != (AgentRunState::AwaitingQuery { query_id })
            {
                return Err(Error::Conflict);
            }
            (agent_run_id, query_id.to_string(), query::deadline(tx, &question)?, None)
        }
        AgentWaitTarget::Approval { approval_id } => {
            let requested = approval::view(
                tx,
                &ExecutionApprovalRequest { session_id: input.session_id, approval_id },
            )?;
            let run = requested.operation.content.identity.agent_run_id.ok_or(Error::Conflict)?;
            let state = turns::load_run(tx, run)?.state;
            if requested.approval.state != ApprovalState::Pending
                || !matches!(state, AgentRunState::AwaitingApproval { .. })
            {
                return Err(Error::Conflict);
            }
            (
                run,
                approval_id.to_string(),
                Some(requested.approval.expires_at),
                Some(requested.approval),
            )
        }
        AgentWaitTarget::LegacyApproval { .. } => return Err(Error::Conflict),
    };
    if current.is_some_and(|deadline| deadline <= at)
        || application::cancellation_requested(tx, run)?
    {
        return Err(Error::Conflict);
    }
    let previous = load(tx, &id)?;
    let next = HumanWait::advance(previous.as_ref(), &input.viewer, input.activity, at)
        .ok_or(Error::Conflict)?;
    if previous.as_ref() != Some(&next) {
        facts::append(tx, EventId::new(), &id, EVENT, &next, at)?;
        if let Some(approval) = approved.as_mut() {
            update_approval_deadline(tx, approval, next.deadline)?;
        }
    }
    Ok((
        AgentWaitActivityView { deadline_at: next.deadline, focused_until: next.focused_until(at) },
        current.is_none_or(|deadline| next.deadline < deadline).then_some(run),
    ))
}

fn update_approval_deadline(
    tx: &Transaction<'_>,
    approval: &mut AuthorizationApproval,
    deadline: UtcTimestamp,
) -> StoreResult<()> {
    // Timing updates leave the authorization choice's CAS revision intact.
    approval.expires_at = deadline;
    record::changed(tx.execute(
        "UPDATE approval_requests SET payload=?3 WHERE id=?1 AND revision=?2",
        params![
            approval.approval_id.to_string(),
            record::sql_u64(approval.revision.value())?,
            record::encode(approval)?,
        ],
    )?)
}
