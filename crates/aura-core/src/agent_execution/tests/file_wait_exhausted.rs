//! A file call that waits past its limit for a file another call holds did not run. While it
//! waits its row says what it waits for; then the row says it did not run and the model reads
//! why. Its result is never an unknown one.
use super::*;
use aura_workspace_domain::{CommitRequest, ExecutionId, ResourceAccess};
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_call_that_waited_past_its_limit_did_not_run_and_says_why() {
    let f = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let service = f.service().agent_execution_required();
    let facts = service
        .workspace
        .resource_facts(f.turn.workspace_id, f.turn.session_id)
        .expect("the Agent workspace is open");
    let owner = service.workspace.eligibility_owner();
    let hold = owner
        .commit(
            &CommitRequest::one(
                facts
                    .slot(&WorkspacePath::parse("held.txt").expect("a relative path"))
                    .expect("the real creation slot"),
                ResourceAccess::Write,
            ),
            f.turn.session_id,
            ExecutionId::next(),
            None,
        )
        .expect("another entry holds this target");
    let batch = f.admit(&[(
        "fs.write",
        json!({"path":"held.txt", "text":"never", "expected":{"expected_kind":"absent"}}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    let commands = f.service().commands_required().clone();
    let (_sender, cancel) = tokio::sync::watch::channel(false);
    let runner = Arc::clone(&service);
    let task = tokio::spawn(async move { runner.execute_batch(batch, commands, cancel).await });

    let agent = f.service().agent().expect("the Agent application");
    let rows = || aura_tool_projection::AgentToolActivityRequest {
        session_id: f.turn.session_id,
        before_call_id: None,
        limit: 10,
        call_ids: Some(vec![id]),
    };
    let waiting = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let page = agent.tool_activity(rows()).await.expect("rows");
            if let Some(entry) = page.calls.into_iter().find(|entry| {
                entry.summary.status == aura_tool_contract::ToolCallStatusV1::WaitingResource
            }) {
                break entry;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("while it waits, its row says it waits");
    assert_eq!(waiting.display.summary, "在等另一个调用用完这个文件");

    // The file stays held past the wait budget.
    tokio::time::timeout(Duration::from_secs(60), task)
        .await
        .expect("the call ends once its wait budget is spent")
        .expect("the Agent task did not panic")
        .expect("the batch settled");
    owner.release_commit(hold);
    assert!(!f.root.join("held.txt").exists(), "nothing was written");
    let result = {
        let store = f.service().broker.lock().expect("read the journal");
        aura_persistence_v2::tool::execution::call(store.database.connection(), id)
            .expect("the call")
            .result
            .expect("the call has its result")
    };
    assert_eq!(
        result.side_effect_state,
        aura_execution_contract::SideEffectStateV1::NoEffect,
        "never an unknown effect"
    );
    let ToolResultOutcomeV1::OperationRejected { error } = &result.outcome else {
        panic!("it did not run: {:?}", result.outcome);
    };
    assert_eq!(error.code.as_str(), "agent.resource_wait_exhausted");
    assert!(error.message.contains("held the same file longer than the wait limit"), "{error:?}");
    let page = agent.tool_activity(rows()).await.expect("rows");
    assert_eq!(page.calls[0].display.summary, "未执行：文件被另一个调用占用");
}
