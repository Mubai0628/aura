//! Real Agent batch execution through a common-owner refusal; Provider output alone is a fixture.
use super::*;
use aura_workspace_domain::{CommitRequest, ExecutionId, ResourceAccess};
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_file_resource_wait_keeps_one_attempt_and_finishes_after_release() {
    let f = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let service = f.service().agent_execution_required();
    service.set_global_tool_concurrency(1).expect("one real global execution slot");
    let facts = service
        .workspace
        .resource_facts(f.turn.workspace_id, f.turn.session_id)
        .expect("the Agent workspace is open");
    let owner = service.workspace.eligibility_owner();
    let hold = owner
        .commit(
            &CommitRequest::one(
                facts
                    .slot(&WorkspacePath::parse("result.txt").expect("a relative path"))
                    .expect("the real creation slot"),
                ResourceAccess::Write,
            ),
            f.turn.session_id,
            ExecutionId::next(),
            None,
        )
        .expect("another entry holds this actual target");
    let batch = f.admit(&[(
        "fs.write",
        json!({
            "path":"result.txt", "text":"real effect", "expected":{"expected_kind":"absent"}
        }),
    ), (
        "fs.write",
        json!({"path":"independent.txt", "text":"unrelated effect", "expected":{"expected_kind":"absent"}}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    let commands = f.service().commands_required().clone();
    let (_sender, cancel) = tokio::sync::watch::channel(false);
    let runner = Arc::clone(&service);
    let task = tokio::spawn(async move { runner.execute_batch(batch, commands, cancel).await });
    let attempt = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let attempt = {
                let store = f.service().broker.lock().expect("read the actual journal");
                aura_persistence_v2::tool::execution::call(store.database.connection(), id)
                    .expect("read the tool call")
                    .attempt_id
            };
            if let Some(attempt) = attempt {
                break attempt;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the Agent reserved its original Attempt");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !task.is_finished(),
        "the actual file conflict waited rather than becoming a tool failure"
    );
    assert!(!f.root.join("result.txt").exists());
    tokio::time::timeout(Duration::from_secs(2), async {
        while !matches!(std::fs::read(f.root.join("independent.txt")), Ok(bytes) if bytes == b"unrelated effect") {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("a real unrelated Agent call can use the only global slot while the first waits");
    assert_eq!(
        std::fs::read(f.root.join("independent.txt")).expect("independent physical effect"),
        b"unrelated effect"
    );
    tokio::time::timeout(Duration::from_secs(1), async {
        while service.global_tool_capacity() != (1, 0) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("resource wait does not retain executing capacity");
    {
        let store = f.service().broker.lock().expect("read the waiting Attempt");
        let call = aura_persistence_v2::tool::execution::call(store.database.connection(), id)
            .expect("the original call exists");
        assert_eq!(call.attempt_id, Some(attempt));
        assert!(call.result.is_none(), "admission refusal has no terminal tool result");
    }
    owner.release_commit(hold);
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(4), task)
            .await
            .expect("the released request finishes inside its wait budget")
            .expect("the Agent task did not panic")
            .expect("the real batch settled"),
        BatchOutcome::Completed(_)
    ));
    assert_eq!(
        std::fs::read(f.root.join("result.txt")).expect("the real file effect"),
        b"real effect"
    );
    let store = f.service().broker.lock().expect("read the real terminal journal");
    let call = aura_persistence_v2::tool::execution::call(store.database.connection(), id)
        .expect("the original call after completion");
    assert_eq!(
        call.attempt_id,
        Some(attempt),
        "retrying pre-effect admission preserves the original Attempt"
    );
    assert!(matches!(
        call.result.expect("one terminal result").outcome,
        ToolResultOutcomeV1::Succeeded
    ));
    assert_eq!(owner.snapshot().commits, 0);
}
