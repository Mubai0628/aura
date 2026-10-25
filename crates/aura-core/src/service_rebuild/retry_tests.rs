use super::*;

#[test]
fn a_failed_first_recovery_releases_its_whole_set_without_losing_completed_work() {
    let ledger = StartupLedger::new();
    use OneTimeStartupStep::{AgentInboxStartupScan as Scan, AgentWaitRestoration as Wait};
    ledger.mark_completed(Scan);
    let claim =
        ledger.claim_steps(&[Scan, Wait]).expect("obtain the actual startup responsibility");
    assert!(claim.needs_work());
    assert!(ledger.is_in_flight(Wait));
    assert!(ledger.claim_steps(&[Scan, Wait]).is_err());
    drop(claim); // a real error return relinquishes this call's uncompleted work
    assert!(ledger.is_completed(Scan));
    assert!(!ledger.is_in_flight(Wait));
    ledger.claim_steps(&[Scan, Wait]).expect("obtain the actual startup responsibility").complete();
    assert!(ledger.is_completed(Wait));
    assert!(
        !ledger
            .claim_steps(&[Scan, Wait])
            .expect("obtain the actual startup responsibility")
            .needs_work()
    );
}

#[test]
fn construction_unwind_cannot_permanently_poison_a_missing_capability() {
    let claim = ConstructionClaim::new();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        claim.with_responsibility(|| false, || (), |()| panic!("constructor publication failed"))
    }));
    assert!(result.is_err());
    assert!(!claim.is_held());
    assert!(matches!(claim.with_responsibility(|| false, || 3, |v| v), ClaimOutcome::Built(3)));
}
