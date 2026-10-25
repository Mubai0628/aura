use super::*;

#[test]
fn command_outcome_ledger_is_bounded_and_keeps_recent_terminals() {
    let first = CommandInvocationId::new();
    let mut ledger = CommandOutcomeLedger::default();
    ledger.insert(
        first,
        CommandOutcome::Confirmed {
            command_invocation_id: first,
            terminal: IpcRequestTerminal::Succeeded,
            response: json!({ "terminal": "succeeded" }),
        },
    );
    let mut latest = first;
    for _ in 0..MAX_COMMAND_OUTCOMES {
        latest = CommandInvocationId::new();
        ledger.insert(
            latest,
            CommandOutcome::Confirmed {
                command_invocation_id: latest,
                terminal: IpcRequestTerminal::Succeeded,
                response: json!({ "terminal": "succeeded" }),
            },
        );
    }
    assert_eq!(ledger.entries.len(), MAX_COMMAND_OUTCOMES);
    assert!(!ledger.entries.contains_key(&first));
    assert!(ledger.entries.contains_key(&latest));
}

#[test]
fn missing_command_outcome_is_explicitly_unknown_and_never_replayed() {
    let command_invocation_id = CommandInvocationId::new();
    assert_eq!(
        CommandOutcomeLedger::default().query(command_invocation_id),
        CommandOutcome::Unknown {
            command_invocation_id,
            reason: aura_runtime_contract::CommandOutcomeUnknown::NotRecordedOrIncomplete,
        }
    );
}

#[test]
fn a_command_invocation_id_is_claimed_before_it_is_executed() {
    // The completed-outcome check alone only stopped replays of an id that
    // had already finished. A replay arriving while the first attempt is
    // still running must be refused too, or the side effect happens twice.
    let id = CommandInvocationId::new();
    let mut ledger = CommandOutcomeLedger::default();
    let claim = ledger.begin(id, CorrelationId::new()).expect("the first attempt claims the id");
    assert_eq!(claim.invocation(), id);
    assert!(
        ledger.begin(id, CorrelationId::new()).is_none(),
        "a second attempt cannot claim it while it runs"
    );
    assert!(ledger.is_in_flight(id), "the refusal can name the in-flight case");
    ledger.insert(
        id,
        CommandOutcome::Confirmed {
            command_invocation_id: id,
            terminal: IpcRequestTerminal::Succeeded,
            response: json!({ "terminal": "succeeded" }),
        },
    );
    assert!(!ledger.is_in_flight(id), "recording the outcome releases the claim");
    assert!(ledger.begin(id, CorrelationId::new()).is_none(), "a completed id stays refused");
}

#[test]
fn an_abandoned_claim_is_not_a_recorded_outcome() {
    // A handler that never produced a recordable terminal must not leave a
    // claim behind: the effect stays Unknown, and the id can be retried.
    let id = CommandInvocationId::new();
    let mut ledger = CommandOutcomeLedger::default();
    let claim = ledger.begin(id, CorrelationId::new()).expect("claim");
    ledger.abandon(&claim);
    assert!(!ledger.is_in_flight(id));
    assert!(ledger.begin(id, CorrelationId::new()).is_some(), "an abandoned id is claimable again");
    assert!(matches!(ledger.query(id), CommandOutcome::Unknown { .. }));
}

#[test]
fn a_claim_belongs_to_one_request_and_one_generation() {
    // Ownership is precise: another request cannot borrow the claim, and after
    // the id is released and claimed again the previous credential is stale.
    let id = CommandInvocationId::new();
    let owner = CorrelationId::new();
    let other = CorrelationId::new();
    let mut ledger = CommandOutcomeLedger::default();
    let claim = ledger.begin(id, owner).expect("claim");
    assert!(ledger.owns(&claim));
    assert!(!claim.covers(other, id), "the credential names its own request");
    ledger.abandon(&claim);
    let second = ledger.begin(id, owner).expect("re-claim");
    assert!(!ledger.owns(&claim), "the previous generation is no longer live");
    assert!(ledger.owns(&second));
    assert_eq!(claim.invocation(), second.invocation());
}

#[test]
fn command_outcome_tracking_covers_side_effects_but_not_observations() {
    assert!(!is_side_effecting_main_method(method::PROJECTION_SNAPSHOT));
    assert!(!is_side_effecting_main_method(method::FS_READ_TEXT));
    assert!(!is_side_effecting_main_method(method::PERMISSION_GET_SESSION_STATE));
    assert!(is_side_effecting_main_method(method::WORKSPACE_CREATE));
    assert!(is_side_effecting_main_method(method::FS_WRITE_FILE));
    assert!(is_side_effecting_main_method(method::PERMISSION_SET_SESSION_MODE));
    assert!(is_side_effecting_main_method(method::RUNTIME_CANCEL));
}
