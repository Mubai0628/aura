//! A new conversation's first request is counted at the rate its model's provider counted another
//! conversation at, instead of a token a byte; a different provider's count is not borrowed.
use super::l10_15_compaction::{first_request_tokens, start};
use super::*;

async fn answered(
    runtime: &crate::RuntimeState,
    workspace: WorkspaceId,
    model: ModelProfileId,
    text: &str,
) -> SessionId {
    let session = create_session(runtime, workspace, PermissionMode::ReadOnly).await;
    let run = start(runtime, session, model, text).await;
    waiting::wait(runtime, &run, |v| matches!(v.turn.state, TurnTaskState::Completed { .. })).await;
    session
}

#[tokio::test]
async fn a_new_conversation_borrows_the_same_models_count_from_another_conversation() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::PlainMeasured).await;
    let runtime = host.start().await;
    let model = profile(&runtime, &provider.url).await;
    // A request no count covers goes byte for byte here, so a borrowed count shows against bytes.
    count_by_bytes(model);
    let root = host.root.path().join("borrowed-count-workspace");
    std::fs::create_dir(&root).expect("workspace folder");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"Borrowed count"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let workspace = workspace.workspace_id;
    let first = answered(&runtime, workspace, model, "AURA-L121-FIRST: answer briefly").await;
    let bytes = first_request_tokens(&runtime, first).await;
    let second = answered(&runtime, workspace, model, "AURA-L121-AGAIN: answer briefly").await;
    let borrowed = first_request_tokens(&runtime, second).await;
    assert!(
        borrowed < bytes / 2,
        "counted near the provider's rate: {borrowed} for a request of about {bytes} bytes"
    );
    let elsewhere = profile_after_existing(&runtime, &provider.url).await;
    count_by_bytes(elsewhere);
    let third = answered(&runtime, workspace, elsewhere, "AURA-L121-OTHER: answer briefly").await;
    let unborrowed = first_request_tokens(&runtime, third).await;
    assert!(
        unborrowed > bytes / 2,
        "another provider's count is not borrowed: {unborrowed} for about {bytes} bytes"
    );
}
