use super::*;

fn expected(view: &Value) -> Value {
    json!({"graph_id":view["graph"]["graph_id"],"revision":view["graph"]["revision"]})
}
async fn change(
    runtime: &crate::RuntimeState,
    session: SessionId,
    view: &Value,
    action: Value,
) -> Value {
    call(
        runtime,
        "agent.team.graph.mutate",
        json!({"session_id":session,"expected_graph":expected(view),"action":action}),
    )
    .await
}

#[tokio::test]
async fn session_agent_graph_keeps_both_a1_teams_and_only_removing_a_member_splits_the_canvas() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::Report }).await;
    let (runtime, first, model) = team(&host, &provider).await;
    turn(&runtime, first, model, "请派一个临时 Agent 完成检查").await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    let workspace = services
        .persistence
        .workspace_session_projection()
        .expect("projection")
        .sessions
        .into_iter()
        .find(|s| s.session_id == first)
        .expect("session")
        .workspace_id;
    let second = create_session(&runtime, workspace, PermissionMode::ReadOnly).await;
    let binding = services.persistence.session_model_binding_state(second).expect("binding");
    call(
        &runtime,
        "models.session.set_binding",
        json!({"session_id":second,"model_profile_id":model,"expected_revision":binding.revision}),
    )
    .await;
    session_agent_controls::set_session_policy(&runtime, second, true).await;
    turn(&runtime, second, model, "请派一个临时 Agent 完成另一检查").await;
    let model_calls = provider.requests.lock().expect("requests").len();
    let one = call(&runtime, "agent.team.graph.get", json!({"session_id":first})).await;
    let two = call(&runtime, "agent.team.graph.get", json!({"session_id":second})).await;
    assert_eq!(one["graph"]["sessions"].as_array().expect("singleton").len(), 1);
    let merged = change(&runtime, first, &one, json!({"action":"merge","other_session_id":second,"expected_other":expected(&two),"confirmed":true})).await;
    assert_eq!(merged["graph"]["sessions"].as_array().expect("shared").len(), 2);
    let teams = merged["teams"].as_array().expect("teams");
    assert_eq!(teams.len(), 2);
    let parents = teams.iter().map(|team| team["parent_session_id"].clone()).collect::<Vec<_>>();
    assert!(
        parents.contains(&json!(first)) && parents.contains(&json!(second)),
        "team ownership must be explicit, not positional: {teams:?}"
    );
    let children = teams
        .iter()
        .map(|team| {
            assert_eq!(team["team"]["members"][0]["agent"], "A1");
            team["team"]["members"][0]["session_id"].clone()
        })
        .collect::<Vec<_>>();
    assert_ne!(children[0], children[1]);
    let linked = change(
        &runtime,
        first,
        &merged,
        json!({"action":"connect","source_session_id":first,"target_session_id":second}),
    )
    .await;
    assert_eq!(linked["graph"]["edges"].as_array().expect("edge").len(), 1);
    let placed = change(&runtime, first, &linked, json!({"action":"place","positions":[{"session_id":first,"x":-120,"y":96},{"session_id":children[0],"x":420,"y":220}]})).await;
    let loaded = call(&runtime, "agent.team.graph.get", json!({"session_id":second})).await;
    assert_eq!(loaded["graph"], placed["graph"]);
    let cut = change(
        &runtime,
        first,
        &placed,
        json!({"action":"disconnect","source_session_id":first,"target_session_id":second}),
    )
    .await;
    assert_eq!(cut["graph"]["sessions"].as_array().expect("not split").len(), 2);
    let independent =
        change(&runtime, first, &cut, json!({"action":"remove","session_id":first})).await;
    assert_eq!(independent["graph"]["sessions"], json!([first]));
    assert_eq!(independent["teams"][0]["team"]["members"][0]["agent"], "A1");
    assert_eq!(members(&runtime, first).await.len(), 1);
    assert_eq!(members(&runtime, second).await.len(), 1);
    assert_eq!(
        provider.requests.lock().expect("requests").len(),
        model_calls,
        "graph editing must not invoke a model or dispatch work"
    );
}
