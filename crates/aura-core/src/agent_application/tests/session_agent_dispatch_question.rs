use super::*;

#[tokio::test]
async fn session_agent_dispatch_question_accepts_reverse_neighbor_without_waiting() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::PeerDispatch { disconnect: false }).await;
    let (runtime, a, model) = Box::pin(prepare(&host, &provider)).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    let workspace = services
        .persistence
        .workspace_session_projection()
        .expect("projection")
        .sessions
        .into_iter()
        .find(|s| s.session_id == a)
        .expect("A")
        .workspace_id;
    let b = Box::pin(create_session(&runtime, workspace, PermissionMode::ReadOnly)).await;
    let binding = services.persistence.session_model_binding_state(a).expect("binding");
    call(
        &runtime,
        "models.session.set_binding",
        json!({"session_id":a,"model_profile_id":model,"expected_revision":binding.revision}),
    )
    .await;
    Box::pin(connect(&runtime, a, b)).await;
    let view = call(&runtime, "agent.team.graph.get", json!({"session_id":b})).await;
    let held = Box::pin(begin(&runtime, a, model, "B4 neutral hold")).await;
    provider.parked.notified().await;
    Box::pin(turn(
        &runtime,
        b,
        model,
        &format!(
            "B5 ask {}",
            json!({
                "graph_id": view["graph"]["graph_id"], "graph_revision": view["graph"]["revision"],
                "target": a, "question": "B5 example question"
            })
        ),
    ))
    .await;
    let outcomes = tool_outcomes(&provider);
    let receipt = outcomes
        .iter()
        .map(|v| &v["outcome"]["payload"]["value"])
        .find(|v| v["used"] == 1 && v["deliveries"][0]["state"] == "queued")
        .unwrap_or_else(|| {
            panic!("no asynchronous question receipt while A was busy: {outcomes:?}")
        });
    assert_eq!(receipt["deliveries"][0]["target_session_id"], json!(a));
    assert_eq!(receipt["deliveries"][0]["kind"], "question");
    let id: EventId = serde_json::from_value(receipt["deliveries"][0]["delivery_id"].clone())
        .expect("isolated fixture state");
    provider.release_window.notify_one();
    Box::pin(wait(&runtime, &held, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })))
        .await;
    let answer = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let mailbox = call(&runtime, "agent.team.peers.get", json!({"session_id":b})).await;
            let entry = &mailbox["entries"][0];
            assert_eq!(entry["delivery"]["delivery_id"], json!(id));
            assert_eq!(entry["budget"]["used"], 1);
            if entry["delivery"]["state"] == "completed" {
                break mailbox;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("one durable answer");
    assert_eq!(answer["entries"].as_array().expect("isolated fixture state").len(), 1);
    assert_eq!(answer["entries"][0]["delivery"]["result"], "B5 once-only answer");
    let receiver = call(&runtime, "agent.team.peers.get", json!({"session_id":a})).await;
    assert!(receiver["entries"][0]["delivery"]["result"].is_null());
    assert_eq!(
        receiver["entries"].as_array().expect("isolated fixture state").len(),
        1,
        "no reply loop"
    );
}
