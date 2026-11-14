use aura_tool_eval::budget::{Ledger, Limits, Usage};

#[test]
fn a_task_cap_does_not_halt_other_tasks_or_a_reopened_ledger() {
    let root = tempfile::tempdir().expect("isolated budget ledger");
    let path = root.path().join("ledger.db");
    let limits = Limits { cumulative: 1000, task: 70, request_input: 40, request_output: 30 };
    let ledger = Ledger::open(&path, limits).expect("ledger opens");
    let a = ledger.reserve("A", 40, 30, "generation").expect("A reservation");
    ledger.settle(a, Usage { input: 40, output: 30, ..Usage::default() }).expect("A cost");
    assert!(ledger.reserve("A", 1, 1, "retry").is_err(), "A cannot overspend");
    assert_eq!(ledger.stop_reason().expect("global stop state"), None, "only A stops");
    let b = ledger.reserve("B", 10, 10, "generation").expect("B may send after A stops");
    ledger.settle(b, Usage { input: 10, output: 10, ..Usage::default() }).expect("B cost");
    drop(ledger);
    let ledger = Ledger::open(&path, limits).expect("reopen settled ledger");
    assert!(ledger.reserve("A", 1, 1, "generation").is_err(), "A remains stopped");
    ledger.reserve("C", 10, 10, "compaction").expect("new task remains allowed");
}

#[test]
fn the_observed_body_growth_fits_the_revised_smoke_reservations() {
    let root = tempfile::tempdir().expect("isolated replay accounting");
    let ledger = Ledger::open(
        &root.path().join("ledger.db"),
        Limits {
            cumulative: 50_000_000,
            task: 600_000,
            request_input: 262_144,
            request_output: 4096,
        },
    )
    .expect("smoke policy");
    for step in 0..8 {
        let body = 60_800 + 6500 * step;
        let id = ledger
            .reserve("smoke", body + 4096, 4096, "generation")
            .expect("eight observed-size steps fit the approved reservation policy");
        ledger
            .settle(id, Usage { input: body / 3, output: 2048, ..Usage::default() })
            .expect("synthetic conservative token ratio, not provider evidence");
    }
    assert_eq!(ledger.records().expect("step accounting").len(), 8);
    assert_eq!(ledger.stop_reason().expect("no global halt"), None);
}
