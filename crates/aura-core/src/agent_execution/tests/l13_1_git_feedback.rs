//! A Git call in a folder that is no repository
//! is refused while it is planned; the model is told why and what to do.
use super::*;
/// It used to be refused with only "the target could not be determined".
#[tokio::test]
async fn a_git_call_outside_a_repository_says_the_folder_is_not_one() {
    let f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, &["git.status"]).await;
    let batch = f.admit(&[("git.status", json!({}))]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute_rejecting(batch).await;
    let store = f.service().broker.lock().expect("persisted refusal");
    let result = aura_persistence_v2::tool::execution::call(store.database.connection(), id)
        .expect("persisted refused call")
        .result
        .expect("persisted refusal result");
    let ToolResultOutcomeV1::NotExecuted { error: Some(error), .. } = result.outcome else {
        panic!("a refusal before dispatch must say why: {:?}", result.outcome)
    };
    assert!(error.message.contains("git.not_repository"), "{}", error.message);
    assert!(error.message.contains("不是 Git 仓库"), "{}", error.message);
    assert!(error.message.contains("git init"), "what to do next: {}", error.message);
}
