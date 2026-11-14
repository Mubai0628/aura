//! The user changes a sub-agent's permission mode while it works, as in a conversation of the
//! user's: from its next call on, the new mode holds, and the team shows it.
use super::l19_team::{begin, call, members, requests, results, team};
use super::*;
use provider_fixture::{Scenario, TeamPlan};

#[tokio::test]
async fn the_user_switches_a_sub_agents_mode_and_its_next_call_follows_it() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::ModeSwitch }).await;
    let (runtime, session, model) = team(&host, &provider).await;
    let request = begin(&runtime, session, model, "写两个文件").await;

    // It shares the workspace, so the settings start it read-only: its first write asks.
    let (child, approval) = tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            let team = members(&runtime, session).await;
            if let Some(member) = team.first()
                && let Some(id) = member["approvals"].as_array().and_then(|ids| ids.first())
            {
                return (member["session_id"].clone(), id.clone());
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("its first write asks");
    assert_eq!(members(&runtime, session).await[0]["mode"], "read_only");

    // The user switches it to 工作区读写 and lets the first write go.
    let state = call(&runtime, "permission.get_session_state", json!({"session_id": child})).await;
    call(
        &runtime,
        "permission.set_session_mode",
        json!({
            "session_id": child,
            "expected_revision": state["revision"],
            "mode": "workspace_read_write",
        }),
    )
    .await;
    let asked =
        call(&runtime, "agent.approval.get", json!({"session_id": child, "approval_id": approval}))
            .await;
    call(
        &runtime,
        "agent.approval.resolve",
        json!({
            "session_id": child,
            "approval_id": approval,
            "expected_revision": asked["approval"]["revision"],
            "decision": "allow_once",
        }),
    )
    .await;

    // Its second write runs without asking, and the turn ends.
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let (theirs, _) = requests(&provider);
    let written = results(theirs.last().expect("the sub-agent's last request"));
    assert_eq!(written.len(), 2, "{written:#?}");
    assert!(written.iter().all(|result| result.contains("logical_hash")), "{written:#?}");
    let team = members(&runtime, session).await;
    assert_eq!(team[0]["mode"], "workspace_read_write");
    assert_eq!(team[0]["approvals"], json!([]));
}
