use super::*;
use aura_core_broker::agent_store::StoreStep;

#[tokio::test]
async fn session_agent_close_between_preflight_and_transaction_preserves_unstarted_work() {
    for (enabled, max_running) in [(false, 3), (true, 0)] {
        let host = crate::r03_tests::Fixture::new();
        let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::Report }).await;
        let (runtime, parent, model) = team(&host, &provider).await;
        turn(&runtime, parent, model, "请使用临时 Agent 检查入口").await;
        let children = members(&runtime, parent).await;
        let child: SessionId =
            serde_json::from_value(children[0]["session_id"].clone()).expect("child");
        let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
        let app = services.agent_required().clone();
        app.store
            .write(StoreStep::operation("test.controls.race.queue"), move |journal| {
                journal.database.unit_of_work(|tx| {
                    let at = UtcTimestamp::now();
                    aura_persistence_v2::agent::inbox::submit(
                        tx,
                        CommandInvocationId::new(),
                        &AgentSubmitRequest::new(
                            child,
                            "已接受但还未开始的工作",
                            AgentInputLane::NextTurn,
                        ),
                        at,
                    )?;
                    aura_persistence_v2::agent::inbox::pause(tx, child, at)
                })
            })
            .await
            .expect("queue");
        let before = app.inbox(AgentSessionRequest { session_id: child }).await.expect("inbox");
        let (entered, release) = app.pause_next_temporary_start_for_test(child);
        let runner = app.clone();
        let start = tokio::spawn(async move {
            runner
                .mutate_inbox(
                    CommandInvocationId::new(),
                    AgentInboxMutation {
                        session_id: child,
                        expected_revision: before.revision,
                        action: AgentInboxAction::Resume,
                    },
                )
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(30), entered.notified())
            .await
            .expect("preflight passed");
        let current = call(&runtime, "agent.team.settings", json!({})).await;
        if !enabled {
            super::session_agent_controls::set_session_policy(&runtime, parent, false).await;
        }
        let mut limits = current["limits"].clone();
        limits["max_running"] = json!(max_running);
        call(
            &runtime,
            "agent.team.settings.set",
            json!({"limits":limits,"expected_revision":current["revision"]}),
        )
        .await;
        release.notify_one();
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(30), start)
            .await
            .expect("start settled")
            .expect("task")
            .expect("temporary pause");
        assert_eq!(outcome.messages.len(), 1, "the stale preflight started a new temporary turn");
        assert!(!outcome.paused, "temporary control must not change the ordinary queue pause");
        assert!(!members(&runtime, parent).await[0]["working"].as_bool().unwrap_or(true));
    }
}
