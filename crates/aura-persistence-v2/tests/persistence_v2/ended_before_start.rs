//! A call whose turn was stopped in the moment after it was marked started, before anything of it
//! was sent, did not run: its row reads 会话中断，未执行 and the model reads that it was not run,
//! never that its result was lost. So does a call that was reserved but not yet started when its
//! turn stopped, which can never start afterwards, and a file call that waited past its limit for
//! a file another call held.
use crate::cut_off_history::{history, next_start, note, stop};
use crate::support;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_execution_contract::{AttemptState, DispatchRequest, SideEffectStateV1};
use aura_persistence_core::{PersistenceCoreError, StoreResult, artifact};
use aura_persistence_v2::{
    V2Store,
    agent::recovery,
    execution,
    tool::{self, execution::BatchExecution},
};
use aura_tool_contract::{
    ModelVisibleToolResultEnvelopeV1, ToolCallStatusV1, ToolCancellationReasonV1,
    ToolNotExecutedReasonV1, ToolResultOutcomeV1,
};
use aura_tool_projection::{
    AgentToolActivityEntry, AgentToolActivityRequest, ToolResultOutcomeSummaryView,
};

/// What a desktop call says when its turn is gone by the time it is to be sent.
const ENDED: &str = "computer.call_not_active: 这次电脑操控调用在发给桌面之前已经结束（回合停止了或调用被取消），什么都没做。";

fn row(store: &V2Store, f: &support::Fixture, call: ToolCallId) -> AgentToolActivityEntry {
    let request = AgentToolActivityRequest {
        session_id: f.turn.session_id,
        before_call_id: None,
        limit: 10,
        call_ids: Some(vec![call]),
    };
    tool::activity::page(store.database.connection(), &request).expect("rows").calls.remove(0)
}

fn ended(
    store: &mut V2Store,
    f: &support::Fixture,
    batch: &BatchExecution,
    request: &DispatchRequest,
) -> StoreResult<()> {
    store.database.unit_of_work(|tx| {
        recovery::end_before_start(
            tx,
            &batch.batch,
            batch.batch.calls[0].tool_call_id,
            request,
            ENDED,
            f.at,
        )
    })
}

fn started(store: &mut V2Store, f: &support::Fixture, batch: &BatchExecution) -> DispatchRequest {
    store.database.unit_of_work(|tx| f.begin_execution(tx, batch, 0, None)).expect("marked started")
}

#[test]
fn a_call_its_stopped_turn_left_just_after_its_start_mark_did_not_run() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let batch =
        f.admit(&mut store, vec![("computer.click", serde_json::json!({"app": "notepad"}))]);
    let call = batch.batch.calls[0].tool_call_id;
    let request = started(&mut store, &f, &batch);
    // 停止 is recorded first: the run ends with the call unanswered, as one still running.
    let stopped = stop(&mut store, &f.turn, "agent.cancelled", true);
    assert_eq!(row(&store, &f, call).summary.status, ToolCallStatusV1::RecoveryRequired);
    // Then the call reaches its tool, finds its turn gone and sends nothing.
    ended(&mut store, &f, &batch, &request).expect("its end is recorded");

    let result = tool::execution::call(store.database.connection(), call)
        .expect("call")
        .result
        .expect("the call has its result although its run had ended");
    assert_eq!(
        result.outcome,
        ToolResultOutcomeV1::Cancelled { reason: ToolCancellationReasonV1::TurnInterrupted }
    );
    assert_eq!(result.side_effect_state, SideEffectStateV1::NoEffect);
    assert!(result.attempts[0].dispatched, "it had been marked started");
    let visible: ModelVisibleToolResultEnvelopeV1 = serde_json::from_slice(
        &artifact::read(
            store.database.connection(),
            result.model_visible_result_ref.expect("visible"),
            &call.to_string(),
        )
        .expect("artifact"),
    )
    .expect("envelope");
    let text = visible.model_text();
    assert!(
        text.contains("This call was not run: the turn was stopped before it started."),
        "{text}"
    );
    assert!(text.contains("Nothing was changed."), "{text}");

    let entry = row(&store, &f, call);
    assert_eq!(entry.summary.status, ToolCallStatusV1::Finalized);
    assert_eq!(
        entry.summary.outcome,
        Some(ToolResultOutcomeSummaryView::Cancelled {
            reason: ToolCancellationReasonV1::TurnInterrupted
        })
    );
    assert_eq!(entry.summary.side_effect_state, Some(SideEffectStateV1::NoEffect));
    assert_eq!(entry.display.summary, "未执行：会话中断");

    let next = next_start(&mut store, &stopped, "继续");
    let note = note(&history(&store, &next));
    assert!(note.contains(&call.to_string()), "{note}");
    assert!(note.contains("it did not run"), "{note}");
    assert!(!note.contains("its result was lost"), "{note}");
}

#[test]
fn a_call_that_ended_before_the_stop_was_recorded_is_answered_by_the_stop() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let batch =
        f.admit(&mut store, vec![("computer.click", serde_json::json!({"app": "notepad"}))]);
    let call = batch.batch.calls[0].tool_call_id;
    let request = started(&mut store, &f, &batch);
    ended(&mut store, &f, &batch, &request).expect("its end is recorded");
    // Its run still holds the call, so the run's own end gives it its result.
    let attempt =
        execution::load(store.database.connection(), request.binding.attempt.operation_id, 1)
            .expect("attempt");
    assert!(matches!(attempt.state, AttemptState::Terminal { .. }));
    assert!(
        tool::execution::call(store.database.connection(), call).expect("call").result.is_none()
    );

    stop(&mut store, &f.turn, "agent.cancelled", true);
    let result = tool::execution::call(store.database.connection(), call)
        .expect("call")
        .result
        .expect("the stop answered it");
    assert_eq!(
        result.outcome,
        ToolResultOutcomeV1::Cancelled { reason: ToolCancellationReasonV1::TurnInterrupted }
    );
    assert_eq!(row(&store, &f, call).display.summary, "未执行：会话中断");
}

#[test]
fn only_a_started_call_without_an_end_can_end_before_its_start() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let batch =
        f.admit(&mut store, vec![("computer.click", serde_json::json!({"app": "notepad"}))]);
    let request = store
        .database
        .unit_of_work(|tx| f.reserve_execution(tx, &batch, 0, None))
        .expect("reserved");
    assert!(
        matches!(ended(&mut store, &f, &batch, &request), Err(PersistenceCoreError::Conflict)),
        "a call that was not marked started is closed by its refusal, not here"
    );
    store
        .database
        .unit_of_work(|tx| {
            execution::mark_dispatched(tx, request.binding.attempt.operation_id, 1, f.at)
        })
        .expect("marked started");
    ended(&mut store, &f, &batch, &request).expect("its end is recorded");
    assert!(
        matches!(ended(&mut store, &f, &batch, &request), Err(PersistenceCoreError::Conflict)),
        "an end is recorded once"
    );
}

#[test]
fn a_call_reserved_but_not_started_when_its_turn_stopped_did_not_run() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let batch =
        f.admit(&mut store, vec![("computer.click", serde_json::json!({"app": "notepad"}))]);
    let call = batch.batch.calls[0].tool_call_id;
    store
        .database
        .unit_of_work(|tx| f.reserve_execution(tx, &batch, 0, None).map(|_| ()))
        .expect("reserved");
    assert_eq!(row(&store, &f, call).summary.status, ToolCallStatusV1::Running);
    // Once its turn has stopped, its start can never be claimed.
    let stopped = stop(&mut store, &f.turn, "agent.cancelled", true);

    let entry = row(&store, &f, call);
    assert_eq!(
        entry.summary.outcome,
        Some(ToolResultOutcomeSummaryView::NotExecuted {
            reason: ToolNotExecutedReasonV1::CancelledBeforeDispatch
        })
    );
    assert_eq!(entry.summary.status, ToolCallStatusV1::Finalized);
    assert!(!entry.summary.can_cancel);

    let next = next_start(&mut store, &stopped, "继续");
    let note = note(&history(&store, &next));
    assert!(note.contains(&call.to_string()), "{note}");
    assert!(note.contains("it did not run"), "{note}");
    assert!(!note.contains("its result was lost"), "{note}");
}

/// What the file executor says when another call held the call's file past the wait limit.
const WAITED: &str = "agent.resource_wait_exhausted: another execution still held this file \
                      resource after the configured wait budget; nothing was written and the \
                      tool did not start";

#[test]
fn a_call_that_waited_past_the_limit_for_its_file_did_not_run() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let batch = f.admit(&mut store, vec![("fs.read", serde_json::json!({"path": "game.html"}))]);
    let call = batch.batch.calls[0].tool_call_id;
    let request = started(&mut store, &f, &batch);
    store
        .database
        .unit_of_work(|tx| {
            recovery::refused_before_start(tx, &batch.batch, call, &request, WAITED, f.at)
        })
        .expect("its refusal is recorded");
    // Its turn goes on, so its own executor reads the result as usual.
    assert!(
        tool::execution::call(store.database.connection(), call).expect("call").result.is_none()
    );
    let result = store
        .database
        .unit_of_work(|tx| tool::result::materialize(tx, batch.batch.tool_batch_id, call))
        .expect("its result");
    let ToolResultOutcomeV1::OperationRejected { error } = &result.outcome else {
        panic!("it did not run: {:?}", result.outcome);
    };
    assert_eq!(error.code.as_str(), "agent.resource_wait_exhausted");
    assert_eq!(result.side_effect_state, SideEffectStateV1::NoEffect, "never an unknown effect");
    let visible: ModelVisibleToolResultEnvelopeV1 = serde_json::from_slice(
        &artifact::read(
            store.database.connection(),
            result.model_visible_result_ref.expect("visible"),
            &call.to_string(),
        )
        .expect("artifact"),
    )
    .expect("envelope");
    let text = visible.model_text();
    assert!(
        text.contains("another running call held the same file longer than the wait limit"),
        "{text}"
    );
    assert!(text.contains("Try again after that call finishes."), "{text}");
    assert_eq!(row(&store, &f, call).display.summary, "未执行：文件被另一个调用占用");
}
