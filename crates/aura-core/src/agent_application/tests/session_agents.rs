use super::*;

#[tokio::test]
async fn session_agent_identity_adapts_existing_conversation_without_creating_another() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::new(false).await;
    let (runtime, session, _) = team(&host, &provider).await;
    let before = sessions(&runtime).await;
    let view = call(&runtime, "agent.team.profile.get", json!({"session_id": session})).await;
    assert_eq!(view["session_id"], json!(session));
    assert_eq!(view["description"], "");
    assert_eq!(view["description_source"], "none");
    assert_eq!(view["revision"], 0);
    assert_eq!(sessions(&runtime).await, before);
    assert!(provider.requests.lock().expect("requests").is_empty());
}

#[tokio::test]
async fn session_agent_user_description_survives_reload_without_renaming_the_conversation() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::new(false).await;
    let (runtime, session, _) = team(&host, &provider).await;
    let before = sessions(&runtime).await;
    let saved = call(
        &runtime,
        "agent.team.profile.set",
        json!({"session_id": session, "expected_revision": 0, "description": "负责回归测试"}),
    )
    .await;
    assert_eq!(saved["description"], "负责回归测试");
    assert_eq!(saved["description_source"], "user");
    assert_eq!(saved["revision"], 1);
    let read = call(&runtime, "agent.team.profile.get", json!({"session_id": session})).await;
    assert_eq!(read, saved);
    assert_eq!(sessions(&runtime).await, before);
}
