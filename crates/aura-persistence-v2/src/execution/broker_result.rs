//! Domain effects and their receipt may be committed in the same owning unit
//! of work. Failed persistence never yields a synthetic successful Result.
use aura_domain_common::{ArtifactId, UtcTimestamp, jcs::canonical_sha256};
use aura_execution_contract::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact};
use rusqlite::Transaction;

pub fn record(
    tx: &Transaction<'_>,
    request: &DispatchRequest,
    evidence: &BrokerEvidence,
    success: bool,
    at: UtcTimestamp,
) -> StoreResult<ExecutionResult> {
    if evidence.action_kind != request.operation.content.action_kind
        || evidence.request_fingerprint != request.operation.fingerprint.0
    {
        return Err(Error::Conflict);
    }
    let current =
        super::load(tx, request.operation.operation_id, request.binding.attempt.ordinal.get())?;
    if current.binding != request.binding {
        return Err(Error::Conflict);
    }
    let id = ArtifactId::new();
    let result = ExecutionResult {
        outcome: if success { ExecutionOutcome::Succeeded } else { ExecutionOutcome::Failed },
        side_effect: evidence.side_effect,
        receipt: ExecutionReceipt {
            attempt: request.binding.attempt.clone(),
            evidence_ref: id,
            postcondition_fingerprint: if evidence.after.is_null() {
                None
            } else {
                Some(
                    canonical_sha256(&evidence.after)
                        .map_err(|_| Error::Invalid("Broker postcondition"))?,
                )
            },
            rollback: match evidence.side_effect {
                SideEffectStateV1::Compensated => RollbackEvidence::Completed { evidence_ref: id },
                SideEffectStateV1::NoEffect => RollbackEvidence::NotApplicable,
                _ => RollbackEvidence::Unavailable,
            },
        },
        termination: TerminationEvidence::Confirmed { evidence_ref: id },
        completed_at: at,
        warnings: if evidence.side_effect == SideEffectStateV1::Unknown {
            vec!["tool.side_effect.unverified".into()]
        } else {
            vec![]
        },
    };
    artifact::put(
        tx,
        id,
        &request.operation.operation_id.to_string(),
        "application/vnd.aura.broker-evidence+json",
        &serde_json::to_vec(evidence)?,
        at,
    )?;
    super::complete(
        tx,
        request.operation.operation_id,
        request.binding.attempt.ordinal.get(),
        &result,
    )?;
    Ok(result)
}
