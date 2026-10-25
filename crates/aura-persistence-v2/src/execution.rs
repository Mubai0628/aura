use aura_domain_common::{SecurityOperationId, UtcTimestamp};
use aura_execution_contract::{
    AttemptState, AuthorizedDispatch, ExecutionAttempt, ExecutionResult,
};
use aura_persistence_core::{
    PersistenceCoreError as Error, StoreResult, event,
    operation::{self, OperationState},
    record,
};
use aura_security_contract::AuthorizationDecision;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
pub mod broker_result;
pub mod process;
pub mod shell;
pub mod shell_result;

/// Receives a Core-checked dispatch, then atomically reserves authority and the
/// first exact Attempt. A replay hits the composite key or consumed grant.
///
/// `live_permission_revision` is the session's current permission revision, read
/// by Core under the mode barrier held across this transaction. A reusable Native
/// session consent is only honored while it still equals the revision the user
/// approved under.
pub fn prepare(
    tx: &Transaction<'_>,
    dispatch: &AuthorizedDispatch,
    authority: &AuthorizationDecision,
    live_permission_revision: u64,
    at: UtcTimestamp,
) -> StoreResult<ExecutionAttempt> {
    let request = dispatch.request();
    // There is one reservation per logical operation. A changed ordinal is not
    // permission to replay an existing dispatch (including an unknown outcome).
    let reserved: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM execution_attempts WHERE operation_id=?1)",
        [request.operation.operation_id.to_string()],
        |r| r.get(0),
    )?;
    if reserved {
        return Err(Error::Conflict);
    }
    let stored: String = tx.query_row(
        "SELECT payload FROM operations WHERE id=?1 AND state='prepared'",
        [request.operation.operation_id.to_string()],
        |r| r.get(0),
    )?;
    if record::decode::<aura_execution_contract::SecuredOperation>(&stored)? != request.operation {
        return Err(Error::Conflict);
    }
    if request.binding.authorization_fingerprint
        != authority.fingerprint().map_err(|_| Error::Invalid("authority fingerprint"))?
    {
        return Err(Error::Conflict);
    }
    crate::security::reserve_authority(
        tx,
        authority,
        &request.operation,
        live_permission_revision,
        at,
    )?;
    let attempt = ExecutionAttempt {
        binding: request.binding.clone(),
        prepared_at: at,
        dispatched_at: None,
        state: AttemptState::Prepared,
    };
    tx.execute(
        "INSERT INTO execution_attempts VALUES(?1,?2,?3,'prepared',1,?4)",
        params![
            attempt.binding.attempt.operation_id.to_string(),
            attempt.binding.attempt.ordinal.get(),
            request.plan.prepared.backend.id.0,
            record::encode(&attempt)?
        ],
    )?;
    operation::journal(
        tx,
        attempt.binding.attempt.operation_id,
        "dispatch_prepared",
        &serde_json::to_value(request)?,
        at,
    )?;
    operation::journal(
        tx,
        attempt.binding.attempt.operation_id,
        "dispatch_authority",
        &serde_json::to_value(authority)?,
        at,
    )?;
    Ok(attempt)
}

/// Reload the authority recorded atomically with the one reserved Attempt.
/// Once consumption belongs to that Attempt; revocation/expiry still apply at
/// the actual dispatch boundary. Legacy records without this fact fail closed.
pub fn dispatch_authority(
    connection: &Connection,
    request: &aura_execution_contract::DispatchRequest,
    now: UtcTimestamp,
) -> StoreResult<AuthorizationDecision> {
    if cancellation_requested(connection, &request.binding.attempt)? {
        return Err(Error::Conflict);
    }
    let value: String = connection.query_row(
        "SELECT payload FROM operation_journal_entries WHERE operation_id=?1 AND kind='dispatch_authority'",
        [request.operation.operation_id.to_string()], |r| r.get(0))?;
    let decision: AuthorizationDecision = record::decode(&value)?;
    if decision.decided_at > now
        || decision.expires_at <= now
        || decision.fingerprint().map_err(|_| Error::Invalid("authority fingerprint"))?
            != request.binding.authorization_fingerprint
    {
        return Err(Error::Conflict);
    }
    if let aura_security_contract::AuthorizationBasis::Grant { grant_id, revision } = decision.basis
    {
        let grant = crate::security::grant(connection, grant_id)?.ok_or(Error::Conflict)?;
        if grant.revision != revision
            || !crate::security::scope_authorizes(&grant.scope, &decision.scope)
            || grant.revoked_at.is_some()
            || grant.expires_at.is_some_and(|limit| limit <= now)
        {
            return Err(Error::Conflict);
        }
    }
    if matches!(decision.basis, aura_security_contract::AuthorizationBasis::Composite { .. }) {
        crate::network_consents::validate_dispatch(connection, &decision, &request.operation, now)?;
    }
    Ok(decision)
}
pub fn load(
    connection: &Connection,
    id: SecurityOperationId,
    ordinal: u32,
) -> StoreResult<ExecutionAttempt> {
    let v: String = connection.query_row(
        "SELECT payload FROM execution_attempts WHERE operation_id=?1 AND ordinal=?2",
        params![id.to_string(), ordinal],
        |r| r.get(0),
    )?;
    let attempt: ExecutionAttempt = record::decode(&v)?;
    attempt.validate().map_err(|_| Error::Invalid("execution attempt"))?;
    Ok(attempt)
}
pub fn mark_dispatched(
    tx: &Transaction<'_>,
    id: SecurityOperationId,
    ordinal: u32,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if cancellation_requested(
        tx,
        &aura_execution_contract::AttemptRef {
            operation_id: id,
            ordinal: std::num::NonZeroU32::new(ordinal).ok_or(Error::Conflict)?,
        },
    )? {
        return Err(Error::Conflict);
    }
    let mut a = load(tx, id, ordinal)?;
    a.mark_dispatched(at).map_err(|_| Error::Conflict)?;
    record::changed(tx.execute("UPDATE execution_attempts SET state='running',revision=revision+1,payload=?3 WHERE operation_id=?1 AND ordinal=?2 AND state='prepared'",params![id.to_string(),ordinal,record::encode(&a)?])?)?;
    let revision: i64 =
        tx.query_row("SELECT revision FROM operations WHERE id=?1", [id.to_string()], |r| {
            r.get(0)
        })?;
    operation::transition(tx, id, revision, OperationState::Running, at)
}
pub fn complete(
    tx: &Transaction<'_>,
    id: SecurityOperationId,
    ordinal: u32,
    result: &ExecutionResult,
) -> StoreResult<()> {
    let mut a = load(tx, id, ordinal)?;
    a.complete(result.clone()).map_err(|_| Error::Conflict)?;
    let mut evidence = vec![result.receipt.evidence_ref];
    if let aura_execution_contract::TerminationEvidence::Confirmed { evidence_ref } =
        result.termination
    {
        evidence.push(evidence_ref);
    }
    match result.receipt.rollback {
        aura_execution_contract::RollbackEvidence::Available { evidence_ref }
        | aura_execution_contract::RollbackEvidence::Completed { evidence_ref } => {
            evidence.push(evidence_ref)
        }
        _ => {}
    }
    for artifact in evidence {
        tx.execute("INSERT INTO operation_artifact_links VALUES(?1,?2,'execution_evidence') ON CONFLICT DO NOTHING",params![id.to_string(),artifact.to_string()])?;
    }
    tx.execute(
        "INSERT INTO execution_results VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            id.to_string(),
            ordinal,
            record::encode(&result.outcome)?,
            record::encode(&result.side_effect)?,
            result.receipt.evidence_ref.to_string(),
            record::encode(result)?
        ],
    )?;
    record::changed(tx.execute("UPDATE execution_attempts SET state='terminal',revision=revision+1,payload=?3 WHERE operation_id=?1 AND ordinal=?2 AND state<>'terminal'",params![id.to_string(),ordinal,record::encode(&a)?])?)?;
    let revision: i64 =
        tx.query_row("SELECT revision FROM operations WHERE id=?1", [id.to_string()], |r| {
            r.get(0)
        })?;
    let next = match result.outcome {
        aura_execution_contract::ExecutionOutcome::Succeeded => OperationState::Succeeded,
        aura_execution_contract::ExecutionOutcome::Cancelled => OperationState::Cancelled,
        aura_execution_contract::ExecutionOutcome::Unknown => OperationState::Unknown,
        _ => OperationState::Failed,
    };
    operation::transition(tx, id, revision, next, result.completed_at)?;
    event::append(
        tx,
        &event::EventDraft {
            id: aura_domain_common::EventId::new(),
            owner: &id.to_string(),
            kind: "execution.completed",
            version: 1,
            payload: &serde_json::to_value(result)?,
            at: result.completed_at,
        },
        "execution",
    )?;
    Ok(())
}
pub fn result(
    connection: &Connection,
    id: SecurityOperationId,
    ordinal: u32,
) -> StoreResult<Option<ExecutionResult>> {
    let v: Option<String> = connection
        .query_row(
            "SELECT payload FROM execution_results WHERE operation_id=?1 AND ordinal=?2",
            params![id.to_string(), ordinal],
            |r| r.get(0),
        )
        .optional()?;
    v.map(|v| {
        let r: ExecutionResult = record::decode(&v)?;
        r.validate().map_err(|_| Error::Invalid("execution result"))?;
        Ok(r)
    })
    .transpose()
}

/// Durable cancellation is written before signalling a backend. It also closes
/// the race where cancellation arrives before the backend registers its task.
pub fn request_cancel(
    tx: &Transaction<'_>,
    attempt: &aura_execution_contract::AttemptRef,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let current = load(tx, attempt.operation_id, attempt.ordinal.get())?;
    if matches!(current.state, AttemptState::Terminal { .. }) {
        return Ok(());
    }
    if !cancellation_requested(tx, attempt)? {
        operation::journal(
            tx,
            attempt.operation_id,
            "execution_cancel_requested",
            &serde_json::to_value(attempt)?,
            at,
        )?;
    }
    Ok(())
}
pub fn cancellation_requested(
    connection: &Connection,
    attempt: &aura_execution_contract::AttemptRef,
) -> StoreResult<bool> {
    Ok(connection.query_row("SELECT EXISTS(SELECT 1 FROM operation_journal_entries WHERE operation_id=?1 AND kind='execution_cancel_requested')",
        [attempt.operation_id.to_string()], |row| row.get(0))?)
}

/// Closes only an unclaimed reservation, without inventing a dispatched_at.
/// A concurrent claim makes this transaction fail; the caller must reconcile.
pub fn reject_prepared(
    tx: &Transaction<'_>,
    request: &aura_execution_contract::DispatchRequest,
    reason: &str,
    at: UtcTimestamp,
) -> StoreResult<ExecutionResult> {
    use aura_execution_contract::*;
    let attempt = &request.binding.attempt;
    let stored = load(tx, attempt.operation_id, attempt.ordinal.get())?;
    if stored.state != AttemptState::Prepared
        || stored.dispatched_at.is_some()
        || stored.binding != request.binding
    {
        return Err(Error::Conflict);
    }
    let evidence = aura_domain_common::ArtifactId::new();
    let cancelled = cancellation_requested(tx, attempt)?;
    let result = ExecutionResult {
        outcome: if cancelled { ExecutionOutcome::Cancelled } else { ExecutionOutcome::Failed },
        side_effect: SideEffectStateV1::NoEffect,
        receipt: ExecutionReceipt {
            attempt: attempt.clone(),
            evidence_ref: evidence,
            postcondition_fingerprint: None,
            rollback: RollbackEvidence::NotApplicable,
        },
        termination: TerminationEvidence::Confirmed { evidence_ref: evidence },
        completed_at: at,
        warnings: vec!["execution.not_dispatched".into()],
    };
    aura_persistence_core::artifact::put(
        tx,
        evidence,
        &attempt.operation_id.to_string(),
        "application/vnd.aura.prepared-rejection+json",
        &serde_json::to_vec(&serde_json::json!({
            "schema":"execution-prepared-rejection-v1", "binding":request.binding,
            "reason":reason, "cancelled":cancelled, "dispatched":false }))?,
        at,
    )?;
    complete(tx, attempt.operation_id, attempt.ordinal.get(), &result)?;
    Ok(result)
}
