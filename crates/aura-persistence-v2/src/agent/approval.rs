//! Waiting owns no execution lease. Security owns the proposal/Grant CAS;
//! Agent owns the link to the exact batch and its subsequent continuation.
use super::{application, facts, turns};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_execution_contract::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use aura_security_contract::*;
use rusqlite::{Connection, OptionalExtension, Transaction};

pub fn pending(db: &Connection, run: AgentRunId) -> StoreResult<Vec<ApprovalRequestId>> {
    let mut q=db.prepare("SELECT payload FROM events WHERE owner=?1 AND kind='agent.execution_approval' ORDER BY sequence")?;
    let views = q
        .query_map([run.to_string()], |r| r.get::<_, String>(0))?
        .map(|r| Ok(serde_json::from_str::<ExecutionApprovalView>(&r?)?))
        .collect::<StoreResult<Vec<_>>>()?;
    views
        .into_iter()
        .filter_map(|v| match crate::security_approval::load(db, v.approval.approval_id) {
            Ok(a) if a.state == ApprovalState::Pending => Some(Ok(a.approval_id)),
            Ok(_) => None,
            Err(e) => Some(Err(e)),
        })
        .collect()
}
/// Whether the call of `operation` in `run` waits for the user's answer to its approval.
pub fn awaits_answer(
    db: &Connection,
    run: AgentRunId,
    operation: SecurityOperationId,
) -> StoreResult<bool> {
    let mut q = db.prepare(
        "SELECT payload FROM events WHERE owner=?1 AND kind='agent.execution_approval' \
         ORDER BY sequence DESC",
    )?;
    let views = q
        .query_map([run.to_string()], |r| r.get::<_, String>(0))?
        .map(|r| Ok(serde_json::from_str::<ExecutionApprovalView>(&r?)?))
        .collect::<StoreResult<Vec<_>>>()?;
    for view in views.iter().filter(|view| view.operation.operation_id == operation) {
        if crate::security_approval::load(db, view.approval.approval_id)?.state
            == ApprovalState::Pending
        {
            return Ok(true);
        }
    }
    Ok(false)
}
pub fn view(
    db: &Connection,
    input: &ExecutionApprovalRequest,
) -> StoreResult<ExecutionApprovalView> {
    let a = crate::security_approval::load(db, input.approval_id)?;
    // An approval is an event of its run, one of the session's runs: looked up through
    // their owners in the index, not across the whole journal. Another session's approval is the
    // conflict it always was.
    let raw: String = db
        .query_row(
            "SELECT e.payload FROM turn_tasks t CROSS JOIN agent_runs ar CROSS JOIN events e \
             WHERE t.session_id=?2 AND ar.turn_task_id=t.id AND e.owner=ar.id \
             AND e.kind='agent.execution_approval' \
             AND json_extract(e.payload,'$.approval.approval_id')=?1",
            [input.approval_id.to_string(), input.session_id.to_string()],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(Error::Conflict)?;
    let mut v: ExecutionApprovalView = serde_json::from_str(&raw)?;
    if v.operation.content.identity.session_id != Some(input.session_id)
        || v.approval.scope != a.scope
    {
        return Err(Error::Conflict);
    }
    v.approval = a;
    Ok(v)
}
#[expect(
    clippy::too_many_arguments,
    reason = "One transaction binds exact run/batch/call, frozen authorization and its validity window"
)]
pub fn pause(
    tx: &Transaction<'_>,
    run_id: AgentRunId,
    batch: ToolBatchId,
    call: ToolCallId,
    proposal: AuthorizationProposal,
    backend: BackendDescriptor,
    at: UtcTimestamp,
    expires_at: UtcTimestamp,
) -> StoreResult<()> {
    let run = turns::load_run(tx, run_id)?;
    let turn = turns::load(tx, run.turn_task_id)?;
    if run.state != (AgentRunState::ToolsRunning { tool_batch_id: batch })
        || turn.state.terminal()
        || !pending(tx, run_id)?.is_empty()
    {
        return Err(Error::Conflict);
    }
    let admitted = crate::tool::admission::load(tx, batch)?.ok_or(Error::Conflict)?;
    let operation = admitted.operations.get(&call).ok_or(Error::Conflict)?.clone();
    let stored = crate::tool::execution::call(tx, call)?;
    if operation.fingerprint != proposal.scope.operation_fingerprint
        || stored.result.is_some()
        || stored.attempt_id.is_some()
        || !backend.supports_operation(&proposal.scope.backend_requirement, &operation)
    {
        return Err(Error::Conflict);
    }
    let approval = AuthorizationApproval {
        approval_id: ApprovalRequestId::new(),
        revision: Revision::INITIAL,
        scope: proposal.scope,
        allowed_decisions: proposal.allowed_decisions,
        state: ApprovalState::Pending,
        requested_at: at,
        expires_at,
    };
    crate::security::insert_approval(tx, &approval)?;
    let step_settings = crate::tool::execution::batch(tx, batch)
        .ok()
        .and_then(|stored| {
            application::step_tool_settings(tx, stored.batch.invocation_identity.model_step_id).ok()
        })
        .unwrap_or_else(|| admitted.settings.clone());
    let network_proxy_notice = super::network_proxy_notice::network_proxy_notice(
        &step_settings,
        &operation.content.action_kind,
        &operation.content.action,
    );
    facts::append(
        tx,
        EventId::new(),
        &run_id.to_string(),
        "agent.execution_approval",
        &ExecutionApprovalView {
            approval,
            operation,
            backend,
            reasons: proposal.reasons,
            requested_capabilities: proposal.requested_capabilities,
            mode: admitted.permission_snapshot.mode,
            native_session_consent: proposal.native_session_consent,
            network_proxy_notice,
        },
        at,
    )?;
    let mut t = turn.clone();
    t.transition(turn.revision, TurnTaskState::AwaitingApproval, at)
        .map_err(|_| Error::Conflict)?;
    turns::save(tx, &turn, &t)?;
    let mut r = run.clone();
    r.state = AgentRunState::AwaitingApproval { tool_batch_id: batch };
    r.revision = Revision::new(run.revision.value() + 1);
    r.updated_at = at;
    turns::save_run(tx, &run, &r)
}
pub fn resolved(
    db: &Connection,
    command: CommandInvocationId,
    input: &ExecutionApprovalResolutionRequest,
) -> StoreResult<bool> {
    let previous: Option<ExecutionApprovalResolutionRequest> =
        facts::latest(db, &command.to_string(), "agent.approval_command")?;
    match previous {
        Some(v) if v == *input => Ok(true),
        Some(_) => Err(Error::Conflict),
        None => Ok(false),
    }
}
pub fn resolve(
    tx: &Transaction<'_>,
    command: CommandInvocationId,
    input: &ExecutionApprovalResolutionRequest,
    session_permission_revision: u64,
    core_runtime_instance_id: RuntimeInstanceId,
    workspace_root: Option<&str>,
    at: UtcTimestamp,
) -> StoreResult<AgentRunId> {
    let v = view(
        tx,
        &ExecutionApprovalRequest { session_id: input.session_id, approval_id: input.approval_id },
    )?;
    let id = v.operation.content.identity.agent_run_id.ok_or(Error::Conflict)?;
    if resolved(tx, command, input)? {
        return Ok(id);
    }
    let run = turns::load_run(tx, id)?;
    let turn = turns::load(tx, run.turn_task_id)?;
    let AgentRunState::AwaitingApproval { tool_batch_id: batch } = run.state else {
        return Err(Error::Conflict);
    };
    if turn.state != TurnTaskState::AwaitingApproval || application::cancellation_requested(tx, id)?
    {
        return Err(Error::Conflict);
    }
    let admitted = crate::tool::admission::load(tx, batch)?.ok_or(Error::Conflict)?;
    let call =
        *admitted.operations.iter().find(|(_, o)| **o == v.operation).ok_or(Error::Conflict)?.0;
    // Core holds the live mode-authority read barrier across this transaction;
    // dispatch independently rechecks it and current source/file/credential facts.
    crate::security::validate_snapshot_binding(tx, &admitted.permission_snapshot)?;
    let grant = crate::security_approval::resolve(
        tx,
        input.approval_id,
        input.expected_revision,
        &v.operation,
        // The backend the user actually answered for. A session approval of a
        // restricted backend is not the reduced-isolation answer a reusable consent
        // describes, so the class travels with the decision instead of being guessed
        // from the tool name.
        input.decision,
        v.backend.class,
        session_permission_revision,
        core_runtime_instance_id,
        workspace_root,
        at,
    )?;
    if grant.is_none() {
        crate::tool::result::not_executed_with_notice(
            tx,
            batch,
            call,
            aura_tool_contract::ToolNotExecutedReasonV1::PermissionDenied,
            Some(APPROVAL_DENIED),
            at,
        )?;
    }
    let mut t = turn.clone();
    t.transition(turn.revision, TurnTaskState::Running, at).map_err(|_| Error::Conflict)?;
    turns::save(tx, &turn, &t)?;
    let mut r = run.clone();
    r.state = AgentRunState::ToolsRunning { tool_batch_id: batch };
    r.revision = Revision::new(run.revision.value() + 1);
    r.updated_at = at;
    turns::save_run(tx, &run, &r)?;
    facts::append(tx, EventId::new(), &command.to_string(), "agent.approval_command", input, at)?;
    Ok(id)
}
pub fn invalidate_mode(
    tx: &Transaction<'_>,
    session: SessionId,
    at: UtcTimestamp,
) -> StoreResult<Option<AgentRunId>> {
    let Some(turn_id) = super::session::load(tx, session)?.active_turn else {
        return Ok(None);
    };
    let turn = turns::load(tx, turn_id)?;
    let run = turns::load_run(tx, turn.agent_run_id)?;
    let AgentRunState::AwaitingApproval { tool_batch_id } = run.state else {
        return Ok(None);
    };
    for id in pending(tx, run.agent_run_id)? {
        crate::security_approval::close(tx, id, ApprovalState::Invalidated, at)?;
    }
    let batch = crate::tool::execution::batch(tx, tool_batch_id)?;
    for call in &batch.batch.calls {
        let stored = crate::tool::execution::call(tx, call.tool_call_id)?;
        if stored.attempt_id.is_none() && stored.result.is_none() {
            crate::tool::result::not_executed_with_notice(
                tx,
                tool_batch_id,
                call.tool_call_id,
                aura_tool_contract::ToolNotExecutedReasonV1::PermissionDenied,
                Some(APPROVAL_INVALIDATED),
                at,
            )?;
        }
    }
    let mut t = turn.clone();
    t.transition(turn.revision, TurnTaskState::Running, at).map_err(|_| Error::Conflict)?;
    turns::save(tx, &turn, &t)?;
    let mut r = run.clone();
    r.state = AgentRunState::ToolsRunning { tool_batch_id };
    r.revision = Revision::new(run.revision.value() + 1);
    r.updated_at = at;
    turns::save_run(tx, &run, &r)?;
    Ok(Some(run.agent_run_id))
}
pub fn deadline(db: &Connection, run: AgentRunId) -> StoreResult<Option<UtcTimestamp>> {
    pending(db, run)?
        .into_iter()
        .map(|id| crate::security_approval::load(db, id).map(|a| a.expires_at))
        .collect::<StoreResult<Vec<_>>>()
        .map(|v| v.into_iter().min())
}
/// What the model and the user read for a call the user refused when asked. The stored reason
/// stays `permission_denied`, the one older Cores read; this line says what happened.
const APPROVAL_DENIED: &str = "approval.denied: 用户在批准时拒绝了这个调用。它没有执行，什么都没改。不要原样再调用：不用它接着做，或者问用户想怎么做。";

/// What the model and the user read for a call whose approval the session's permission mode
/// replaced while it waited.
const APPROVAL_INVALIDATED: &str = "approval.invalidated: 等待批准时会话的权限模式变了，这个调用没有执行，什么都没改。还需要的话重新调用，会按新的模式处理。";

/// What the model reads for a call whose approval expired before the user answered. The stored
/// reason stays `permission_denied`, the one older Cores read; this line says what happened.
const APPROVAL_EXPIRED: &str = "approval.expired: 这个调用的批准请求在用户回答之前就过期了。这不是拒绝：用户没有拒绝，调用也没有执行，什么都没改。用户可能暂时不在：建议先做完不需要它的部分，再告诉用户需要批准什么、为什么，等用户回来再调用（会重新请求批准）。";

/// Expiry records the unexecuted call and resumes the same admitted batch.
/// It never creates a Grant or reserves a new Attempt.
pub fn expire(tx: &Transaction<'_>, id: AgentRunId, at: UtcTimestamp) -> StoreResult<bool> {
    let run = turns::load_run(tx, id)?;
    let turn = turns::load(tx, run.turn_task_id)?;
    let AgentRunState::AwaitingApproval { tool_batch_id: batch } = run.state else {
        return Ok(false);
    };
    if turn.state.terminal() || application::cancellation_requested(tx, id)? {
        return Ok(false);
    }
    let admitted = crate::tool::admission::load(tx, batch)?.ok_or(Error::Conflict)?;
    let mut closed = false;
    for pending in pending(tx, id)? {
        let v = view(
            tx,
            &ExecutionApprovalRequest { session_id: turn.session_id, approval_id: pending },
        )?;
        if at < v.approval.expires_at {
            continue;
        }
        let call =
            *admitted.operations.iter().find(|(_, o)| **o == v.operation).ok_or(Error::Conflict)?.0;
        crate::security_approval::close(tx, pending, ApprovalState::Expired, at)?;
        crate::tool::result::not_executed_with_notice(
            tx,
            batch,
            call,
            aura_tool_contract::ToolNotExecutedReasonV1::PermissionDenied,
            Some(APPROVAL_EXPIRED),
            at,
        )?;
        closed = true;
    }
    if !closed || !pending(tx, id)?.is_empty() {
        return Ok(false);
    }
    let mut t = turn.clone();
    t.transition(turn.revision, TurnTaskState::Running, at).map_err(|_| Error::Conflict)?;
    turns::save(tx, &turn, &t)?;
    let mut r = run.clone();
    r.state = AgentRunState::ToolsRunning { tool_batch_id: batch };
    r.revision = Revision::new(run.revision.value().checked_add(1).ok_or(Error::Conflict)?);
    r.updated_at = at;
    turns::save_run(tx, &run, &r)?;
    Ok(true)
}
