use super::*;
use aura_core_broker::agent_store::StoreStep;
use std::time::Duration;

#[path = "session_agent_dispatch_compaction.rs"]
mod compaction;
#[path = "session_agent_dispatch_completed.rs"]
mod completed;
#[path = "session_agent_dispatch_context.rs"]
mod context;
#[path = "session_agent_dispatch_question.rs"]
mod question;
#[path = "session_agent_dispatch_recovery.rs"]
mod recovery;
#[path = "session_agent_dispatch_source_discovery.rs"]
mod source_discovery;

fn expected(view: &Value) -> Value {
    json!({"graph_id":view["graph"]["graph_id"],"revision":view["graph"]["revision"]})
}

// The restart future owns an entire Core runtime. Keep it out of the long test's debug frame.
fn restart(
    host: &crate::r03_tests::Fixture,
    runtime: crate::RuntimeState,
) -> std::pin::Pin<Box<dyn Future<Output = crate::RuntimeState> + '_>> {
    Box::pin(host.restart(runtime))
}

async fn connect(runtime: &crate::RuntimeState, a: SessionId, b: SessionId) {
    let one = call(runtime, "agent.team.graph.get", json!({"session_id":a})).await;
    let two = call(runtime, "agent.team.graph.get", json!({"session_id":b})).await;
    let merged = call(runtime, "agent.team.graph.mutate", json!({"session_id":a,"expected_graph":expected(&one),"action":{"action":"merge","other_session_id":b,"expected_other":expected(&two),"confirmed":true}})).await;
    let linked = call(runtime, "agent.team.graph.mutate", json!({"session_id":a,"expected_graph":expected(&merged),"action":{"action":"connect","source_session_id":a,"target_session_id":b}})).await;
    assert_eq!(linked["graph"]["edges"].as_array().expect("edges").len(), 1);
}

async fn store_legacy_switch_with_zero_limit(runtime: &crate::RuntimeState) {
    let current = call(runtime, "agent.team.settings", json!({})).await;
    let mut limits = current["limits"].clone();
    limits["max_running"] = json!(0);
    call(
        runtime,
        "agent.team.settings.set",
        json!({"enabled":false,"expected_revision":current["revision"],"limits":limits}),
    )
    .await;
}

fn tool_outcomes(provider: &Provider) -> Vec<Value> {
    provider
        .requests
        .lock()
        .expect("requests")
        .iter()
        .flat_map(results)
        .map(|s| serde_json::from_str(&s).expect("tool result envelope"))
        .collect()
}

async fn request_with(provider: &Provider, marker: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            if let Some(request) = provider
                .requests
                .lock()
                .expect("requests")
                .iter()
                .find(|request| texts(request, "user").iter().any(|text| text.contains(marker)))
                .cloned()
            {
                return request;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no loopback request carrying {marker}"))
}

#[test]
fn session_agent_dispatch_keeps_pending_user_input_in_its_own_turn() {
    // This fixture runs the entire Core and checks several live model turns on a debug stack.
    std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("test runtime")
                .block_on(pending_user_input_remains_separate())
        })
        .expect("test thread")
        .join()
        .expect("pending user input scenario");
}

async fn pending_user_input_remains_separate() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::PeerDispatch { disconnect: false }).await;
    let (runtime, a, model) = prepare(&host, &provider).await;
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
    let b = create_session(&runtime, workspace, PermissionMode::WorkspaceReadWrite).await;
    let binding = services.persistence.session_model_binding_state(b).expect("binding");
    call(
        &runtime,
        "models.session.set_binding",
        json!({"session_id":b,"model_profile_id":model,"expected_revision":binding.revision}),
    )
    .await;
    connect(&runtime, a, b).await;
    store_legacy_switch_with_zero_limit(&runtime).await;
    assert!(provider.requests.lock().expect("requests").is_empty(), "a graph edit ran a model");
    let held = begin(&runtime, b, model, "B4 neutral hold").await;
    tokio::time::timeout(Duration::from_secs(30), provider.parked.notified())
        .await
        .expect("B's real run is using its model");
    let app = services.agent_required();
    let pending = app
        .submit(
            CommandInvocationId::new(),
            AgentSubmitRequest::new(b, "B4 unrelated user input", AgentInputLane::NextTurn),
        )
        .await
        .expect("submit");
    assert_eq!(pending.messages.len(), 1);
    assert_eq!(pending.messages[0].text, "B4 unrelated user input");
    eprintln!(
        "B has a live ordinary run and exactly one pending ordinary user event: {}",
        pending.messages[0].message_id
    );
    let source = begin(&runtime, a, model, "B4 user authorizes a summary for B").await;
    let source =
        wait(&runtime, &source, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let output = tool_outcomes(&provider);
    assert!(
        output
            .iter()
            .any(|v| v["outcome"]["payload"]["value"]["peers"][0]["session_id"] == json!(b)),
        "A did not discover B through the model tool while B's own input was pending: {output:?}"
    );
    let receipt = output
        .iter()
        .map(|v| &v["outcome"]["payload"]["value"])
        .find(|v| v["used"] == 1)
        .unwrap_or_else(|| panic!("dispatch not accepted: {output:?}"));
    assert_eq!(receipt["limit"], 32);
    assert_eq!(receipt["deliveries"][0]["state"], "queued");
    assert_eq!(receipt["deliveries"][0]["reason"], "目标会话正在处理当前轮次。");
    assert!(receipt["deliveries"][0]["result"].is_null());
    let delivery: EventId = serde_json::from_value(receipt["deliveries"][0]["delivery_id"].clone())
        .expect("receipt identity");
    let queued = app
        .store
        .read(StoreStep::operation("test.peer.pending"), move |db| {
            aura_agent_collaboration::dispatch::load(db, delivery)
        })
        .await
        .expect("durable delivery")
        .expect("row");
    assert!(queued.target_run.is_none(), "B claimed an accepted peer task while busy");
    assert_eq!(queued.sender.session_id, a);
    assert_eq!(queued.sender.turn_task_id, source.turn.turn_task_id);
    assert_eq!(queued.origin.root_event_id, source.turn.root_user_event_id);
    provider.release_window.notify_one();
    wait(&runtime, &held, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let ordinary = request_with(&provider, "B4 unrelated user input").await;
    assert!(!ordinary.to_string().contains("B4 peer: summarize only"));
    assert!(offered(&ordinary).contains("peers.ask"), "incoming link should allow a question");
    assert!(!offered(&ordinary).contains("peers.dispatch"), "incoming link cannot dispatch");
    let remote = request_with(&provider, "B4 peer: summarize only").await;
    assert!(
        !remote.to_string().contains("B4 unrelated user input"),
        "unrelated ordinary user input leaked into peer request"
    );
    assert!(remote.to_string().contains("B4 user authorizes a summary for B"));
    assert!(remote.to_string().contains("peer_collaboration"));
    assert!(remote.to_string().contains(&source.turn.root_user_event_id.to_string()));
    assert!(remote.to_string().contains("peer_sender"));
    let run = app
        .store
        .read(StoreStep::operation("test.peer.run"), move |db| {
            let d = aura_agent_collaboration::dispatch::load(db, delivery)?.expect("row");
            assert!(
                aura_persistence_v2::agent::session::user_message(db, b, delivery).is_err(),
                "peer envelope became a user event"
            );
            Ok(d.target_run.expect("claim"))
        })
        .await
        .expect("read claim");
    wait(&runtime, &AgentRunRequest { session_id: b, agent_run_id: run }, |p| {
        matches!(p.turn.state, TurnTaskState::Completed { .. })
    })
    .await;
    turn(&runtime, a, model, &format!("B4 reconcile {delivery}")).await;
    let resolved = tool_outcomes(&provider)
        .into_iter()
        .map(|v| v["outcome"]["payload"]["value"]["deliveries"][0].clone())
        .find(|v| v["state"] == "completed")
        .expect("sender queries the durable result with temporary sub-agents disabled");
    assert_eq!(resolved["target_run_id"], json!(run));
    assert_eq!(resolved["result"], "B4 summary result");
}

#[tokio::test]
async fn session_agent_dispatch_unknown_reconciles_the_original_call_after_restart_without_replay()
{
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::PeerDispatch { disconnect: true }).await;
    let (runtime, a, model) = prepare(&host, &provider).await;
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
    let b = create_session(&runtime, workspace, PermissionMode::WorkspaceReadWrite).await;
    let binding = services.persistence.session_model_binding_state(b).expect("binding");
    call(
        &runtime,
        "models.session.set_binding",
        json!({"session_id":b,"model_profile_id":model,"expected_revision":binding.revision}),
    )
    .await;
    connect(&runtime, a, b).await;
    turn(&runtime, a, model, "B4 user authorizes a summary for B").await;
    tokio::time::timeout(Duration::from_secs(20), provider.parked.notified())
        .await
        .expect("B has sent its second real request after the filesystem tool");
    let effect = host.root.path().join("waiting-workspace/peer-effect.txt");
    assert_eq!(
        std::fs::read_to_string(&effect).expect("real persisted effect"),
        "one real receiver write"
    );
    let receipt = tool_outcomes(&provider)
        .into_iter()
        .map(|v| v["outcome"]["payload"]["value"].clone())
        .find(|v| v["used"] == 1)
        .expect("dispatch receipt");
    let delivery: EventId =
        serde_json::from_value(receipt["deliveries"][0]["delivery_id"].clone()).expect("id");
    let info = services
        .agent_required()
        .store
        .read(StoreStep::operation("test.peer.before_restart"), move |db| {
            aura_agent_collaboration::dispatch::load(db, delivery)
        })
        .await
        .expect("delivery")
        .expect("row");
    let run = info.target_run.expect("receiver claimed");
    let before = provider
        .requests
        .lock()
        .expect("requests")
        .iter()
        .filter(|v| v.to_string().contains("peer_task"))
        .count();
    let runtime = restart(&host, runtime).await;
    turn(&runtime, a, model, &format!("B4 reconcile call:{}", info.sender.tool_call_id)).await;
    let resolved = tool_outcomes(&provider)
        .into_iter()
        .map(|v| v["outcome"]["payload"]["value"]["deliveries"][0].clone())
        .find(|v| v["state"] == "unknown")
        .expect("actual unconfirmed receiver result after restart");
    assert_eq!(resolved["delivery_id"], json!(delivery));
    assert_eq!(resolved["target_run_id"], json!(run));
    assert!(resolved["result"].is_null());
    assert_eq!(
        provider
            .requests
            .lock()
            .expect("requests")
            .iter()
            .filter(|v| v.to_string().contains("peer_task"))
            .count(),
        before
    );
    assert_eq!(
        std::fs::read_to_string(&effect).expect("effect retained"),
        "one real receiver write"
    );
}
