use aura_contract_schema::{V2KernelFixture, v2_kernel_fixture};
use aura_execution_contract::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, operation};
use aura_persistence_v2::{V2Store, execution, security};

fn reserve(store: &mut V2Store, f: &V2KernelFixture) -> AuthorizedDispatch {
    let dispatch = AuthorizedDispatch::revalidate(
        f.dispatch.clone(),
        &f.authority,
        &f.dispatch.plan.prepared.backend,
        &f.cancellation,
        f.authority.decided_at,
    )
    .expect("authorized fixture");
    store
        .database
        .unit_of_work(|tx| {
            operation::begin(
                tx,
                &operation::OperationDraft {
                    id: f.dispatch.operation.operation_id,
                    domain: "execution",
                    kind: "fixture.read",
                    request_key: "same-logical-operation",
                    version: 1,
                    payload: &serde_json::to_value(&f.dispatch.operation)?,
                    at: f.authority.decided_at,
                },
            )?;
            security::insert_grant(tx, &f.grant)?;
            execution::prepare(tx, &dispatch, &f.authority, 0, f.authority.decided_at)
        })
        .expect("atomic reservation");
    dispatch
}

#[test]
fn cancellation_survives_restart_and_prevents_late_claim_without_fake_dispatch() {
    let dir = tempfile::tempdir().expect("temp");
    let root = dir.path().join("v2");
    let mut store = V2Store::create(&root).expect("create");
    let f = v2_kernel_fixture().expect("fixture");
    let dispatch = reserve(&mut store, &f);
    let attempt = &f.dispatch.binding.attempt;
    let at = f.authority.decided_at;
    store
        .database
        .unit_of_work(|tx| {
            execution::request_cancel(tx, attempt, at)?;
            execution::request_cancel(tx, attempt, at)
        })
        .expect("idempotent durable cancel");
    drop(store);
    let mut store = V2Store::open(&root).expect("restart");
    assert!(execution::dispatch_authority(store.database.connection(), &f.dispatch, at).is_err());
    assert!(
        store
            .database
            .unit_of_work(|tx| execution::mark_dispatched(tx, attempt.operation_id, 1, at,))
            .is_err()
    );
    let result = store
        .database
        .unit_of_work(|tx| {
            execution::reject_prepared(tx, &f.dispatch, "cancelled before Runner registration", at)
        })
        .expect("close unclaimed reservation");
    assert_eq!(result.outcome, ExecutionOutcome::Cancelled);
    assert_eq!(result.side_effect, SideEffectStateV1::NoEffect);
    assert!(
        execution::load(store.database.connection(), attempt.operation_id, 1)
            .expect("attempt")
            .dispatched_at
            .is_none()
    );
    assert!(
        store
            .database
            .unit_of_work(|tx| execution::prepare(tx, &dispatch, &f.authority, 0, at))
            .is_err()
    );
    assert!(
        store
            .database
            .unit_of_work(|tx| execution::complete(tx, attempt.operation_id, 1, &result))
            .is_err()
    );
}

#[test]
fn prepared_rejection_rolls_back_evidence_and_cannot_close_a_claimed_attempt() {
    let dir = tempfile::tempdir().expect("temp");
    let mut store = V2Store::create(&dir.path().join("v2")).expect("create");
    let f = v2_kernel_fixture().expect("fixture");
    reserve(&mut store, &f);
    let id = f.dispatch.operation.operation_id;
    let at = f.authority.decided_at;
    let failed: StoreResult<()> = store.database.unit_of_work(|tx| {
        execution::reject_prepared(tx, &f.dispatch, "fixture refusal", at)?;
        Err(Error::Invalid("simulated interruption before commit"))
    });
    assert!(failed.is_err());
    assert!(execution::result(store.database.connection(), id, 1).expect("result").is_none());
    let artifacts: u32 = store
        .database
        .connection()
        .query_row("SELECT count(*) FROM artifacts", [], |r| r.get(0))
        .expect("count");
    assert_eq!(artifacts, 0);
    store.database.unit_of_work(|tx| execution::mark_dispatched(tx, id, 1, at)).expect("claim");
    assert!(
        store
            .database
            .unit_of_work(|tx| execution::reject_prepared(tx, &f.dispatch, "late refusal", at))
            .is_err()
    );
    assert_eq!(
        execution::load(store.database.connection(), id, 1).expect("attempt").state,
        AttemptState::Running
    );
}

#[test]
fn undispatched_terminal_requires_exact_no_effect_evidence() {
    let f = v2_kernel_fixture().expect("fixture");
    let prepared = ExecutionAttempt {
        binding: f.dispatch.binding.clone(),
        prepared_at: f.authority.decided_at,
        dispatched_at: None,
        state: AttemptState::Prepared,
    };
    let mut rejection = f.result;
    rejection.outcome = ExecutionOutcome::Failed;
    rejection.side_effect = SideEffectStateV1::NoEffect;
    rejection.receipt.rollback = RollbackEvidence::NotApplicable;
    rejection.termination =
        TerminationEvidence::Confirmed { evidence_ref: rejection.receipt.evidence_ref };
    rejection.warnings = vec!["execution.not_dispatched".into()];
    prepared.clone().complete(rejection.clone()).expect("proved refusal");
    for change in 0..5 {
        let mut invalid = rejection.clone();
        match change {
            0 => invalid.outcome = ExecutionOutcome::Succeeded,
            1 => invalid.side_effect = SideEffectStateV1::Unknown,
            2 => invalid.termination = TerminationEvidence::Unknown,
            3 => invalid.warnings.clear(),
            _ => {
                invalid.receipt.attempt.operation_id =
                    aura_domain_common::SecurityOperationId::new()
            }
        }
        assert!(prepared.clone().complete(invalid).is_err(), "invalid refusal {change}");
    }
}
