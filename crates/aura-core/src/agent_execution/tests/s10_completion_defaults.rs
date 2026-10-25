//! A completion call may leave out its optional fields.
//!
//! In an evaluation baseline, two runs called normal.complete with only the required fields.
//! The commit compared the stored arguments with the re-serialized input, which had gained
//! `"disclosed_limitations": []`, and refused the valid call as a revision or identity conflict.
//! The model saw only `not_executed / batch_rejected`.
use super::*;

#[tokio::test]
async fn normal_complete_without_its_optional_limitations_completes_the_turn() {
    let f = Fixture::new(PermissionMode::ReadOnly).await;
    let batch = f.admit(&[(
        "normal.complete",
        json!({"lineage_id":f.turn.lineage_id,"expected_lineage_revision":Revision::INITIAL,
            "final_report":"Finished by model decision"}),
    )]);
    let group = f.execute(batch).await;
    let store = f.service().broker.lock().expect("persisted completion");
    let db = store.database.connection();
    let call = aura_persistence_v2::tool::execution::call(db, group.calls[0].tool_call_id)
        .expect("persisted completion call");
    let outcome = call.result.expect("persisted completion result").outcome;
    assert!(matches!(outcome, ToolResultOutcomeV1::Succeeded), "{outcome:?}");
    assert!(
        aura_persistence_v2::agent::turns::load(db, f.turn.turn_task_id)
            .expect("completed turn")
            .state
            .terminal()
    );
}

/// A call Core refuses before it reaches any backend tells the model so, instead of a bare
/// `not_executed / batch_rejected`.
#[tokio::test]
async fn a_completion_refused_before_it_runs_says_so_and_that_nothing_changed() {
    let f = Fixture::new(PermissionMode::ReadOnly).await;
    // A lineage this turn does not own: the completion is refused before anything is written.
    let other = aura_domain_common::TaskLineageId::new();
    let batch = f.admit(&[(
        "normal.complete",
        json!({"lineage_id":other,"expected_lineage_revision":Revision::INITIAL,
            "final_report":"Finished","disclosed_limitations":[]}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute_rejecting(batch).await;
    let store = f.service().broker.lock().expect("persisted refusal");
    let db = store.database.connection();
    let result = aura_persistence_v2::tool::execution::call(db, id)
        .expect("persisted refused call")
        .result
        .expect("persisted refusal result");
    assert_eq!(result.side_effect_state, SideEffectStateV1::NotStarted);
    let ToolResultOutcomeV1::NotExecuted { error: Some(error), .. } = result.outcome else {
        panic!("a refusal before dispatch must say why: {:?}", result.outcome)
    };
    assert_eq!(error.code.as_str(), "tool.runtime.dispatch_failed");
    assert!(error.message.contains("refused this call before running it"), "{}", error.message);
    assert!(error.message.contains("Nothing was changed"), "{}", error.message);
    assert!(
        !aura_persistence_v2::agent::turns::load(db, f.turn.turn_task_id)
            .expect("turn still open")
            .state
            .terminal()
    );
}
