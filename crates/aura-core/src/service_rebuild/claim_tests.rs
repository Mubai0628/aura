use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::time::Duration;

use super::*;

#[test]
fn construction_responsibility_covers_the_exact_publication_window() {
    let claim = Arc::new(ConstructionClaim::new());
    let slot = Arc::new(crate::DependentSlot::<usize>::empty());
    let builds = Arc::new(AtomicUsize::new(0));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let first_claim = Arc::clone(&claim);
    let first_slot = Arc::clone(&slot);
    let first_builds = Arc::clone(&builds);
    let first = std::thread::spawn(move || {
        first_claim.with_responsibility(
            || first_slot.is_present(),
            || {
                first_builds.fetch_add(1, Ordering::SeqCst);
                Arc::new(7)
            },
            |service| {
                assert!(
                    first_claim.is_held(),
                    "the construction responsibility must still be held before publication"
                );
                entered_tx.send(()).expect("publication entered");
                release_rx.recv_timeout(Duration::from_secs(5)).expect("bounded release");
                assert!(first_slot.publish_if_vacant(service));
            },
        )
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).expect("the exact publication window is held");
    // While the first caller holds the responsibility, a second caller genuinely contends: it waits
    // and then takes the already-published service rather than building a second one.
    assert!(claim.is_held(), "the responsibility is really held during publication");
    let second_claim = Arc::clone(&claim);
    let second_slot = Arc::clone(&slot);
    let second_builds = Arc::clone(&builds);
    let second = std::thread::spawn(move || {
        second_claim.with_responsibility(
            || second_slot.is_present(),
            || {
                second_builds.fetch_add(1, Ordering::SeqCst);
                Arc::new(9)
            },
            |service| assert!(second_slot.publish_if_vacant(service)),
        )
    });
    release_tx.send(()).expect("release first publication");
    assert!(matches!(first.join().expect("first builder"), ClaimOutcome::Built(())));
    assert!(matches!(second.join().expect("second caller"), ClaimOutcome::AlreadyPresent));
    assert_eq!(builds.load(Ordering::SeqCst), 1);
    assert_eq!(*slot.get().expect("the first service is retained"), 7);
}

#[test]
fn an_existing_occupant_is_kept_and_a_failed_build_can_be_retried() {
    let claim = ConstructionClaim::new();
    let slot = crate::DependentSlot::<usize>::empty();
    let first = claim.with_responsibility(
        || slot.is_present(),
        || Err::<Arc<usize>, _>("failed"),
        |result| result,
    );
    assert!(matches!(first, ClaimOutcome::Built(Err("failed"))));
    let second = claim.with_responsibility(
        || slot.is_present(),
        || Arc::new(11),
        |service| assert!(slot.publish_if_vacant(service)),
    );
    assert!(matches!(second, ClaimOutcome::Built(())));
    let occupant = slot.get().expect("published service");
    let third = claim.with_responsibility(
        || slot.is_present(),
        || panic!("an occupied slot must not construct another service"),
        |_: ()| panic!("an occupied slot must not publish"),
    );
    assert!(matches!(third, ClaimOutcome::AlreadyPresent));
    assert!(Arc::ptr_eq(&occupant, &slot.get().expect("same service")));
}

/// **A contended responsibility is refused after a bounded wait, and builds nothing.**
///
/// An unbounded wait lets one slow construction occupy every
/// caller that asks for the same capability, which is a worker unavailable to the unrelated
/// recoveries and controls that must keep working. The wait is therefore bounded, and a caller that
/// exceeds it reports the real wait and constructs **nothing** — giving up on the wait is the
/// opposite of permission to build a second service.
#[test]
fn a_contended_responsibility_is_refused_within_the_bound_and_builds_nothing() {
    let claim = Arc::new(ConstructionClaim::new());
    let slot = Arc::new(crate::DependentSlot::<usize>::empty());
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let holder_claim = Arc::clone(&claim);
    let holder_slot = Arc::clone(&slot);
    let holder = std::thread::spawn(move || {
        holder_claim.with_responsibility(
            || holder_slot.is_present(),
            || Arc::new(1),
            |service| {
                entered_tx.send(()).expect("publication entered");
                // Held well past the bound, so the contention below is real rather than a race.
                release_rx.recv_timeout(responsibility_wait() * 6).expect("release");
                assert!(holder_slot.publish_if_vacant(service));
            },
        )
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).expect("the responsibility is held");

    let builds = Arc::new(AtomicUsize::new(0));
    let contended_builds = Arc::clone(&builds);
    let started = std::time::Instant::now();
    let outcome = claim.with_responsibility(
        || slot.is_present(),
        || {
            contended_builds.fetch_add(1, Ordering::SeqCst);
            Arc::new(2)
        },
        |service| {
            slot.publish_if_vacant(service);
        },
    );
    let waited = started.elapsed();
    let bound = responsibility_wait();
    assert!(
        matches!(outcome, ClaimOutcome::Contended { .. }),
        "a caller that cannot take the responsibility within the bound is refused"
    );
    // The bound is enforced, measured against the bound this run actually used. An exact lower bound
    // would be dishonest under load: `Condvar::wait_timeout` may return marginally early, and a
    // shared machine adds scheduling delay to the observed wall clock. What the requirement is about
    // is that the caller is bounded rather than parked forever.
    assert!(
        waited >= bound * 8 / 10,
        "the caller really waited the bound before giving up: {waited:?} against a bound of {bound:?}"
    );
    assert_eq!(
        builds.load(Ordering::SeqCst),
        0,
        "a contended caller must not construct: that would be the second service the claim exists \
         to prevent"
    );
    release_tx.send(()).expect("release the holder");
    assert!(matches!(holder.join().expect("holder"), ClaimOutcome::Built(())));
    assert_eq!(*slot.get().expect("the holder's service is the one published"), 1);
}

/// A step is recorded complete only after its work returns, and exactly one caller may claim it.
///
/// The defect was an assumption rather than a record: the live
/// constructors behaved as though the one-time startup work had happened, and nothing said whether
/// it had. A ledger that marked a step on an intention would be the same mistake in a new place, so
/// this pins both halves — a claimed step is not completed, and only one caller gets the claim.
#[test]
fn a_one_time_step_is_claimed_once_and_completed_only_after_its_work() {
    let ledger = StartupLedger::new();
    let step = OneTimeStartupStep::AgentInboxStartupScan;

    assert!(!ledger.is_completed(step), "a step nobody ran is not complete");
    assert!(
        ledger.outstanding().contains(&step),
        "and it is reported as outstanding, which is what makes the first recovery happen"
    );

    assert!(ledger.claim(step), "the first caller takes the responsibility");
    assert!(
        !ledger.is_completed(step),
        "a claimed step whose work has not returned is NOT complete: marking it on an intention is \
         the copyable marker the brief rejects"
    );
    assert!(ledger.is_in_flight(step), "and it is visibly in flight");
    assert!(!ledger.claim(step), "no second caller may claim the same step");

    ledger.mark_completed(step);
    assert!(ledger.is_completed(step), "the completion is recorded after the work returned");
    assert!(!ledger.outstanding().contains(&step), "and it stops being outstanding");
    assert!(!ledger.claim(step), "a completed step is never claimed again");
}

/// The steps a process really discharged are not outstanding, and the rest are.
///
/// This is the distinction the whole item turns on: "already done, do not repeat" versus "never
/// reached, must be discharged". A ledger that reported everything outstanding would re-run
/// completed reconciliation; one that reported nothing outstanding would skip the missing first
/// responsibility, which is the defect.
#[test]
fn only_the_steps_that_really_completed_are_excluded_from_the_outstanding_set() {
    let ledger = StartupLedger::new();
    // The shape of a process that had a model authority but no Agent application.
    ledger.mark_completed(OneTimeStartupStep::ModelConfigJournalReconciliation);
    ledger.mark_completed(OneTimeStartupStep::InterruptedModelOperationCancellation);

    let outstanding = ledger.outstanding();
    assert!(
        !outstanding.contains(&OneTimeStartupStep::ModelConfigJournalReconciliation),
        "what startup really did is not repeated"
    );
    assert!(
        outstanding.contains(&OneTimeStartupStep::AgentInboxStartupScan),
        "what startup never reached is still owed"
    );
    assert!(
        outstanding.contains(&OneTimeStartupStep::AgentWaitRestoration),
        "including the wait restoration"
    );
    assert!(
        outstanding.contains(&OneTimeStartupStep::AgentDerivedBackfill),
        "and the derived backfill owner"
    );
}
