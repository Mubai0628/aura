use super::*;

fn owner() -> (CommandSettlements, Arc<Mutex<CommandOutcomeLedger>>) {
    let ledger = Arc::new(Mutex::new(CommandOutcomeLedger::default()));
    (CommandSettlements::new(RuntimeInstanceId::new(), Arc::clone(&ledger)), ledger)
}

#[test]
fn another_runtime_cannot_complete_or_release_this_runtime() {
    let (a, _) = owner();
    let (b, ledger) = owner();
    let invocation = CommandInvocationId::new();
    let correlation = CorrelationId::new();
    let a_ticket =
        a.begin(invocation, correlation, "session.create").expect("fixture claim admitted");
    let b_ticket =
        b.begin(invocation, correlation, "session.create").expect("fixture claim admitted");

    b.caller_departed(&b_ticket);
    b.task_finished(&a_ticket, TaskEnding::Succeeded(serde_json::json!({"source": "a"})));
    b.caller_settled(&a_ticket);
    b.never_ran(&a_ticket);
    assert!(b.is_pending(invocation));
    assert!(ledger.lock().expect("fixture ledger lock").is_pending(invocation));
    assert!(matches!(
        ledger.lock().expect("fixture ledger lock").query(invocation),
        CommandOutcome::Unknown { .. }
    ));

    b.task_finished(&b_ticket, TaskEnding::Succeeded(serde_json::json!({"source": "b"})));
    assert!(matches!(
        ledger.lock().expect("fixture ledger lock").query(invocation),
        CommandOutcome::Confirmed { terminal: IpcRequestTerminal::Succeeded, .. }
    ));
}

#[test]
fn a_stale_ticket_cannot_release_a_later_claim_with_the_same_ids() {
    let (owner, ledger) = owner();
    let invocation = CommandInvocationId::new();
    let correlation = CorrelationId::new();
    let old =
        owner.begin(invocation, correlation, "session.create").expect("fixture claim admitted");
    owner.never_ran(&old);
    let current =
        owner.begin(invocation, correlation, "session.create").expect("fixture claim admitted");

    owner.caller_departed(&old);
    owner.task_finished(&old, TaskEnding::Succeeded(serde_json::json!({"stale": true})));
    owner.caller_settled(&old);
    owner.never_ran(&old);
    assert!(owner.is_pending(invocation));
    assert!(ledger.lock().expect("fixture ledger lock").is_pending(invocation));
    assert!(matches!(
        ledger.lock().expect("fixture ledger lock").query(invocation),
        CommandOutcome::Unknown { .. }
    ));

    owner.caller_departed(&current);
    owner.task_finished(&current, TaskEnding::Succeeded(serde_json::json!({"current": true})));
    assert!(matches!(
        ledger.lock().expect("fixture ledger lock").query(invocation),
        CommandOutcome::Confirmed { terminal: IpcRequestTerminal::Succeeded, .. }
    ));
}

#[test]
fn a_typed_unknown_ending_keeps_the_pending_unknown_outcome() {
    let (owner, ledger) = owner();
    let invocation = CommandInvocationId::new();
    let correlation = CorrelationId::new();
    let ticket =
        owner.begin(invocation, correlation, "session.create").expect("fixture claim admitted");
    owner.caller_departed(&ticket);
    owner.task_finished(
        &ticket,
        TaskEnding::Failed(outcome_unknown_envelope(
            correlation,
            Some(invocation),
            "the underlying writer has not settled".into(),
        )),
    );
    assert!(owner.is_pending(invocation));
    assert!(ledger.lock().expect("fixture ledger lock").is_pending(invocation));
    assert!(matches!(
        ledger.lock().expect("fixture ledger lock").query(invocation),
        CommandOutcome::Unknown { .. }
    ));
}

#[test]
fn overdue_work_keeps_its_owner_until_the_real_thread_delivers() {
    let (owner, ledger) = owner();
    let invocation = CommandInvocationId::new();
    let ticket = owner
        .begin(invocation, CorrelationId::new(), "session.create")
        .expect("fixture claim admitted");
    owner.caller_departed(&ticket);
    let delivery = owner.clone();
    let (release, wait) = std::sync::mpsc::channel();
    let work = std::thread::spawn(move || {
        wait.recv().expect("release signal");
        delivery
            .task_finished(&ticket, TaskEnding::Succeeded(serde_json::json!({"finished": true})));
    });
    owner
        .inner
        .lock()
        .expect("fixture owner lock")
        .records
        .get_mut(&invocation)
        .expect("live owner")
        .deadline = Instant::now() - Duration::from_secs(1);
    owner
        .begin(CommandInvocationId::new(), CorrelationId::new(), "session.create")
        .expect("fixture claim admitted");
    let held_while_running = owner.is_pending(invocation);
    release.send(()).expect("worker is waiting");
    work.join().expect("worker finishes");

    assert!(held_while_running);
    assert!(matches!(
        ledger.lock().expect("fixture ledger lock").query(invocation),
        CommandOutcome::Confirmed { terminal: IpcRequestTerminal::Succeeded, .. }
    ));
}

#[test]
fn overdue_work_cannot_bypass_the_pending_cap_or_orphan_ledger_rows() {
    let (owner, ledger) = owner();
    let mut tickets = Vec::new();
    for _ in 0..MAX_PENDING_SETTLEMENTS {
        tickets.push(
            owner
                .begin(CommandInvocationId::new(), CorrelationId::new(), "session.create")
                .expect("fixture claim admitted"),
        );
    }
    for record in owner.inner.lock().expect("fixture owner lock").records.values_mut() {
        record.deadline = Instant::now() - Duration::from_secs(1);
    }
    assert!(
        owner.begin(CommandInvocationId::new(), CorrelationId::new(), "session.create").is_none()
    );
    assert_eq!(owner.pending(), MAX_PENDING_SETTLEMENTS);
    assert_eq!(ledger.lock().expect("fixture ledger lock").entries.len(), MAX_PENDING_SETTLEMENTS);
    assert!(
        tickets.iter().all(|ticket| ledger
            .lock()
            .expect("fixture ledger lock")
            .is_pending(ticket.invocation()))
    );

    let completed = &tickets[0];
    owner.caller_departed(completed);
    owner.task_finished(completed, TaskEnding::Succeeded(serde_json::json!({"finished": true})));
    assert!(!ledger.lock().expect("fixture ledger lock").is_pending(completed.invocation()));
    assert!(
        owner.begin(CommandInvocationId::new(), CorrelationId::new(), "session.create").is_some()
    );
    assert_eq!(owner.pending(), MAX_PENDING_SETTLEMENTS);
}
