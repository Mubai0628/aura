use aura_tool_eval::budget::{Ledger, Limits, ReconciledRequest, Reconciliation, Usage};

#[test]
fn recovery_requires_a_complete_explicit_reconciliation_and_persists_it() {
    let root = tempfile::tempdir().expect("isolated reconciliation ledger");
    let path = root.path().join("ledger.db");
    let limits = Limits { cumulative: 1000, task: 200, request_input: 100, request_output: 100 };
    let ledger = Ledger::open(&path, limits).expect("ledger");
    ledger.stop_task("stopped", "budget_exhausted").expect("independent task budget stop");
    let id = ledger.reserve("A", 100, 100, "generation").expect("unknown-cost request");
    ledger.unresolved(id).expect("stop all further sends");
    drop(ledger);
    let ledger = Ledger::open_for_reconciliation(&path).expect("read original limits");
    assert!(ledger.reserve("B", 1, 1, "generation").is_err(), "restart never clears a halt");
    let incomplete = Reconciliation {
        explanation: "Reviewed the provider usage receipt.".into(),
        requests: vec![],
    };
    assert!(ledger.reconcile(&incomplete).is_err(), "a note alone cannot release unknown cost");
    let mut note = Reconciliation {
        explanation: String::new(),
        requests: vec![ReconciledRequest {
            id,
            usage: Some(Usage { input: 60, output: 10, ..Usage::default() }),
        }],
    };
    assert!(ledger.reconcile(&note).is_err(), "an explanation is mandatory");
    note.explanation =
        "Reviewed provider receipt: this request used 60 input and 10 output tokens.".into();
    ledger
        .reconcile(&note)
        .expect("explicit recovery commits accounting and explanation atomically");
    assert_eq!(ledger.charged().expect("verified cost"), 70);
    assert!(
        ledger.reserve("stopped", 1, 1, "probe").is_err(),
        "global recovery never revives a stopped task"
    );
    let b = ledger.reserve("B", 1, 1, "probe").expect("later task after explicit recovery");
    ledger.settle(b, Usage { input: 1, output: 1, ..Usage::default() }).expect("B settled");
    drop(ledger);
    let ledger = Ledger::open(&path, limits).expect("reopen explicitly recovered ledger");
    assert_eq!(ledger.stop_reason().expect("global stop"), None);
    let db = rusqlite::Connection::open(path).expect("inspect metadata audit");
    let explanation: String = db
        .query_row("SELECT explanation FROM reconciliations", [], |r| r.get(0))
        .expect("durable audit note");
    assert_eq!(explanation, note.explanation);
}

#[test]
fn recovery_does_not_raise_the_cumulative_limit_or_revive_a_stopped_task() {
    let root = tempfile::tempdir().expect("isolated ledger");
    let path = root.path().join("ledger.db");
    let limits = Limits { cumulative: 100, task: 70, request_input: 40, request_output: 30 };
    let ledger = Ledger::open(&path, limits).expect("ledger");
    let id = ledger.reserve("A", 40, 30, "generation").expect("A reservation");
    ledger.settle(id, Usage { input: 40, output: 30, ..Usage::default() }).expect("A settled");
    assert!(ledger.reserve("A", 1, 1, "retry").is_err());
    let id = ledger.reserve("B", 15, 15, "generation").expect("B reservation");
    ledger
        .settle(id, Usage { input: 15, output: 15, ..Usage::default() })
        .expect("budget fully used");
    let note = Reconciliation {
        explanation: "The provider receipts confirm the full budget was consumed.".into(),
        requests: vec![],
    };
    assert!(ledger.reconcile(&note).is_err(), "consumed cumulative budget cannot be cleared");
    assert!(ledger.reserve("C", 1, 1, "generation").is_err());
}

#[test]
fn unconfirmed_usage_resumes_only_at_the_whole_reservation() {
    let root = tempfile::tempdir().expect("isolated ledger");
    let path = root.path().join("ledger.db");
    let limits = Limits { cumulative: 1000, task: 300, request_input: 100, request_output: 100 };
    let ledger = Ledger::open(&path, limits).expect("ledger");
    let settled = ledger.reserve("A", 100, 100, "generation").expect("first request");
    ledger.settle(settled, Usage { input: 30, output: 5, ..Usage::default() }).expect("settled");
    let lost = ledger.reserve("A", 90, 100, "retry").expect("request whose usage is lost");
    drop(ledger);
    let ledger = Ledger::open_for_reconciliation(&path).expect("reopen halts on pending usage");
    assert_eq!(ledger.stop_reason().expect("stop").as_deref(), Some("usage_unknown"));
    let note: Reconciliation = serde_json::from_value(serde_json::json!({
        "explanation": "Usage of the second request cannot be confirmed; it stays charged at its reservation.",
        "requests": [{"id": lost, "usage": null}]
    }))
    .expect("an explicit null usage");
    ledger.reconcile(&note).expect("recovery keeps the whole reservation charged");
    assert_eq!(ledger.charged().expect("cost"), 35 + 190, "nothing of the reservation is released");
    let record =
        ledger.records().expect("records").into_iter().find(|r| r.id == lost).expect("record");
    assert_eq!(
        (record.state.as_str(), record.input, record.output),
        ("charged_unknown", None, None)
    );
    drop(ledger);
    let ledger = Ledger::open(&path, limits).expect("reopen");
    assert_eq!(ledger.stop_reason().expect("no new halt"), None);
    ledger.reserve("B", 1, 1, "generation").expect("other tasks continue");
}
