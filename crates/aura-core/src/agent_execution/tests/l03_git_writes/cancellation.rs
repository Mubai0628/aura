use super::*;

#[tokio::test]
async fn cancellation_during_commit_records_unknown_and_repeated_recovery_does_not_replay() {
    let f = Fixture::with_exposure_setup(PermissionMode::WorkspaceReadWrite, &["git.commit"], |root| {
        setup(root);
        std::fs::write(root.join("selected.txt"), b"changed\n").expect("change");
        std::fs::write(root.join(".git/hooks/pre-commit"),
            b"#!/bin/sh\nprintf 'run\\n' >>hook.calls\nprintf started >hook.started\nsleep 30\nprintf finished >hook.finished\n").expect("owned hook");
    }).await;
    let batch = f
        .admit(&[("git.commit", json!({"message":"cancel this commit","paths":["selected.txt"]}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let operation = batch.calls[&id].operation.operation_id;
    let (sender, cancel) = tokio::sync::watch::channel(false);
    let engine = f.service().agent_execution_required().clone();
    let commands = f.service().commands_required().clone();
    let worker = tokio::spawn(async move { engine.execute_batch(batch, commands, cancel).await });
    let started = tokio::time::timeout(Duration::from_secs(15), async {
        while !f.root.join("hook.started").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    sender.send(true).expect("stop request");
    let finished = tokio::time::timeout(Duration::from_secs(10), worker)
        .await
        .expect("bounded native stop")
        .expect("worker")
        .expect("durable cancellation result");
    assert!(started.is_ok(), "hook reached the actual Native process");
    assert!(matches!(finished, BatchOutcome::Unknown { .. } | BatchOutcome::Completed(_)));
    assert!(matches!(result(&f, id).outcome, ToolResultOutcomeV1::Indeterminate { .. }));
    assert_eq!(result(&f, id).side_effect_state, SideEffectStateV1::Unknown);
    assert!(!f.root.join("hook.finished").exists());
    let engine = f.service().agent_execution_required();
    let recovered = engine.resume_batch(batch_id).expect("query the persisted batch");
    let (_fresh_sender, fresh_cancel) = tokio::sync::watch::channel(false);
    engine
        .execute_batch(recovered, f.service().commands_required().clone(), fresh_cancel)
        .await
        .expect("recovery returns the durable result");
    let store = f.service().broker.lock().expect("attempt ledger");
    let count: i64 = store
        .database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM execution_attempts WHERE operation_id=?1",
            [operation.to_string()],
            |row| row.get(0),
        )
        .expect("one durable attempt");
    assert_eq!(count, 1);
    assert_eq!(std::fs::read(f.root.join("hook.calls")).expect("hook runs"), b"run\n");
}
