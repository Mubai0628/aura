use super::super::l10_19_compaction;
use super::super::*;
use serde_json::Value;

/// Keep the two historical recipes distinct from the active-turn handoff regression.
async fn probe(dispatch_first: bool) {
    let fixture = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Plain).await;
    let (runtime, a, model) = Box::pin(prepare(&fixture, &provider)).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    let workspace = services
        .persistence
        .workspace_session_projection()
        .expect("projection")
        .sessions
        .into_iter()
        .find(|s| s.session_id == a)
        .expect("source session")
        .workspace_id;
    for part in 0..3 {
        let text = (0..2200)
            .map(|i| format!("record-{part}-{i:016x} verified"))
            .collect::<Vec<_>>()
            .join("\n");
        Box::pin(turn(&runtime, a, model, &text)).await;
    }
    let b = Box::pin(create_session(&runtime, workspace, PermissionMode::ReadOnly)).await;
    Box::pin(connect(&runtime, a, b)).await;
    let prefix =
        if dispatch_first { "B4 user authorizes a summary for B." } else { "原始要求。" };
    let text = format!("{prefix} {} 验收条件：必须保留原用户要求", "长文细节 ".repeat(4_000));
    let source = if dispatch_first {
        Box::pin(turn(&runtime, a, model, &text)).await;
        None
    } else {
        let request = Box::pin(begin(&runtime, a, model, &text)).await;
        Box::pin(wait(&runtime, &request, |p| {
            matches!(p.turn.state, TurnTaskState::Completed { .. })
        }))
        .await;
        Some(request.agent_run_id)
    };
    let deliveries: std::collections::BTreeSet<_> = tool_outcomes(&provider)
        .into_iter()
        .map(|v| v["outcome"]["payload"]["value"].clone())
        .filter(|v| v["used"] == 1)
        .filter_map(|v| v["deliveries"][0]["delivery_id"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(deliveries.len(), usize::from(dispatch_first));
    let delivery = deliveries.first();
    l10_19_compaction::resize(&runtime, model, 1_000_000, 4096).await;
    let prior_tokens = l10_19_compaction::used(&runtime, a, 2).await;
    let window = (prior_tokens + 20_000) * 100 / 95;
    l10_19_compaction::resize(&runtime, model, window, 4096).await;
    let before = provider.requests.lock().expect("fixture requests").len();
    let started = crate::r03_tests::request(
        &runtime,
        "agent.compaction.start",
        json!(AgentCompactionStartRequest {
            computer_mode: None,
            session_id: a,
            model_profile_id: model,
            focus: Some("keep the acceptance condition".into()),
        }),
    )
    .await;
    let command: AgentRunProjection =
        serde_json::from_value(started["result"]["value"].clone()).expect("manual compaction");
    let run = command.run.agent_run_id;
    let done =
        Box::pin(wait(&runtime, &AgentRunRequest { session_id: a, agent_run_id: run }, |p| {
            p.turn.state.terminal()
        }))
        .await;
    let requests = provider.requests.lock().expect("fixture requests");
    let incoming = &requests[before..];
    let label = if dispatch_first { "293" } else { "294" };
    let journal = services.broker.lock().expect("same diagnostic store");
    let db = journal.database.connection();
    let compactions = rows(
        db,
        "SELECT payload FROM context_compactions WHERE json_extract(payload,'$.agent_run_id')=?1",
        &run.to_string(),
    );
    let diagnostics = rows(
        db,
        "SELECT json_object('owner',owner,'kind',kind,'payload',json(payload)) FROM events \
         WHERE (kind='agent.application_error' AND owner=?1) OR (kind='model.failure' AND owner IN ( \
           SELECT id FROM model_steps WHERE agent_run_id=?1 UNION \
           SELECT json_extract(a.value,'$.model_step_id') FROM context_compactions c, \
           json_each(c.payload,'$.attempts') a WHERE json_extract(c.payload,'$.agent_run_id')=?1 \
         )) ORDER BY sequence",
        &run.to_string(),
    );
    let compactions = compactions
        .iter()
        .map(|row| {
            json!({"id":row["id"],"state":row["state"],"attempts":row["attempts"],"failure":row["failure"]})
        })
        .collect::<Vec<_>>();
    let summary_requests =
        incoming.iter().filter(|r| l10_19_compaction::asks_compaction(r)).count();
    eprintln!(
        "{}",
        json!({
            "recipe":label,"run":run,"window":window,"result":done.turn.state,
            "source_run_when_no_delivery_exists":source,"delivery_before_compaction":delivery,
            "local_requests":incoming.len(),"summary_requests":summary_requests,
            "compactions":compactions,"same_run_diagnostics":diagnostics,
        })
    );
    match done.turn.state {
        TurnTaskState::Completed { .. } => {
            assert!(summary_requests > 0);
            assert!(compactions.iter().any(|c| c["state"] == "completed"));
        }
        _ => assert!(!diagnostics.is_empty(), "diagnose the actual run before disposing its store"),
    }
}

fn rows(db: &aura_persistence_core::Connection, sql: &str, run: &str) -> Vec<Value> {
    db.prepare(sql)
        .expect("diagnostic query")
        .query_map([run], |r| r.get::<_, String>(0))
        .expect("diagnostic rows")
        .map(|row| serde_json::from_str(&row.expect("diagnostic row")).expect("JSON event"))
        .collect()
}

#[tokio::test]
#[ignore = "explicit historical fault probe, separate from the green handoff suite"]
async fn session_agent_dispatch_compaction_diagnostic_293() {
    Box::pin(probe(true)).await;
}

#[tokio::test]
#[ignore = "explicit historical fault probe, separate from the green handoff suite"]
async fn session_agent_dispatch_compaction_diagnostic_294() {
    Box::pin(probe(false)).await;
}
