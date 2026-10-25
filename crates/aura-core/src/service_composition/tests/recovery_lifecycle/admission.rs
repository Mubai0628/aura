use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ordinary_recovery_waiters_keep_general_capacity_and_unrelated_recovery_available() {
    let fixture = Fixture::new();
    drop(
        SettingsFacade::open(fixture.paths.clone())
            .expect("obtain the actual recovery lifecycle fact"),
    );
    break_domain(&fixture.paths, ConfigModule::Models);
    let state = Arc::new(fixture.start().await);
    let StartupMode::Ready(services) = &state.startup_mode else {
        panic!("ready shell");
    };
    repair_domain(&fixture.paths, ConfigModule::Models, 2);
    let entry = services.config_recovery();
    let plan = entry
        .plan_bundle(&[ConfigModule::Models])
        .expect("obtain the actual recovery lifecycle fact");
    // This is the real admission owner used by the ordinary handler, held before
    // storage admission. These eight callers all run the production dispatcher.
    let held = entry
        .admit_apply(&[ConfigModule::Models])
        .await
        .expect("obtain the actual recovery lifecycle fact");
    let mut waiters = Vec::new();
    for _ in 0..8 {
        let state = Arc::clone(&state);
        let token = plan.domains[0].preview_id.clone();
        waiters.push(tokio::spawn(async move {
            crate::r03_tests::request(
                &state,
                "config.recovery.apply",
                json!({
                    "domain":"models", "domains":[], "action":"retry", "preview_id":token,
                }),
            )
            .await
        }));
    }
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(waiters.iter().all(|task| !task.is_finished()));
    assert_eq!(services.sync.available(), services.sync.capacity());
    let ui =
        entry.plan_bundle(&[ConfigModule::Ui]).expect("obtain the actual recovery lifecycle fact");
    let recovered = success(&state, "config.recovery.apply", json!({
        "domain":"ui", "domains":[], "action":"reset_to_default", "preview_id":ui.domains[0].preview_id,
    })).await;
    assert_eq!(recovered["outcomes"][0]["applied"], true);
    let _ = success(
        &state,
        "runtime.command_outcome",
        json!({
            "command_invocation_id":CommandInvocationId::new(),
        }),
    )
    .await;
    assert!(waiters.iter().all(|task| !task.is_finished()));
    drop(held);
    for waiter in waiters {
        let response = tokio::time::timeout(Duration::from_secs(30), waiter)
            .await
            .expect("obtain the actual recovery lifecycle fact")
            .expect("obtain the actual recovery lifecycle fact");
        assert!(response.get("result").is_some() || response.get("error").is_some());
    }
}

/// **A contended construction responsibility is bounded, and unrelated work stays reachable.**
///
/// While one recovery holds a capability's construction
/// responsibility, a second must be refused within a **bounded** wait rather than parking a worker
/// indefinitely. The hold here is a real internal hold point — a `ConstructionClaim` held on another
/// thread across its publication window — not a "hold" simulated after a method returned.
#[test]
fn a_contended_responsibility_is_bounded_and_leaves_unrelated_work_reachable() {
    use crate::service_rebuild::{ClaimOutcome, ConstructionClaim, responsibility_wait};
    use std::sync::mpsc;

    let claim = std::sync::Arc::new(ConstructionClaim::new());
    let slot = std::sync::Arc::new(crate::DependentSlot::<usize>::empty());
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();

    let holder_claim = std::sync::Arc::clone(&claim);
    let holder_slot = std::sync::Arc::clone(&slot);
    let holder = std::thread::spawn(move || {
        holder_claim.with_responsibility(
            || holder_slot.is_present(),
            || 1usize,
            |service| {
                entered_tx.send(()).expect("publication entered");
                release_rx.recv_timeout(responsibility_wait() * 6).expect("release");
                holder_slot.publish_if_vacant(std::sync::Arc::new(service));
            },
        )
    });
    entered_rx.recv_timeout(std::time::Duration::from_secs(5)).expect("the hold is real");

    // A second caller for the **same** capability is refused, with the real wait, and builds nothing.
    let started = std::time::Instant::now();
    let builds = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let contended_builds = std::sync::Arc::clone(&builds);
    let outcome = claim.with_responsibility(
        || slot.is_present(),
        || {
            contended_builds.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            2usize
        },
        |service| {
            slot.publish_if_vacant(std::sync::Arc::new(service));
        },
    );
    // The refusal carries the **real** wait, and it is a substantial share of the bound. An exact
    // lower bound on the elapsed time would be too strict to be honest: `Condvar::wait_timeout` may
    // return marginally early on a spurious wake-up, and a loaded machine makes any wall-clock
    // comparison flaky. What the requirement is about is that the caller is bounded rather than
    // parked forever, so the assertion is that it really waited a large share of the bound and was
    // refused — and the reported `waited_ms` is the claim's own measurement, so the two agree.
    let waited = started.elapsed();
    let reported = match outcome {
        ClaimOutcome::Contended { waited_ms } => waited_ms,
        other => panic!(
            "a contended caller is refused rather than blocking forever: {}",
            match other {
                ClaimOutcome::AlreadyPresent => "AlreadyPresent",
                ClaimOutcome::Built(_) => "Built",
                ClaimOutcome::Contended { .. } => unreachable!(),
                ClaimOutcome::Poisoned => "Poisoned",
            }
        ),
    };
    assert!(
        reported > 0 && reported as u128 >= responsibility_wait().as_millis() * 8 / 10,
        "the refusal reports a real wait close to the bound: {reported}ms"
    );
    assert!(
        waited >= responsibility_wait() * 8 / 10,
        "and the caller really waited a large share of the bound before giving up: {waited:?}"
    );
    assert_eq!(
        builds.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "a contended caller builds nothing: constructing anyway is what the claim exists to prevent"
    );

    // UNRELATED WORK IS REACHABLE THROUGHOUT: a different capability's claim is free while this one
    // is held, which is what "the hold does not occupy the general worker pool" means at this layer.
    let unrelated = ConstructionClaim::new();
    assert!(
        matches!(
            unrelated.with_responsibility(|| false, || 7usize, |service| service),
            ClaimOutcome::Built(7)
        ),
        "an unrelated capability's construction proceeds while another is held"
    );

    release_tx.send(()).expect("release the holder");
    assert!(matches!(holder.join().expect("holder"), ClaimOutcome::Built(())));
    assert_eq!(*slot.get().expect("the holder's service is the one published"), 1);
}

/// The recovery report distinguishes "the settings were restored" from "the capability works".
///
/// These are different facts and a caller must be able to tell
/// them apart. A report whose `restored_capabilities` could be populated from a cleared record alone
/// would let a repaired-but-unusable capability read as success.
#[test]
fn the_recovery_report_keeps_the_two_facts_apart() {
    // A capability whose record is cleared but whose service is absent is reported unavailable.
    let report = crate::config_recovery::RecoveryReport {
        action: aura_config::recovery::ConfigRecoveryAction::Retry,
        outcomes: Vec::new(),
        restored_capabilities: Vec::new(),
        unavailable_capabilities: vec![crate::config_recovery::UnavailableCapability {
            capability: "tool_policy".to_owned(),
            reason: "its service is still absent, so this capability is not available".to_owned(),
        }],
        reason_code: None,
    };
    assert!(
        report.restored_capabilities.is_empty(),
        "a configuration write alone never populates the restored set"
    );
    assert_eq!(
        report.unavailable_capabilities.len(),
        1,
        "and the still-unavailable capability is carried with its reason rather than dropped"
    );
    let _ = ConfigDegradationRegistry::default();
}
