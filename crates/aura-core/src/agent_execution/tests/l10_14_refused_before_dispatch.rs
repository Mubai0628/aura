//! In the owner's
//! store the read's attempt had ended the way the execution contract asks for a call its backend
//! refused before starting it — failed, no effect, marked `execution.not_dispatched` — but its tool
//! result could not carry that marker. The call never got a result, its run could not record the
//! failure ("persistent revision or identity conflict"), and no stop could end the turn.
use super::*;
use aura_backend_api::{BackendFuture, ExecutionBackend};
use aura_execution_contract::{
    AttemptRef, AuthorizedDispatch, BackendDescriptor, BackendError, CancelReceipt,
    ExecutionHandle, ExecutionPlan, PrepareRequest, PreparedExecution, ReconcileResult,
};
use fixture::Fixture;
use std::sync::Arc;

/// A backend that refuses every dispatch before it starts, as the broker does when a call's target
/// changed between its preparation and its claim.
struct RefusingBackend {
    descriptor: BackendDescriptor,
}

impl ExecutionBackend for RefusingBackend {
    fn descriptor(&self) -> BackendDescriptor {
        self.descriptor.clone()
    }

    fn prepare(&self, request: PrepareRequest) -> BackendFuture<'_, ExecutionPlan> {
        let descriptor = self.descriptor.clone();
        Box::pin(async move {
            ExecutionPlan::new(PreparedExecution {
                operation_fingerprint: request.operation.fingerprint,
                backend: descriptor,
                preparation: serde_json::json!({}),
            })
            .map_err(|error| BackendError::PreparationRejected { reason: error.to_string() })
        })
    }

    fn execute(&self, _dispatch: AuthorizedDispatch) -> BackendFuture<'_, ExecutionHandle> {
        Box::pin(async move {
            Err(BackendError::DispatchRejected { reason: "agent.resource_changed".into() })
        })
    }

    fn cancel(&self, handle: ExecutionHandle) -> BackendFuture<'_, CancelReceipt> {
        Box::pin(async move { Err(BackendError::DispatchUnknown { attempt: handle.attempt }) })
    }

    fn reconcile(&self, attempt: AttemptRef) -> BackendFuture<'_, ReconcileResult> {
        Box::pin(async move { Ok(ReconcileResult::Unknown { attempt }) })
    }
}

#[tokio::test]
async fn a_call_refused_before_it_started_has_a_result_and_its_run_can_be_stopped() {
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let admitted = fixture.admit(&[(
        "fs.write",
        serde_json::json!({"path":"refused.txt","text":"never","expected":{"expected_kind":"absent"}}),
    )]);
    let call_id = admitted.batch.calls[0].tool_call_id;
    let plan = admitted.planned.execution_plan.calls[0].clone();
    let generation = admitted.planned.runtime_calls[0].generation.clone();
    let call = admitted.calls[&call_id].clone();
    let execution = fixture.service().agent_execution_required();
    let refusing: Arc<dyn ExecutionBackend> = Arc::new(RefusingBackend {
        descriptor: execution.broker_backend(&call).expect("broker backend").descriptor(),
    });
    let (_keep, cancel) = tokio::sync::watch::channel(false);
    let outcome = execution
        .execute_call(call, &plan, generation, std::slice::from_ref(&refusing), cancel)
        .await;
    let result = match outcome {
        Ok(CallOutcome::Terminal(result)) => result,
        Ok(_) => panic!("a call refused before it started is answered with its result"),
        Err(error) => panic!("a call refused before it started is answered: {error:#}"),
    };
    // It did not run, nothing changed, and that is what the call's result says.
    assert!(matches!(result.outcome, ToolResultOutcomeV1::Failed { .. }), "{:?}", result.outcome);
    assert_eq!(result.side_effect_state, SideEffectStateV1::NoEffect);
    assert!(!result.attempts[0].dispatched, "{result:?}");
    assert!(!fixture.root.join("refused.txt").exists());

    // 停止 ends the turn: the interruption finds every call of the batch answered.
    let run = fixture.turn.agent_run_id;
    fixture
        .service()
        .broker
        .lock()
        .expect("V2 store")
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::agent::recovery::interrupt(
                tx,
                run,
                "agent.cancelled",
                true,
                UtcTimestamp::now(),
            )
        })
        .expect("the turn can be interrupted");
}

/// A backend that takes the call and never says how it ended.
struct SilentBackend {
    descriptor: BackendDescriptor,
}

impl ExecutionBackend for SilentBackend {
    fn descriptor(&self) -> BackendDescriptor {
        self.descriptor.clone()
    }

    fn prepare(&self, request: PrepareRequest) -> BackendFuture<'_, ExecutionPlan> {
        let descriptor = self.descriptor.clone();
        Box::pin(async move {
            ExecutionPlan::new(PreparedExecution {
                operation_fingerprint: request.operation.fingerprint,
                backend: descriptor,
                preparation: serde_json::json!({}),
            })
            .map_err(|error| BackendError::PreparationRejected { reason: error.to_string() })
        })
    }

    fn execute(&self, dispatch: AuthorizedDispatch) -> BackendFuture<'_, ExecutionHandle> {
        let request = dispatch.request().clone();
        Box::pin(async move {
            let fingerprint =
                aura_domain_common::jcs::canonical_sha256(&request.binding).map_err(|_| {
                    BackendError::DispatchUnknown { attempt: request.binding.attempt.clone() }
                })?;
            Ok(ExecutionHandle {
                attempt: request.binding.attempt,
                dispatch_fingerprint: fingerprint,
            })
        })
    }

    fn cancel(&self, handle: ExecutionHandle) -> BackendFuture<'_, CancelReceipt> {
        Box::pin(async move { Err(BackendError::DispatchUnknown { attempt: handle.attempt }) })
    }

    fn reconcile(&self, attempt: AttemptRef) -> BackendFuture<'_, ReconcileResult> {
        Box::pin(async move { Ok(ReconcileResult::Unknown { attempt }) })
    }
}

#[tokio::test]
async fn a_turn_ends_when_stopped_even_if_one_call_result_cannot_be_built() {
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let admitted = fixture.admit(&[(
        "fs.write",
        serde_json::json!({"path":"unreadable.txt","text":"never","expected":{"expected_kind":"absent"}}),
    )]);
    let call_id = admitted.batch.calls[0].tool_call_id;
    let plan = admitted.planned.execution_plan.calls[0].clone();
    let generation = admitted.planned.runtime_calls[0].generation.clone();
    let call = admitted.calls[&call_id].clone();
    let operation = call.operation.operation_id;
    let execution = fixture.service().agent_execution_required();
    let silent: Arc<dyn ExecutionBackend> = Arc::new(SilentBackend {
        descriptor: execution.broker_backend(&call).expect("broker backend").descriptor(),
    });
    let (_keep, cancel) = tokio::sync::watch::channel(false);
    let outcome = execution
        .execute_call(call, &plan, generation, std::slice::from_ref(&silent), cancel)
        .await;
    assert!(matches!(outcome, Ok(CallOutcome::Unknown(_))), "the call started and went quiet");

    let run = fixture.turn.agent_run_id;
    let mut store = fixture.service().broker.lock().expect("V2 store");
    // Its end arrives later with evidence this Core cannot read (a success in a form it does not
    // know), so the call's result can never be built.
    store
        .database
        .unit_of_work(|tx| {
            let evidence = aura_domain_common::ArtifactId::new();
            let at = UtcTimestamp::now();
            // Started, as the broker records it when it claims a call.
            aura_persistence_v2::execution::mark_dispatched(tx, operation, 1, at)?;
            aura_persistence_core::artifact::put(
                tx,
                evidence,
                &operation.to_string(),
                "application/json",
                br#"{"schema":"a-form-this-core-does-not-know"}"#,
                at,
            )?;
            let ended = aura_execution_contract::ExecutionResult {
                outcome: aura_execution_contract::ExecutionOutcome::Succeeded,
                side_effect: SideEffectStateV1::NoEffect,
                receipt: aura_execution_contract::ExecutionReceipt {
                    attempt: AttemptRef {
                        operation_id: operation,
                        ordinal: std::num::NonZeroU32::MIN,
                    },
                    evidence_ref: evidence,
                    postcondition_fingerprint: None,
                    rollback: aura_execution_contract::RollbackEvidence::NotApplicable,
                },
                termination: aura_execution_contract::TerminationEvidence::Confirmed {
                    evidence_ref: evidence,
                },
                completed_at: at,
                warnings: Vec::new(),
            };
            aura_persistence_v2::execution::complete(tx, operation, 1, &ended)
        })
        .expect("the call's end is stored");
    store
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::tool::result::materialize(
                tx,
                admitted.batch.tool_batch_id,
                call_id,
            )
            .map(|_| ())
        })
        .expect_err("its result cannot be built");

    store
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::agent::recovery::interrupt(
                tx,
                run,
                "agent.cancelled",
                true,
                UtcTimestamp::now(),
            )
        })
        .expect("停止 ends the turn whatever one call holds");
    let turn = aura_persistence_v2::agent::turns::load(
        store.database.connection(),
        fixture.turn.turn_task_id,
    )
    .expect("turn");
    assert!(turn.state.terminal(), "{:?}", turn.state);
}
