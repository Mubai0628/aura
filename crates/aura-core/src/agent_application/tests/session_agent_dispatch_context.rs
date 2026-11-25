use super::*;
use aura_domain_common::ContentHash;

fn chat_context(request: &Value) -> (&str, &str, Value) {
    let first = &request["messages"][0];
    assert_eq!(first["role"], "system");
    let text = first["content"].as_str().expect("OpenAI Chat system message");
    // Chat puts the per-turn checkpoint at the end of its system message, after the
    // reusable instructions. Other adapters need not serialize those sections together.
    let (prefix, checkpoint) = text.split_once("<Current-Task-State>\n").expect("task checkpoint");
    let (checkpoint, suffix) =
        checkpoint.split_once("</Current-Task-State>").expect("closed task checkpoint");
    let mut values = serde_json::Deserializer::from_str(checkpoint).into_iter::<Value>();
    (prefix, suffix, values.next().expect("checkpoint JSON").expect("valid checkpoint"))
}

#[tokio::test]
async fn session_agent_dispatch_context_comparison_without_automatic_delegation() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::PeerDispatch { disconnect: false }).await;
    let (runtime, a, model) = Box::pin(prepare(&host, &provider)).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    let workspace = services
        .persistence
        .workspace_session_projection()
        .expect("isolated fixture state")
        .sessions
        .into_iter()
        .find(|s| s.session_id == a)
        .expect("isolated fixture state")
        .workspace_id;
    let b = Box::pin(create_session(&runtime, workspace, PermissionMode::ReadOnly)).await;
    Box::pin(turn(&runtime, a, model, "B7 unlinked comparison")).await;
    let unlinked = request_with(&provider, "B7 unlinked comparison").await;
    assert!(!unlinked.to_string().contains("peer_collaboration"));
    Box::pin(connect(&runtime, a, b)).await;
    Box::pin(turn(&runtime, a, model, "B7 linked comparison")).await;
    let linked = request_with(&provider, "B7 linked comparison").await;
    assert!(linked.to_string().contains("peer_collaboration"));
    let profile = call(&runtime, "agent.team.profile.get", json!({"session_id":b})).await;
    call(&runtime, "agent.team.profile.set", json!({"session_id":b,"expected_revision":profile["revision"],"description":"B7 revised reviewing role"})).await;
    Box::pin(turn(&runtime, a, model, "B7 revised comparison")).await;
    let revised = request_with(&provider, "B7 revised comparison").await;
    let (linked_prefix, linked_suffix, linked_task) = chat_context(&linked);
    let (revised_prefix, revised_suffix, revised_task) = chat_context(&revised);
    let peer =
        |task: &Value| task["peer_collaboration"]["directory"]["preview"]["peers"][0].clone();
    let revised_peer = peer(&revised_task);
    assert_eq!(revised_peer["session_id"], json!(b));
    assert_eq!(peer(&linked_task)["session_id"], revised_peer["session_id"]);
    assert_eq!(revised_peer["description"], "B7 revised reviewing role");
    assert_ne!(peer(&linked_task)["profile_revision"], revised_peer["profile_revision"]);
    assert!(!revised_prefix.contains("B7 revised reviewing role"));
    assert_eq!(linked_prefix, revised_prefix, "profile change must not rewrite the fixed prefix");
    assert_eq!(linked_suffix, revised_suffix, "no instruction changed outside the task checkpoint");
    let mailbox = call(&runtime, "agent.team.peers.get", json!({"session_id":a})).await;
    assert_eq!(mailbox["entries"], json!([]), "a link did not delegate automatically");
    println!(
        "B7 loopback OpenAI Chat serialized_request_bytes: unlinked={} linked={} profile_changed={}; fixed_prefix_bytes={} fixed_prefix_hash={:?}; external_cache_tokens=not_measured",
        serde_json::to_vec(&unlinked).expect("request").len(),
        serde_json::to_vec(&linked).expect("request").len(),
        serde_json::to_vec(&revised).expect("request").len(),
        revised_prefix.len(),
        ContentHash::sha256(revised_prefix.as_bytes()),
    );
}
