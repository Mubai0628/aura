use super::*;

#[tokio::test]
async fn s07_n9_post_timeout_is_unknown_and_is_sent_exactly_once() {
    let server = Server::start().await;
    let f = fixture(PermissionMode::WorkspaceReadWrite, &["http.request"]).await;
    let batch=f.admit(&[("http.request",json!({"url":format!("{}/timeout",server.url),"method":"POST","body":{"kind":"text","text":"one mutation"}}))]);
    let id = batch.batch.calls[0].tool_call_id;
    tokio::time::timeout(std::time::Duration::from_secs(40), f.execute_rejecting(batch))
        .await
        .expect("the real request timeout is bounded");
    let store = f.service().broker.lock().expect("durable timeout outcome");
    let call = aura_persistence_v2::tool::execution::call(store.database.connection(), id)
        .expect("paired timeout result");
    let result = call.result.expect("unknown remote effect has a terminal envelope");
    assert!(matches!(result.outcome, ToolResultOutcomeV1::Indeterminate { .. }));
    assert_eq!(result.side_effect_state, aura_execution_contract::SideEffectStateV1::Unknown);
    assert_eq!(server.count(), 1, "a possibly sent POST must never retry");
}

#[tokio::test]
async fn s07_n9_cancelling_a_real_download_reaps_staging_before_acknowledging_stop() {
    let server = Server::start().await;
    let f = Fixture::with_exposure_setup(
        PermissionMode::WorkspaceReadWrite,
        &["http.download"],
        |root| {
            std::fs::write(root.join("cancelled.txt"), "original").expect("previous target");
        },
    )
    .await;
    let batch = f.admit(&[(
        "http.download",
        json!({"url":format!("{}/slow-body",server.url),"path":"cancelled.txt","overwrite":true}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    let (cancel, receiver) = tokio::sync::watch::channel(false);
    let stop = async {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while server.count() == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the real download starts before stop");
        cancel.send_replace(true);
    };
    let engine = f.service().agent_execution_required();
    let execute = engine.execute_batch(batch, f.service().commands_required().clone(), receiver);
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        tokio::join!(execute, stop)
    })
    .await
    .expect("stop is acknowledged within its budget");
    result.expect("the cancelled batch has a paired terminal response");
    assert_eq!(
        std::fs::read_to_string(f.root.join("cancelled.txt")).expect("old target"),
        "original"
    );
    assert!(
        std::fs::read_dir(&f.root).expect("workspace after stop").all(|item| !item
            .expect("directory item")
            .file_name()
            .to_string_lossy()
            .starts_with(".aura-download-")),
        "stop must acknowledge only after temporary objects are gone"
    );
    let store = f.service().broker.lock().expect("cancel result");
    let call = aura_persistence_v2::tool::execution::call(store.database.connection(), id)
        .expect("download call");
    assert!(call.attempt_id.is_some());
    assert!(matches!(
        call.result.expect("paired stopped outcome").outcome,
        ToolResultOutcomeV1::Cancelled { .. }
    ));
}
