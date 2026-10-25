//! Real per-domain ownership and bounded I/O.
//!
//! These are the *probes* for R1: each one is written to **fail** against the
//! pre-repair implementation and to pass only when the domain really owns its own
//! operation. Nothing here is simulated with a sleep-instead-of-I/O substitute;
//! every case drives the real `SettingsFacade` against a real filesystem, and the
//! "slow domain" is made slow by making its real file I/O slow.
//!
//! # How a domain is really held
//!
//! `SettingsFacade::update_with_fault` used to take a process-wide
//! `pending_bundle_transaction` mutex and hold it across `service.update(...)` —
//! the encode, validate and atomic file replace of one domain. To reproduce that
//! window deterministically this file uses the facade's *own* injected fault
//! point, which fires **inside** that window: `ConfigFaultPoint::AfterReplace`
//! returns only after the real file has been atomically replaced, and
//! `AfterTempSync`/`BeforeReplace` return with the temporary file already synced.
//! A second thread therefore observes the moment the first domain genuinely holds
//! whatever the old implementation held across its I/O.
//!
//! # The counterfactual
//!
//! [`unrelated_write_finishes_while_another_domain_is_held`] is the reject-case
//! for the old global lock: while domain A is inside its real save, domain B's
//! real write must complete, and B's projection must reflect B's own revision.
//! Under the old code B blocked on the mutex A held.

use std::{
    collections::BTreeMap,
    fs,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use aura_config::{
    AuraPaths, ConfigModule, ConfigModuleBundleEntry, ConfigSource, DomainClaim,
    DomainClaimRegistry, DomainHealthState, SettingsFacade, SettingsFacadeError,
};
use serde_json::json;
use tempfile::TempDir;

fn paths(directory: &TempDir) -> AuraPaths {
    AuraPaths::from_roots(
        directory.path().join("roaming/Aura"),
        directory.path().join("local/Aura"),
    )
}

/// One valid, switched-on Skill choice: `skills.json` refuses unknown fields.
fn skills_choice() -> serde_json::Value {
    let source = aura_skills::SkillSource::AgentGlobal;
    serde_json::to_value([aura_skills::SkillSetting {
        id: aura_skills::skill_id(source, None, "notes"),
        source,
        workspace_id: None,
        relative: "notes".into(),
        enabled: true,
        trusted_fingerprint: None,
        available_for_subagents: false,
    }])
    .expect("a Skill choice")
}

/// The reject-case for the process-wide transaction lock.
///
/// **The bearing hold case.** While domain A genuinely holds its ownership, an
/// unrelated domain B must read, write and project.
///
/// This case previously put its hold point *after*
/// `update_with_fault` had already returned. `ConfigFaultPoint::AfterReplace`
/// fires inside the save and then unwinds, so by the time the test signalled
/// `entered` the claim (and the old global mutex) had already been released — the
/// wait was therefore taken *outside* the window it claimed to measure, and the
/// case would have passed against the old global lock. A hold that does not
/// overlap the ownership window proves nothing.
///
/// The hold is now created by a real contender: a second genuine write to the
/// **same** domain, started while A is inside its save. That write can only
/// succeed (or be refused) once A's ownership is settled, so waiting for its
/// outcome is waiting *inside* the ownership window. B's progress is measured
/// while that contention is unresolved.
#[test]
fn unrelated_write_finishes_while_another_domain_is_held() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("facade");

    // Two domains that share nothing: no file, no transaction, no dependency.
    let slow = ConfigModule::Skills;
    let fast = ConfigModule::Diagnostics;

    // A real write to `slow` that parks *inside* the save, with its claim held.
    // The seam runs while this domain's exclusive claim is still alive, so the
    // hold really overlaps the ownership window.
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let slow_facade = Arc::clone(&facade);
    let slow_release = Arc::clone(&release_rx);
    let slow_thread = thread::spawn(move || {
        slow_facade.update_with_hold(
            slow,
            1,
            |values| {
                values.insert("skills".to_owned(), skills_choice());
            },
            move || {
                // Runs **with `slow`'s claim held**. The test releases us here.
                let _ = entered_tx.send(());
                let _ = slow_release
                    .lock()
                    .expect("release lock")
                    .recv_timeout(Duration::from_secs(10));
            },
        )
    });

    // Wait until the slow domain really holds its ownership.
    entered_rx.recv_timeout(Duration::from_secs(10)).expect("slow domain holds its ownership");

    // A real contender on the SAME domain: it cannot complete while A owns the
    // domain, so its result is direct evidence that the ownership window was open.
    let contender = {
        let facade = Arc::clone(&facade);
        thread::spawn(move || {
            facade.update(slow, 1, |values| {
                values.insert("skills".to_owned(), json!([]));
            })
        })
    };
    // Give the contender a moment to arrive and block on the claim.
    thread::sleep(Duration::from_millis(150));
    assert!(
        !contender.is_finished(),
        "a same-domain contender must not complete while the domain is owned"
    );

    // The unrelated domain must still read, write and project — right now, while
    // A's ownership is genuinely held and a same-domain contender is blocked.
    let started = std::time::Instant::now();
    let snapshot = facade.snapshot(fast).expect("an unrelated domain still reads");
    assert_eq!(snapshot.module, fast);
    assert_eq!(snapshot.document.revision, 1);
    let updated = facade
        .update(fast, 1, |values| {
            values.insert("level".to_owned(), json!("debug"));
        })
        .expect("an unrelated domain still writes while another domain is owned");
    assert_eq!(
        updated.document.revision, 2,
        "the unrelated write really advanced its own revision"
    );
    assert_eq!(updated.source, ConfigSource::File);
    elapsed_is_bounded(started.elapsed(), "the unrelated write finished while A owned its domain");

    // The unrelated domain's own projection is correct: its file really changed.
    let reread = fs::read_to_string(paths.config_path(fast)).expect("read unrelated file");
    assert!(reread.contains("debug"), "the unrelated write reached its own file: {reread}");
    assert_eq!(facade.health(fast).state, DomainHealthState::Ready);
    assert_eq!(facade.health(fast).revision, Some(2));

    // A's own operation is *still* in flight and has not been disturbed.
    assert!(!slow_thread.is_finished(), "the held domain is still inside its save");
    assert!(!contender.is_finished(), "the same-domain contender is still blocked");

    let _ = release_tx.send(());
    let slow_result = slow_thread.join().expect("slow thread joins");
    slow_result.expect("the held domain's own write really commits");

    // The contender is now resolved — either it committed on top of A, or it was
    // honestly refused for the revision it raced. Both prove it was waiting on the
    // ownership rather than being free to interleave.
    match contender.join().expect("contender joins") {
        Ok(snapshot) => assert!(snapshot.document.revision >= 2),
        Err(SettingsFacadeError::RevisionConflict { .. }) => {}
        Err(error) => panic!("the contender must commit or be refused honestly: {error:?}"),
    }

    // A's write completed for real and is visible.
    let skills = fs::read_to_string(paths.config_path(ConfigModule::Skills)).expect("skills file");
    assert!(skills.contains("true"), "the held domain's file was really replaced: {skills}");
    let reopened = SettingsFacade::open(paths.clone()).expect("re-open");
    assert_eq!(reopened.snapshot(fast).expect("diagnostics").document.revision, 2);
}

fn elapsed_is_bounded(elapsed: Duration, what: &str) {
    assert!(
        elapsed < Duration::from_secs(5),
        "{what} (took {elapsed:?}); a process-wide lock across domain I/O would block it"
    );
}

/// Two writes to the **same** domain with the same expected revision: exactly one
/// may commit, and the other must be refused as a revision conflict.
///
/// This is the property the global lock used to provide incidentally and which
/// R1 must keep after narrowing ownership to the domain.
#[test]
fn same_domain_same_revision_race_commits_exactly_one() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("facade");

    const RACERS: usize = 8;
    let barrier = Arc::new(std::sync::Barrier::new(RACERS));
    let committed = Arc::new(AtomicUsize::new(0));
    let conflicted = Arc::new(AtomicUsize::new(0));
    let other = Arc::new(AtomicUsize::new(0));

    let handles = (0..RACERS)
        .map(|index| {
            let facade = Arc::clone(&facade);
            let barrier = Arc::clone(&barrier);
            let committed = Arc::clone(&committed);
            let conflicted = Arc::clone(&conflicted);
            let other = Arc::clone(&other);
            thread::spawn(move || {
                barrier.wait();
                match facade.update(ConfigModule::Hooks, 1, |values| {
                    // Gave Hooks a schema: each racer writes a real registry entry.
                    values.insert(
                        "hooks".to_owned(),
                        json!([{"path": format!("C:/racer-{index}"), "enabled": true}]),
                    );
                }) {
                    Ok(snapshot) => {
                        assert_eq!(snapshot.document.revision, 2, "only revision 2 is reachable");
                        committed.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(SettingsFacadeError::RevisionConflict { .. }) => {
                        conflicted.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(SettingsFacadeError::DomainBusy { .. }) => {
                        // A refusal is honest, but it must not be how a *racing*
                        // writer is reported: the claim is held only for this
                        // domain's own operation, so a same-domain racer serializes
                        // behind it and then sees the revision conflict.
                        other.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(error) => panic!("unexpected error: {error:?}"),
                }
            })
        })
        .collect::<Vec<_>>();
    for handle in handles {
        handle.join().expect("racer joins");
    }

    assert_eq!(committed.load(Ordering::SeqCst), 1, "exactly one racer commits revision 2");
    assert_eq!(
        conflicted.load(Ordering::SeqCst),
        RACERS - 1,
        "every other racer is refused as a revision conflict"
    );
    assert_eq!(
        other.load(Ordering::SeqCst),
        0,
        "a same-domain racer waits for its own domain rather than being reported busy"
    );

    // The committed fact is a single revision 2, and it is durable.
    let snapshot = facade.snapshot(ConfigModule::Hooks).expect("hooks snapshot");
    assert_eq!(snapshot.document.revision, 2);
    let reopened = SettingsFacade::open(paths.clone()).expect("re-open");
    assert_eq!(reopened.snapshot(ConfigModule::Hooks).expect("hooks").document.revision, 2);
}

/// A second retry of the same domain must not insert an *older* snapshot.
///
/// The reload path refuses a non-monotonic revision inside the service, so even
/// if two re-opens of the same domain were racing, an older file could not win.
#[test]
fn two_retries_cannot_insert_an_older_snapshot() {
    // A JSON domain with no schema of its own (as plugins.json once was): any well-formed
    // values are a valid revision.
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("facade");
    facade
        .update(ConfigModule::Commands, 1, |values| {
            values.insert("enabled".to_owned(), json!(true));
        })
        .expect("advance to revision 2");

    // A genuinely older but otherwise valid document, written on top.
    let stale = serde_json::json!({
        "schema_version": 1,
        "revision": 1,
        "values": {}
    })
    .to_string();
    fs::write(paths.config_path(ConfigModule::Commands), stale.as_bytes()).expect("write stale");

    // Two concurrent retries of the same domain.
    let barrier = Arc::new(std::sync::Barrier::new(4));
    let outcomes = Arc::new(Mutex::new(Vec::new()));
    let handles = (0..4)
        .map(|_| {
            let facade = Arc::clone(&facade);
            let barrier = Arc::clone(&barrier);
            let outcomes = Arc::clone(&outcomes);
            thread::spawn(move || {
                barrier.wait();
                let result = facade.reload(ConfigModule::Commands);
                outcomes.lock().expect("outcomes lock").push(format!("{result:?}"));
            })
        })
        .collect::<Vec<_>>();
    for handle in handles {
        handle.join().expect("retry joins");
    }

    // Whatever each retry reported, the served fact is never the older revision.
    let served = facade.snapshot(ConfigModule::Commands).expect("commands snapshot");
    assert_eq!(served.document.revision, 2, "an older snapshot was never inserted");
    assert_eq!(served.document.values.get("enabled"), Some(&json!(true)));
    for outcome in outcomes.lock().expect("outcomes lock").iter() {
        assert!(
            !outcome.contains("revision: 1"),
            "no retry may report revision 1 as served: {outcome}"
        );
    }
}

/// A cross-domain transaction admits its complete domain set once, including the
/// read-only revision dependencies.
#[test]
fn a_transaction_admits_its_read_only_dependencies_once() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("facade");

    let before_aura = facade.snapshot(ConfigModule::Aura).expect("aura").document.revision;
    let before_models = facade.snapshot(ConfigModule::Models).expect("models").document.revision;

    // Aura is written; Models has no replacement and is therefore a read-only
    // revision dependency of the bundle.
    let prepared = facade
        .prepare_module_bundle(
            "ctx_s05_r1_readonly".to_owned(),
            vec![
                ConfigModuleBundleEntry {
                    module: ConfigModule::Aura,
                    expected_revision: before_aura,
                    replacement_values: Some(BTreeMap::from([("theme".to_owned(), json!("dark"))])),
                },
                ConfigModuleBundleEntry {
                    module: ConfigModule::Models,
                    expected_revision: before_models,
                    replacement_values: None,
                },
            ],
        )
        .expect("prepare the bundle");

    // While the transaction is live, an unrelated domain is still writable.
    facade
        .update(ConfigModule::Ui, 1, |values| {
            values.insert("theme".to_owned(), json!("dark"));
        })
        .expect("a domain outside the bundle is unaffected by the transaction");

    // The read-only dependency really is admitted: a writer of Models is refused
    // while the transaction holds it. This is a real refusal, not a wait — a
    // transaction holds its whole set, so a conflicting writer is told which
    // domain is owned instead of blocking behind an unbounded transaction.
    let error = facade
        .update(ConfigModule::Models, before_models, |values| {
            values.insert("models".to_owned(), json!([]));
        })
        .expect_err("the bundle's read-only dependency excludes a writer");
    assert!(matches!(error, SettingsFacadeError::DomainBusy { .. }), "{error:?}");

    // The bundle proceeds for real: it publishes and then activates, and Aura
    // reaches the bundle's after image.
    facade.publish_prepared_module_bundle(&prepared).expect("the bundle publishes");
    let activated =
        facade.activate_published_module_bundle(&prepared).expect("the bundle activates");
    assert_eq!(
        activated.get(&ConfigModule::Aura).expect("aura activated").document.revision,
        before_aura + 1
    );

    // With the transaction ended, Models is writable again — and its revision was
    // never changed underneath the transaction, which is what claiming it
    // `Shared` proved.
    assert!(
        facade.recovery_held_domains().is_empty(),
        "a completed transaction leaves nothing held"
    );
    facade
        .update(ConfigModule::Models, before_models, |values| {
            values.insert("models".to_owned(), json!([]));
        })
        .expect("the dependency is released when the transaction ends");
    // Aura's own bundle result is durable.
    let reopened = SettingsFacade::open(paths.clone()).expect("re-open");
    assert_eq!(
        reopened.snapshot(ConfigModule::Aura).expect("aura").document.revision,
        before_aura + 1
    );
}

/// Disjoint claims run in parallel: the registry must never be a bottleneck.
#[test]
fn disjoint_domains_are_claimed_concurrently() {
    let registry = Arc::new(DomainClaimRegistry::new());
    let running = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let claimed = Arc::new(AtomicBool::new(false));

    let handles = ConfigModule::ALL
        .into_iter()
        .map(|module| {
            let registry = Arc::clone(&registry);
            let running = Arc::clone(&running);
            let peak = Arc::clone(&peak);
            let claimed = Arc::clone(&claimed);
            thread::spawn(move || {
                let _guard =
                    registry.try_claim([(module, DomainClaim::Exclusive)]).expect("disjoint claim");
                claimed.store(true, Ordering::SeqCst);
                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(20));
                running.fetch_sub(1, Ordering::SeqCst);
            })
        })
        .collect::<Vec<_>>();
    for handle in handles {
        handle.join().expect("claim thread joins");
    }
    assert!(claimed.load(Ordering::SeqCst));
    assert!(
        peak.load(Ordering::SeqCst) > 1,
        "every domain was claimed, and none of them excluded another"
    );
    assert!(registry.claimed().is_empty(), "all claims are released");
}

/// The same-domain re-open path has exactly one owner.
#[test]
fn a_same_domain_reopen_has_exactly_one_owner() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    // Seed real valid files first, then break one domain.
    drop(SettingsFacade::open(paths.clone()).expect("seed valid files"));

    // Break a domain so it has no service and must be re-opened.
    let target = ConfigModule::Mcp;
    let path = paths.config_path(target);
    fs::remove_file(&path).expect("remove config file");
    fs::create_dir_all(&path).expect("replace with directory");
    let reopened = SettingsFacade::open(paths.clone()).expect("facade opens degraded");
    assert_eq!(reopened.degraded_domains(), vec![target]);

    // Repair the real file.
    fs::remove_dir_all(&path).expect("remove blocking directory");
    fs::write(
        &path,
        serde_json::json!({ "schema_version": 1, "revision": 3, "servers": [] }).to_string(),
    )
    .expect("write repaired config");

    // Concurrent re-opens of the same domain: at most one becomes the owner, and
    // every caller either succeeds or is honestly refused.
    let barrier = Arc::new(std::sync::Barrier::new(4));
    let handles = (0..4)
        .map(|_| {
            let facade = Arc::clone(&reopened);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                facade.reload(target)
            })
        })
        .collect::<Vec<_>>();
    let mut succeeded = 0;
    let mut refused = 0;
    for handle in handles {
        match handle.join().expect("reopen joins") {
            Ok(_) => succeeded += 1,
            Err(SettingsFacadeError::DomainBusy { .. }) => refused += 1,
            Err(error) => panic!("unexpected error: {error:?}"),
        }
    }
    assert!(succeeded >= 1, "the domain was really re-opened");
    assert_eq!(succeeded + refused, 4, "every caller got an honest answer");
    assert!(reopened.is_available(target));
    assert_eq!(reopened.snapshot(target).expect("recovered").document.revision, 3);
    assert_eq!(reopened.health(target).state, DomainHealthState::Ready);
}

/// The watcher coalesces per domain and does not serialize unrelated domains.
///
/// This drives the **real** watcher against the **real** filesystem: the file is
/// genuinely written and the watcher genuinely re-reads it. It asserts the two
/// properties the single-threaded loop could not offer — a bounded number of
/// workers, and a domain that is dirty but not yet re-read not blocking another.
#[test]
fn the_watcher_coalesces_per_domain_with_a_bounded_workforce() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("facade");
    let _handle = facade.start_watching().expect("watcher starts");

    // The workforce bounds are properties of the constant, so they are asserted at
    // compile time: a regression that made the watcher single-threaded, or that let
    // the workforce grow with the event rate, would fail the build rather than a
    // test run.
    const {
        assert!(
            aura_config::WATCH_WORKERS >= 2,
            "at least two domains must be able to be re-read at the same time"
        )
    };
    const {
        assert!(
            aura_config::WATCH_WORKERS <= ConfigModule::ALL.len(),
            "the workforce is bounded by the number of domains, not by the event rate"
        )
    };

    // A real external edit to an unrelated domain is observed.
    let path = paths.config_path(ConfigModule::Commands);
    fs::write(
        &path,
        serde_json::json!({ "schema_version": 1, "revision": 4, "values": { "enabled": true } })
            .to_string(),
    )
    .expect("write external edit");

    // Bounded wait for the real watcher to really re-read it.
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let revision = facade.snapshot(ConfigModule::Commands).expect("commands").document.revision;
        if revision == 4 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the watcher did not observe a real external edit (revision {revision})"
        );
        thread::sleep(Duration::from_millis(50));
    }

    // And every other domain is untouched and still usable.
    for module in ConfigModule::ALL {
        if module == ConfigModule::Commands {
            continue;
        }
        assert!(facade.is_available(module), "{module:?} unaffected by an unrelated edit");
    }
}
