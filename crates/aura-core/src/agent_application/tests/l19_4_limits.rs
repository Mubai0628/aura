//! The sub-agents' limits through the real model loop: a sub-agent its provider refuses for its
//! rate waits and asks again; one past its token budget is stopped by the watchdog and its main
//! agent is told; after Core restarts, one that waited for the user is reported to its main agent
//! instead of waiting on; and the limits are read and saved over their revision.
use super::l19_team::{begin, call, listed_until, members, requests, results, team, turn};
use super::*;
use aura_agent_team::AgentTeamLimits;
use aura_core_broker::agent_store;
use provider_fixture::{RATE_LIMITED, SUMMARY, Scenario, TeamPlan};

fn agent(runtime: &crate::RuntimeState) -> Arc<AgentApplication> {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    services.agent_required()
}

/// Why saving these limits was refused.
async fn refused(runtime: &crate::RuntimeState, params: Value) -> String {
    let response =
        Box::pin(crate::r03_tests::request(runtime, "agent.team.settings.set", params)).await;
    response["error"]["message"].as_str().unwrap_or_default().to_owned()
}

#[tokio::test]
async fn a_sub_agent_refused_for_its_rate_waits_and_asks_again() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::RateLimited }).await;
    let (runtime, session, model) = team(&host, &provider).await;
    turn(&runtime, session, model, "看看入口在哪").await;

    let (theirs, ours) = requests(&provider);
    assert!(theirs.len() > RATE_LIMITED, "it asked again after {} refusals", theirs.len());
    let read = results(ours.last().expect("the conversation's last request"));
    assert!(read.last().is_some_and(|wait| wait.contains(SUMMARY)), "{read:?}");
    let team = members(&runtime, session).await;
    assert_eq!((&team[0]["state"], &team[0]["last_report"]), (&json!("reported"), &json!("ready")));
}

#[tokio::test]
async fn past_its_token_budget_a_sub_agent_is_stopped_and_its_main_agent_is_told() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::Budget }).await;
    let (runtime, session, model) = team(&host, &provider).await;
    // The smallest budget the settings take; two of the fixture sub-agent's answers pass it.
    let chosen = AgentTeamLimits {
        token_budget: Some(*AgentTeamLimits::TOKENS.start()),
        ..AgentTeamLimits::DEFAULT
    };
    agent(&runtime)
        .store
        .write(agent_store::StoreStep::operation("test.limits"), move |journal| {
            journal.database.unit_of_work(|tx| aura_agent_team::limits::set_limits(tx, &chosen, 0))
        })
        .await
        .expect("the limits");
    turn(&runtime, session, model, "看看入口在哪").await;

    let (theirs, ours) = requests(&provider);
    // Stopped once its second answer passed the budget: before its third request, or while that
    // request was held.
    assert!((2..=3).contains(&theirs.len()), "it asked {} times", theirs.len());
    let read = results(ours.last().expect("the conversation's last request"));
    assert!(read.last().is_some_and(|wait| wait.contains("预算用完")), "{read:?}");
    let view = listed_until(&runtime, session, "its turn ends", |view| {
        view["members"][0]["working"] == false
    })
    .await;
    let member = &view["members"][0];
    assert_eq!((&member["state"], &member["last_report"]), (&json!("reported"), &json!("failed")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn after_a_restart_a_sub_agent_that_waited_for_the_user_is_reported_to_its_main_agent() {
    let host = Arc::new(crate::r03_tests::Fixture::new());
    let provider = Arc::new(Provider::scenario(Scenario::Team { plan: TeamPlan::Approval }).await);
    let outside = host.root.path().join("outside-team");
    std::fs::create_dir_all(&outside).expect("a folder outside the workspace");
    let file = outside.join("note.txt");
    std::fs::write(&file, format!("{}\n", provider_fixture::EXTERNAL_BODY_TOKEN))
        .expect("a file outside the workspace");
    let (first_host, first_provider) = (host.clone(), provider.clone());
    let session = crate::agent_application::tests::recovery::auto_continue::crashed(move || {
        Box::pin(async move {
            let (runtime, session, model) = team(&first_host, &first_provider).await;
            let text = format!("先看 AURA_EXTERNAL_PATH={}", file.display());
            begin(&runtime, session, model, &text).await;
            listed_until(&runtime, session, "A1 waits for the user", |view| {
                view["members"][0]["approvals"].as_array().is_some_and(|ids| !ids.is_empty())
            })
            .await;
            session
        })
    })
    .await;

    let runtime = host.start().await;
    let view = call(&runtime, "agent.team.list", json!({"session_id": session})).await;
    let member = &view["members"][0];
    assert_eq!((&member["state"], &member["last_report"]), (&json!("reported"), &json!("blocked")));
    assert!(member["summary"].as_str().is_some_and(|said| said.contains("重启")), "{member}");
    assert_eq!(member["approvals"], json!([]));
    assert_eq!(member["working"], false);
    // Only the main agent's cut-off turn is continued; the sub-agent waits for its main agent.
    let decisions = agent(&runtime).continue_after_restart(CoreRestartCause::Crash).await;
    assert_eq!(decisions.len(), 1, "{decisions:?}");
    let (theirs, _) = requests(&provider);
    assert_eq!(theirs.len(), 1, "the read that waited for the user is not asked again");
}

#[tokio::test]
async fn the_limits_are_saved_over_their_revision_and_refused_out_of_range() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let read = call(&runtime, "agent.team.settings", json!({})).await;
    assert_eq!(read["revision"], 0);
    assert_eq!(read["limits"], read["defaults"]);
    assert_eq!(read["limits"]["max_running"], 3);
    // By default the tokens are not limited and a task has two hours.
    assert_eq!(read["limits"]["token_budget"], Value::Null);
    assert_eq!(read["limits"]["minutes_per_task"], 120);
    assert_eq!(read["minimum"]["token_budget"], json!(AgentTeamLimits::TOKENS.start()));
    assert_eq!(read["maximum"]["max_running"], json!(AgentTeamLimits::RUNNING.end()));
    let chosen = json!({
        "max_running": 5,
        "token_budget": 2_000_000,
        "minutes_per_task": 30,
        "cpu_percent": 50
    });
    let saved = call(
        &runtime,
        "agent.team.settings.set",
        json!({"limits": chosen, "expected_revision": 0}),
    )
    .await;
    assert_eq!((&saved["limits"], &saved["revision"]), (&chosen, &json!(1)));

    let stale =
        refused(&runtime, json!({"limits": read["defaults"], "expected_revision": 0})).await;
    assert!(stale.contains("agents.limits_changed"), "{stale}");
    let mut wide = chosen.clone();
    wide["max_running"] = json!(AgentTeamLimits::RUNNING.end() + 1);
    let out = refused(&runtime, json!({"limits": wide, "expected_revision": 1})).await;
    assert!(out.contains("agents.limits_invalid"), "{out}");
    let again = call(&runtime, "agent.team.settings", json!({})).await;
    assert_eq!((&again["limits"], &again["revision"]), (&chosen, &json!(1)));
    // The budget is taken away again: the tokens are not limited.
    let mut unlimited = chosen.clone();
    unlimited["token_budget"] = Value::Null;
    let saved = call(
        &runtime,
        "agent.team.settings.set",
        json!({"limits": unlimited, "expected_revision": 1}),
    )
    .await;
    assert_eq!((&saved["limits"], &saved["revision"]), (&unlimited, &json!(2)));
}
