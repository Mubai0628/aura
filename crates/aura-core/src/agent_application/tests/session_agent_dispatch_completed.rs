use super::*;

#[tokio::test]
async fn session_agent_dispatch_completed_receipt_survives_core_restart() {
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
    let binding = services.persistence.session_model_binding_state(b).expect("binding");
    call(
        &runtime,
        "models.session.set_binding",
        json!({"session_id":b,"model_profile_id":model,"expected_revision":binding.revision}),
    )
    .await;
    Box::pin(connect(&runtime, a, b)).await;
    Box::pin(turn(&runtime, a, model, "B4 user authorizes a summary for B")).await;
    request_with(&provider, "B4 peer: summarize only").await;
    let receipt = tool_outcomes(&provider)
        .into_iter()
        .map(|v| v["outcome"]["payload"]["value"].clone())
        .find(|v| v["used"] == 1)
        .expect("delivery receipt");
    let delivery: EventId =
        serde_json::from_value(receipt["deliveries"][0]["delivery_id"].clone()).expect("id");
    let run = services
        .agent_required()
        .store
        .read(StoreStep::operation("test.peer.completed_claim"), move |db| {
            Ok(aura_agent_collaboration::dispatch::load(db, delivery)?
                .expect("delivery")
                .target_run
                .expect("claim"))
        })
        .await
        .expect("claim read");
    let root = services
        .agent_required()
        .store
        .read(StoreStep::operation("test.peer.source"), move |db| {
            Ok(aura_agent_collaboration::dispatch::load(db, delivery)?
                .expect("delivery")
                .origin
                .root_event_id)
        })
        .await
        .expect("source root");
    let request = AgentRunRequest { session_id: b, agent_run_id: run };
    Box::pin(wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })))
        .await;
    let activity = call(&runtime, "agent.sessions.activity", json!({})).await;
    let notice = activity["peer_results"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["delivery_id"] == json!(delivery)))
        .expect("the requestor sees its result once");
    assert_eq!(notice["session_id"], json!(a));
    assert_eq!(notice["target_session_id"], json!(b));
    assert_eq!(notice["state"], "completed");
    call(
        &runtime,
        "agent.team.peers.manage",
        json!({"session_id":a,"action":{"kind":"mark_result_seen","delivery_id":delivery}}),
    )
    .await;
    let cleared = call(&runtime, "agent.sessions.activity", json!({})).await;
    assert!(
        cleared["peer_results"]
            .as_array()
            .is_none_or(|items| items.iter().all(|item| item["delivery_id"] != json!(delivery))),
        "opening the receipt clears only the prompt"
    );
    let before = call(&runtime, "agent.team.graph.get", json!({"session_id":a})).await;
    assert_eq!(before["peer_activity"][0]["state"], "completed");
    let runtime = restart(&host, runtime).await;
    let after = call(&runtime, "agent.team.graph.get", json!({"session_id":b})).await;
    assert_eq!(after["graph"], before["graph"]);
    assert_eq!(after["peer_activity"], before["peer_activity"]);
    let history = call(
        &runtime,
        "agent.history",
        json!({"session_id":b,"before_turn_task_id":null,"within_turn":null,"limit":10}),
    )
    .await;
    let input = &history["turns"][0]["peer_input"];
    assert_eq!(input["delivery_id"], json!(delivery));
    assert_eq!(input["source_session_id"], json!(a));
    assert_eq!(input["text"], "B4 peer: summarize only");
    assert_eq!(history["turns"][0]["user_messages"], json!([]));
    Box::pin(turn(&runtime, a, model, &format!("B4 reconcile {delivery}"))).await;
    let settled = tool_outcomes(&provider)
        .into_iter()
        .map(|v| v["outcome"]["payload"]["value"]["deliveries"][0].clone())
        .find(|v| v["state"] == "completed")
        .expect("the persisted result reached A");
    assert_eq!(settled["target_run_id"], json!(run));
    assert_eq!(settled["result"], "B4 summary result");
    Box::pin(turn(
        &runtime,
        a,
        model,
        &format!("B5 source {}", json!({"delivery_id":delivery,"event_id":root})),
    ))
    .await;
    let source = tool_outcomes(&provider)
        .into_iter()
        .map(|v| v["outcome"]["payload"]["value"].clone())
        .find(|v| v["delivery_id"] == json!(delivery) && v["event_id"] == json!(root))
        .expect("the source read returned the authorized fragment");
    assert!(
        source["text"]
            .as_str()
            .is_some_and(|text| text.contains("B4 user authorizes a summary for B"))
    );
    assert_eq!(source["complete"], true);
}
