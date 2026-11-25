use super::*;

#[tokio::test]
async fn session_agent_dispatch_distinguishes_legacy_empty_queue_and_user_pause() {
    for manually_paused in [false, true] {
        let host = crate::r03_tests::Fixture::new();
        let provider = Provider::scenario(Scenario::PeerDispatch { disconnect: false }).await;
        let (runtime, a, model) = Box::pin(prepare(&host, &provider)).await;
        let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
        let workspace = services
            .persistence
            .workspace_session_projection()
            .expect("sessions")
            .sessions
            .into_iter()
            .find(|row| row.session_id == a)
            .expect("sender")
            .workspace_id;
        let b = Box::pin(create_session(&runtime, workspace, PermissionMode::ReadOnly)).await;
        let binding = services.persistence.session_model_binding_state(b).expect("binding");
        call(
            &runtime,
            "models.session.set_binding",
            json!({
                "session_id":b, "model_profile_id":model, "expected_revision":binding.revision,
            }),
        )
        .await;
        Box::pin(connect(&runtime, a, b)).await;
        let app = services.agent_required();
        app.store
            .write(StoreStep::operation("test.peer.empty_pause"), move |journal| {
                use aura_persistence_v2::agent::inbox;
                journal.database.unit_of_work(|tx| {
                    let at = UtcTimestamp::now();
                    if manually_paused {
                        inbox::mutate(
                            tx,
                            CommandInvocationId::new(),
                            &AgentInboxMutation {
                                session_id: b,
                                expected_revision: inbox::load(tx, b)?.revision,
                                action: AgentInboxAction::Pause,
                            },
                            at,
                        )?;
                    }
                    // The older startup discarded the cause, but the user's command still has a receipt.
                    inbox::pause(tx, b, at)
                })
            })
            .await
            .expect("legacy pause state");
        Box::pin(turn(&runtime, a, model, "B4 user authorizes a summary for B")).await;
        let receipt = tool_outcomes(&provider)
            .into_iter()
            .map(|result| result["outcome"]["payload"]["value"].clone())
            .find(|value| value["used"] == 1)
            .expect("accepted delivery");
        let delivery = receipt["deliveries"][0]["delivery_id"].clone();
        if manually_paused {
            let queue = call(
                &runtime,
                "agent.team.peers.get",
                json!({
                    "session_id":b, "view":"active",
                }),
            )
            .await;
            let item = &queue["entries"][0]["delivery"];
            assert_eq!(item["delivery_id"], delivery);
            assert_eq!(item["state"], "queued");
            assert_eq!(item["reason"], "目标会话的输入队列已暂停，可在目标会话恢复。");
            let inbox =
                app.inbox(AgentSessionRequest { session_id: b }).await.expect("retained pause");
            app.mutate_inbox(
                CommandInvocationId::new(),
                AgentInboxMutation {
                    session_id: b,
                    expected_revision: inbox.revision,
                    action: AgentInboxAction::Resume,
                },
            )
            .await
            .expect("explicit resume");
        }
        request_with(&provider, "B4 peer: summarize only").await;
        assert!(!app.inbox(AgentSessionRequest { session_id: b }).await.expect("inbox").paused);
        let entries = call(
            &runtime,
            "agent.team.peers.get",
            json!({
                "session_id":a, "view":"settings",
            }),
        )
        .await;
        assert_eq!(entries["entries"], json!([]), "settings never returns dispatch history");
    }
}

#[tokio::test]
async fn session_agent_dispatch_recovers_at_deadline_without_activity_after_restart() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::PeerDispatch { disconnect: false }).await;
    let (runtime, a, model) = Box::pin(prepare(&host, &provider)).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    let workspace = services
        .persistence
        .workspace_session_projection()
        .expect("workspace")
        .sessions
        .into_iter()
        .find(|row| row.session_id == a)
        .expect("A")
        .workspace_id;
    let b = Box::pin(create_session(&runtime, workspace, PermissionMode::ReadOnly)).await;
    Box::pin(connect(&runtime, a, b)).await;
    Box::pin(turn(&runtime, a, model, "B4 user authorizes a summary for B")).await;
    let receipt = tool_outcomes(&provider)
        .into_iter()
        .map(|x| x["outcome"]["payload"]["value"].clone())
        .find(|v| v["used"] == 1)
        .expect("one durable acceptance");
    let delivery: EventId =
        serde_json::from_value(receipt["deliveries"][0]["delivery_id"].clone()).expect("delivery");
    let original = services.agent_required().store.write(StoreStep::operation("test.peer.extend_retry"), move |journal| {
        journal.database.unit_of_work(|tx| {
            let change = tx.execute("UPDATE agent_collaboration_retry SET next_at=strftime('%Y-%m-%dT%H:%M:%fZ','now','+8 seconds') WHERE delivery_id=?1 AND last_reason='model_unavailable'", [delivery.to_string()])?;
            assert_eq!(change, 1, "real first retry persisted; only extend its deadline for the restart");
            Ok(aura_agent_collaboration::dispatch::load(tx, delivery)?.expect("delivery"))
        })
    }).await.expect("fixture deadline");
    let binding = services.persistence.session_model_binding_state(b).expect("model binding");
    call(
        &runtime,
        "models.session.set_binding",
        json!({"session_id":b,"model_profile_id":model,"expected_revision":binding.revision}),
    )
    .await;
    let runtime = Box::pin(host.restart(runtime)).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else {
        panic!("ready after restart")
    };
    let app = services.agent_required();
    assert!(
        !app.store
            .read(StoreStep::operation("test.peer.before_due"), move |db| {
                aura_agent_collaboration::dispatch::retry_due(db, delivery)
            })
            .await
            .expect("deadline"),
        "Core restarted while the persisted delay was still in effect"
    );
    let run = tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            let row = app
                .store
                .read(StoreStep::operation("test.peer.observe_only"), move |db| {
                    aura_agent_collaboration::dispatch::load(db, delivery)
                })
                .await
                .expect("read-only observation")
                .expect("same delivery");
            assert_eq!(row.sender.tool_call_id, original.sender.tool_call_id);
            if let Some(run) = row.target_run {
                break run;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!("{delivery} remained queued past its durable deadline without an activity request")
    });
    Box::pin(wait(&runtime, &AgentRunRequest { session_id: b, agent_run_id: run }, |p| {
        matches!(p.turn.state, TurnTaskState::Completed { .. })
    }))
    .await;
    let (attempts, used) = app
        .store
        .read(StoreStep::operation("test.peer.recovered_receipt"), move |db| {
            use aura_agent_collaboration::dispatch;
            Ok((dispatch::retry_attempts(db, delivery)?, dispatch::budget(db, &original.origin)?.0))
        })
        .await
        .expect("one settled receipt");
    assert_eq!((attempts, used), (0, 1));
    eprintln!(
        "{delivery} auto-claimed {run} after re-open and one persisted model retry; no activity requests"
    );
}

#[tokio::test]
async fn session_agent_dispatch_activity_wakes_a_queued_peer_after_its_model_is_bound() {
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
        .find(|session| session.session_id == a)
        .expect("A")
        .workspace_id;
    let b = Box::pin(create_session(&runtime, workspace, PermissionMode::ReadOnly)).await;
    assert!(
        services.persistence.session_model_binding_state(b).is_ok(),
        "the receiver starts without a model binding"
    );
    Box::pin(connect(&runtime, a, b)).await;
    Box::pin(turn(&runtime, a, model, "B4 user authorizes a summary for B")).await;
    let receipt = tool_outcomes(&provider)
        .into_iter()
        .map(|value| value["outcome"]["payload"]["value"].clone())
        .find(|value| value["used"] == 1)
        .expect("delivery receipt");
    let delivery: EventId =
        serde_json::from_value(receipt["deliveries"][0]["delivery_id"].clone()).expect("id");
    assert!(
        services
            .agent_required()
            .store
            .read(StoreStep::operation("test.peer.recovery.unclaimed"), move |db| {
                Ok(aura_agent_collaboration::dispatch::load(db, delivery)?
                    .expect("delivery")
                    .target_run
                    .is_none())
            })
            .await
            .expect("unclaimed"),
        "without a model the accepted delivery stays queued"
    );
    call(&runtime, "agent.sessions.activity", json!({})).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let attempts = services
        .agent_required()
        .store
        .read(StoreStep::operation("test.peer.recovery.attempts"), move |db| {
            aura_agent_collaboration::dispatch::retry_attempts(db, delivery)
        })
        .await
        .expect("attempt count");
    assert!(attempts >= 1, "the missing model was recorded as a bounded retry");
    let binding = services.persistence.session_model_binding_state(b).expect("binding state");
    call(
        &runtime,
        "models.session.set_binding",
        json!({"session_id":b,"model_profile_id":model,"expected_revision":binding.revision}),
    )
    .await;
    call(&runtime, "agent.sessions.activity", json!({})).await;
    let run = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            call(&runtime, "agent.sessions.activity", json!({})).await;
            let claimed = services
                .agent_required()
                .store
                .read(StoreStep::operation("test.peer.recovery.claim"), move |db| {
                    Ok(aura_agent_collaboration::dispatch::load(db, delivery)?
                        .expect("delivery")
                        .target_run)
                })
                .await
                .expect("claim");
            if let Some(run) = claimed {
                break run;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the activity wake claimed the delivery after binding");
    Box::pin(wait(&runtime, &AgentRunRequest { session_id: b, agent_run_id: run }, |projection| {
        matches!(projection.turn.state, TurnTaskState::Completed { .. })
    }))
    .await;
    let result = services
        .agent_required()
        .store
        .read(StoreStep::operation("test.peer.recovery.result"), move |db| {
            let delivery =
                aura_agent_collaboration::dispatch::load(db, delivery)?.expect("delivery");
            Ok(delivery.target_run)
        })
        .await
        .expect("result binding");
    assert_eq!(result, Some(run), "one durable receiver run, not a second claim");
    let attempts = services
        .agent_required()
        .store
        .read(StoreStep::operation("test.peer.recovery.attempts_cleared"), move |db| {
            aura_agent_collaboration::dispatch::retry_attempts(db, delivery)
        })
        .await
        .expect("attempt count after claim");
    assert_eq!(attempts, 0, "a claimed delivery clears its bounded retry state");
}
