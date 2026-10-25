use super::completion::{MAX_PENDING_TAILS, PostExecutionOwners, UnenteredClaim};
use crate::command_claims::CommandOutcomeLedger;
use crate::command_settlement::{CommandSettlements, TaskEnding};
use aura_domain_common::{CommandInvocationId, CorrelationId, RuntimeInstanceId};
use std::sync::{Arc, Mutex};

fn owners() -> (PostExecutionOwners, CommandSettlements) {
    let runtime = RuntimeInstanceId::new();
    (
        PostExecutionOwners::new(runtime),
        CommandSettlements::new(runtime, Arc::new(Mutex::new(CommandOutcomeLedger::default()))),
    )
}

#[test]
fn abandoning_the_pre_handler_slot_releases_its_claim_and_capacity() {
    let (owners, settlements) = owners();
    let invocation = CommandInvocationId::new();
    let responsibility = owners
        .claim(&settlements, Some(invocation), CorrelationId::new(), "test.write")
        .expect("free owner");
    let slot = Arc::new(Mutex::new(Some(responsibility)));
    let guard = UnenteredClaim::new(Arc::clone(&slot), settlements.clone());
    assert_eq!(settlements.pending(), 1);
    drop(guard);
    assert_eq!(settlements.pending(), 0);
    assert!(slot.lock().expect("slot").is_none());
    assert!(settlements.begin(invocation, CorrelationId::new(), "test.write").is_some());
}

#[test]
fn taking_the_responsibility_keeps_admitted_work_owned_after_the_waiter_drops() {
    let (owners, settlements) = owners();
    let responsibility = owners
        .claim(&settlements, Some(CommandInvocationId::new()), CorrelationId::new(), "test.write")
        .expect("free owner");
    let slot = Arc::new(Mutex::new(Some(responsibility)));
    let guard = UnenteredClaim::new(Arc::clone(&slot), settlements.clone());
    let running = slot.lock().expect("slot").take().expect("port took responsibility");
    drop(guard);
    assert_eq!(settlements.pending(), 1);
    let ticket = running.ticket().expect("command ticket");
    settlements.caller_departed(&ticket);
    settlements.task_finished(&ticket, TaskEnding::Succeeded(serde_json::json!({"done": true})));
    assert_eq!(settlements.pending(), 0);
}

#[test]
fn read_only_tails_also_refuse_before_start_at_the_owner_cap() {
    let (owners, settlements) = owners();
    let mut pending = Vec::new();
    for _ in 0..MAX_PENDING_TAILS {
        pending.push(
            owners.claim(&settlements, None, CorrelationId::new(), "test.read").expect("below cap"),
        );
    }
    let refused = owners.claim(&settlements, None, CorrelationId::new(), "test.read");
    let Err(error) = refused else { panic!("the owner cap must refuse this read") };
    assert_eq!(error.code.as_str(), "persistence.busy");
    assert!(error.retryable);
    assert!(!error.details.contains_key("outcome_recovery"));
    pending.pop();
    assert!(owners.claim(&settlements, None, CorrelationId::new(), "test.read").is_ok());
}

#[tokio::test]
async fn a_handler_or_evidence_panic_keeps_the_real_effect_unknown_and_never_replays_it() {
    for panic_in_evidence in [false, true] {
        let fixture = crate::r03_tests::Fixture::new();
        let state = fixture.start().await;
        let crate::StartupMode::Ready(services) = &state.startup_mode else { panic!("ready Core") };
        let correlation = CorrelationId::new();
        let invocation = CommandInvocationId::new();
        let file = fixture.root.path().join("owned-effect.txt");
        let handler_file = file.clone();
        let operation = super::operation(
            aura_ipc::method::FS_WRITE_FILE,
            &serde_json::json!({}),
            correlation,
            vec![aura_security_contract::ResolvedCapability::ConfigurationRead],
            vec![aura_security_contract::ResourceDescriptor::ExternalService {
                service_id: "fixture".into(),
                object_ref: None,
            }],
        )
        .expect("fixture operation");
        let result = super::run(
            &state,
            &services.broker,
            operation,
            invocation.to_string(),
            Some(invocation),
            Arc::new(|| Ok(serde_json::json!({"scope": "fixture"}))),
            Arc::new(move |_, _| -> anyhow::Result<aura_execution_contract::BrokerEvidence> {
                assert!(panic_in_evidence, "the handler panic must not enter evidence");
                panic!("fixture post-execution panic");
            }),
            move || async move {
                std::fs::write(handler_file, b"exactly one effect").expect("fixture effect");
                assert!(panic_in_evidence, "fixture handler panic");
                Ok(serde_json::json!({"done": true}))
            },
        )
        .await;
        let envelope = result.expect_err("the business terminal could not be established");
        assert_eq!(envelope.code.as_str(), "command.outcome_unknown");
        assert!(!envelope.retryable);
        assert_eq!(std::fs::read(file).expect("real effect"), b"exactly one effect");
        assert_eq!(state.settlements.pending(), 0, "the unwound owner really ended");
        let request = aura_ipc::JsonRpcRequest {
            jsonrpc: aura_ipc::JSON_RPC_VERSION.into(),
            id: correlation,
            command_invocation_id: Some(invocation),
            method: aura_ipc::method::FS_WRITE_FILE.into(),
            params: serde_json::json!({}),
        };
        crate::record_command_outcome(
            &state,
            &request,
            &aura_ipc::JsonRpcResponse::failure(correlation, envelope),
        )
        .expect("record real caller answer");
        let outcomes = state.command_outcomes.lock().expect("ledger");
        assert!(matches!(
            outcomes.query(invocation),
            aura_runtime_contract::CommandOutcome::Unknown { .. }
        ));
    }
}
