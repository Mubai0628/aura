use aura_tool_eval::budget::{Ledger, Limits, Usage};
use std::sync::Arc;

fn limits() -> Limits {
    Limits { cumulative: 100, task: 70, request_input: 40, request_output: 30 }
}

#[test]
fn budget_refuses_before_send_at_each_limit() {
    let root = tempfile::tempdir().expect("isolated ledger");
    let ledger = Ledger::open(&root.path().join("ledger.db"), limits()).expect("ledger opens");
    assert!(ledger.reserve("oversized-input", 41, 1, "probe").is_err(), "input cap gates sending");
    assert!(
        ledger.reserve("oversized-output", 1, 31, "probe").is_err(),
        "output cap gates sending"
    );
    ledger.reserve("one", 40, 30, "generation").expect("first reservation");
    assert!(ledger.reserve("one", 1, 1, "retry").is_err(), "task cap includes retries");
    assert!(
        ledger.reserve("two", 40, 30, "compaction").is_err(),
        "cumulative cap includes auxiliaries"
    );
}

#[test]
fn concurrent_reservations_cannot_overspend() {
    let root = tempfile::tempdir().expect("isolated ledger");
    let ledger = Arc::new(Ledger::open(&root.path().join("ledger.db"), limits()).expect("ledger"));
    let handles = (0..8)
        .map(|i| {
            let ledger = Arc::clone(&ledger);
            std::thread::spawn(move || {
                ledger.reserve(&format!("t{i}"), 20, 20, "generation").is_ok()
            })
        })
        .collect::<Vec<_>>();
    let allowed = handles
        .into_iter()
        .filter(|h| h.thread().id() != std::thread::current().id())
        .map(|h| h.join().expect("reservation worker"))
        .filter(|v| *v)
        .count();
    assert_eq!(allowed, 2);
}

#[test]
fn usage_is_idempotent_and_cache_is_included_in_total() {
    let root = tempfile::tempdir().expect("isolated ledger");
    let ledger = Ledger::open(&root.path().join("ledger.db"), limits()).expect("ledger");
    let id = ledger.reserve("task", 40, 30, "generation").expect("reserve");
    let usage = Usage {
        input: 20,
        output: 10,
        cached_input: 12,
        cached_input_reported: true,
        ..Usage::default()
    };
    ledger.settle(id, usage).expect("settle");
    ledger.settle(id, usage).expect("duplicate usage is harmless");
    assert_eq!(ledger.charged().expect("charged"), 30);
    assert_eq!(ledger.records().expect("cached metadata")[0].cached_input, Some(12));
    assert!(ledger.settle(id, Usage { input: 21, ..usage }).is_err());
}

#[test]
fn missing_usage_and_crash_do_not_release_reservation_or_resume() {
    let root = tempfile::tempdir().expect("isolated ledger");
    let path = root.path().join("ledger.db");
    let ledger = Ledger::open(&path, limits()).expect("ledger");
    let id = ledger.reserve("task", 40, 30, "generation").expect("reserve");
    ledger.unresolved(id).expect("unknown usage recorded");
    assert_eq!(ledger.charged().expect("held budget"), 70);
    assert!(ledger.reserve("other", 1, 1, "probe").is_err());
    drop(ledger);
    assert!(
        Ledger::open(&path, limits())
            .expect("historical ledger remains readable")
            .reserve("other", 1, 1, "generation")
            .is_err()
    );
}

#[test]
fn failed_ledger_open_never_permits_a_send() {
    let root = tempfile::tempdir().expect("isolated ledger");
    assert!(Ledger::open(root.path(), limits()).is_err());
}

#[test]
fn crash_with_a_pending_request_and_limit_changes_fail_closed() {
    let root = tempfile::tempdir().expect("isolated ledger");
    let path = root.path().join("ledger.db");
    let ledger = Ledger::open(&path, limits()).expect("ledger");
    ledger.reserve("task", 40, 30, "generation").expect("pending request");
    drop(ledger);
    let ledger = Ledger::open(&path, limits()).expect("historical ledger");
    assert_eq!(ledger.charged().expect("reservation retained"), 70);
    assert_eq!(
        ledger.stop_reason().expect("crash recovery state").as_deref(),
        Some("usage_unknown"),
        "a request left in flight by a crash halts every task until reconciliation"
    );
    assert!(ledger.reserve("task", 1, 1, "retry").is_err());
    assert!(ledger.reserve("other", 1, 1, "generation").is_err(), "not only the crashed task");
    assert!(
        Ledger::open(&path, Limits { cumulative: 200, ..limits() }).is_err(),
        "cannot raise cumulative limit on restart"
    );
}
