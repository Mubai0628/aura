use super::*;
use aura_core_broker::agent_store::StoreStep;

#[tokio::test]
async fn session_agent_tools_call_cannot_bypass_session_off_or_zero_temporary_limit() {
    for (allowed, max_running) in [(false, 3), (true, 0)] {
        let host = crate::r03_tests::Fixture::new();
        let provider = Provider::scenario(Scenario::Sequence {
            calls: &[
                (
                    "agents.spawn",
                    r#"{"name":"不得启动","task":{"goal":"读代码","done_when":"报告"}}"#,
                ),
                ("agents.status", "{}"),
            ],
        })
        .await;
        let (runtime, session, model) = team_with_model(&host, &provider).await;
        set_session_policy(&runtime, session, allowed).await;
        let current = call(&runtime, "agent.team.settings", json!({})).await;
        let mut limits = current["limits"].clone();
        limits["max_running"] = json!(max_running);
        call(
            &runtime,
            "agent.team.settings.set",
            json!({"enabled":true,"limits":limits,"expected_revision":current["revision"]}),
        )
        .await;
        turn(&runtime, session, model, "验证限制仍生效，不准通过包装调用绕过").await;
        let (_, ours) = requests(&provider);
        assert!(ours.len() >= 3, "spawn refusal, management query, final answer");
        let carried = &ours[1]["messages"];
        assert!(
            carried.as_array().expect("messages").iter().any(|message| {
                message["tool_calls"].as_array().is_some_and(|calls| {
                    calls.iter().any(|call| {
                        call["function"]["name"]
                            .as_str()
                            .is_some_and(|name| name.starts_with("tools_call__"))
                    })
                })
            }),
            "the fixture did not actually call the wrapper: {carried}"
        );
        let outputs = results(ours.last().expect("last"));
        assert!(outputs.len() >= 2);
        assert!(members(&runtime, session).await.is_empty(), "a child started through tools.call");
        assert!(
            outputs[0].contains("unavailable")
                || outputs[0].contains("disabled")
                || outputs[0].contains("no_slot")
                || outputs[0].contains("tool.registry.tool_not_found"),
            "refusal missing: {}",
            outputs[0]
        );
        assert!(outputs[1].contains("succeeded"), "management unavailable: {}", outputs[1]);
    }
}

pub(super) async fn set_session_policy(
    runtime: &crate::RuntimeState,
    session: SessionId,
    allowed: bool,
) {
    let current = call(runtime, "agent.team.policy.get", json!({"session_id":session})).await;
    let saved = call(
        runtime,
        "agent.team.policy.set",
        json!({
            "session_id":session,
            "expected_revision":current["revision"],
            "allow_proactive_temporary_agents":allowed
        }),
    )
    .await;
    assert_eq!(saved["allow_proactive_temporary_agents"], allowed);
}

#[tokio::test]
async fn session_agent_parent_policy_blocks_only_new_temporary_turns_and_preserves_user_pause() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::Report }).await;
    let (runtime, parent, model) = team(&host, &provider).await;
    turn(&runtime, parent, model, "请派一个临时 Agent 检查入口并收回报告").await;
    let children = members(&runtime, parent).await;
    let child: SessionId =
        serde_json::from_value(children[0]["session_id"].clone()).expect("child");
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    let app = services.agent_required();
    app.store
        .write(StoreStep::operation("test.controls.queue"), move |journal| {
            journal.database.unit_of_work(|tx| {
                let at = UtcTimestamp::now();
                aura_persistence_v2::agent::inbox::submit(
                    tx,
                    CommandInvocationId::new(),
                    &AgentSubmitRequest::new(child, "检查后续项", AgentInputLane::NextTurn),
                    at,
                )?;
                aura_persistence_v2::agent::inbox::pause(tx, child, at)
            })
        })
        .await
        .expect("accepted queue, paused by user");
    let pending = app.inbox(AgentSessionRequest { session_id: child }).await.expect("queue");
    assert_eq!(pending.messages.len(), 1);
    set_session_policy(&runtime, parent, false).await;
    set_session_policy(&runtime, parent, true).await;
    let kept = app.inbox(AgentSessionRequest { session_id: child }).await.expect("kept");
    assert!(
        kept.paused && kept.messages.len() == 1,
        "opening the switch must not clear the user's own pause"
    );
    set_session_policy(&runtime, parent, false).await;
    let before = app.inbox(AgentSessionRequest { session_id: child }).await.expect("inbox");
    let resumed = app
        .mutate_inbox(
            CommandInvocationId::new(),
            AgentInboxMutation {
                session_id: child,
                expected_revision: before.revision,
                action: AgentInboxAction::Resume,
            },
        )
        .await
        .expect("remove user pause only");
    assert!(!resumed.paused);
    assert_eq!(
        resumed.messages.len(),
        1,
        "session-off must retain the accepted message instead of starting it"
    );
    assert!(!members(&runtime, parent).await[0]["working"].as_bool().unwrap_or(true));
    turn(&runtime, parent, model, "普通用户对话仍应正常结束").await;
    let (_, ours) = requests(&provider);
    let tools = offered(ours.last().expect("ordinary request"));
    assert!(
        !tools.contains("agents.spawn") && !tools.contains("agents.send"),
        "new work is still offered: {tools:?}"
    );
    for action in ["agents.status", "agents.result", "agents.stop"] {
        assert!(tools.contains(action), "management hidden: {tools:?}");
    }
    set_session_policy(&runtime, parent, true).await;
    let resumed = app.inbox(AgentSessionRequest { session_id: child }).await.expect("resumed");
    assert!(resumed.messages.is_empty(), "opening the switch did not reconsider its paused queue");
}

#[tokio::test]
async fn session_agent_policy_defaults_off_and_changes_only_at_its_revision() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::Report }).await;
    let (runtime, session, model) = team_with_model(&host, &provider).await;
    let current = call(&runtime, "agent.team.policy.get", json!({"session_id":session})).await;
    assert_eq!(current["allow_proactive_temporary_agents"], false);
    assert_eq!(current["revision"], 0);
    let saved = call(
        &runtime,
        "agent.team.policy.set",
        json!({"session_id":session,"expected_revision":0,"allow_proactive_temporary_agents":true}),
    )
    .await;
    assert_eq!(saved["allow_proactive_temporary_agents"], true);
    assert_eq!(saved["revision"], 1);
    turn(&runtime, session, model, "请使用一个临时 Agent 检查入口").await;
    let (_, ours) = requests(&provider);
    let guide = texts(ours.first().expect("actual model request"), "system").join("\n");
    assert!(
        guide.contains("\"allow_proactive_temporary_agents\":true"),
        "missing real policy: {guide}"
    );
}
