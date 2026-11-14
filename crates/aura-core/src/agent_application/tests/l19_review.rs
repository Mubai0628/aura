//! The settings let the main agent answer first a sub-agent's question that only deletes in the
//! sub-agent's own copy: agents.wait gives it while the main agent's turn is under way, and its
//! answer covers that one call, or hands the question to the user. The settings' download preset
//! answers a sub-agent's download for that one call. Each answer is kept with who gave it and why.
use super::l19_allowance::{asked, decide, started};
use super::l19_team::{members, requests, results};
use super::l19_worktrees::only_copy;
use super::*;
use provider_fixture::{REVIEW_REASON, REVIEWED_FILE, REVIEWED_SECOND, Scenario, TeamPlan, offers};

fn answered(member: &Value) -> Value {
    member["reviews"][0]["answered"].clone()
}

#[tokio::test]
async fn the_main_agent_answers_a_deletion_in_its_sub_agents_copy_first_and_says_why() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::ReviewAllow }).await;
    let settings = json!({"main_agent_approves": true});
    let (runtime, session, request, location, _) =
        started(&host, &provider, &[REVIEWED_FILE], "outside-review.txt", Some(settings)).await;

    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    assert!(!only_copy(&location).join(REVIEWED_FILE).exists(), "the answered deletion ran");
    let member = members(&runtime, session).await.remove(0);
    assert_eq!(member["approvals"], json!([]), "the user was not asked: {member}");
    let kept = answered(&member);
    assert_eq!(kept["by"], "main_agent", "{member}");
    assert_eq!((&kept["answer"], &kept["reason"]), (&json!("allow"), &json!(REVIEW_REASON)));
    assert!(member["reviews"][0]["presented_at"].is_string(), "{member}");

    let (_, ours) = requests(&provider);
    let last = ours.last().expect("the main agent's last request");
    assert!(offers(last, "agents.approve"), "the settings let it answer");
    let given = &results(last)[1];
    assert!(given.contains("Remove-Item $png") && given.contains("seconds_left"), "{given}");
}

/// allow_workspace is the user's card's allowance: the call runs, and the sub-agent's next deletion
/// in its copy asks neither the main agent nor the user.
#[tokio::test]
async fn the_main_agents_allowance_lets_the_next_deletion_in_the_copy_go_unasked() {
    let host = crate::r03_tests::Fixture::new();
    let plan = TeamPlan::ReviewAllowWorkspace;
    let provider = Provider::scenario(Scenario::Team { plan }).await;
    let settings = json!({"main_agent_approves": true});
    let files = [REVIEWED_FILE, REVIEWED_SECOND];
    let (runtime, session, request, location, _) =
        started(&host, &provider, &files, "outside-allowance.txt", Some(settings)).await;

    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    for name in files {
        assert!(!only_copy(&location).join(name).exists(), "{name} was deleted");
    }
    let member = members(&runtime, session).await.remove(0);
    assert_eq!(member["approvals"], json!([]), "the user was not asked: {member}");
    assert_eq!(member["allowed"]["deletions"], true, "{member}");
    let reviews = member["reviews"].as_array().expect("reviews");
    assert_eq!(reviews.len(), 1, "the main agent was asked once: {member}");
    assert_eq!(answered(&member)["answer"], "allow_workspace", "{member}");

    let (_, ours) = requests(&provider);
    let given = &results(ours.last().expect("the main agent's last request"))[1];
    assert!(given.contains("\"allow_workspace\":true"), "{given}");
}

#[tokio::test]
async fn a_question_the_main_agent_hands_over_waits_for_the_user() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::ReviewAskUser }).await;
    let settings = json!({"main_agent_approves": true});
    let (runtime, session, request, location, _) =
        started(&host, &provider, &[REVIEWED_FILE], "outside-handed.txt", Some(settings)).await;

    // The user's card appears once the main agent hands the question over, with its reason.
    let (child, view) = asked(&runtime, session, &[]).await;
    let member = members(&runtime, session).await.remove(0);
    let kept = answered(&member);
    assert_eq!((&kept["by"], &kept["answer"]), (&json!("main_agent"), &json!("ask_user")));
    assert!(only_copy(&location).join(REVIEWED_FILE).exists(), "nothing ran before the user");
    decide(&runtime, &child, &view, "allow_once").await;

    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    assert!(!only_copy(&location).join(REVIEWED_FILE).exists(), "the user's answer ran it");
}

#[tokio::test]
async fn the_settings_answer_a_sub_agents_download_for_that_call() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::DownloadPreset }).await;
    let settings = json!({"allow_downloads": true});
    let (runtime, session, request, _, _) =
        started(&host, &provider, &[REVIEWED_FILE], "outside-download.txt", Some(settings)).await;

    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let member = members(&runtime, session).await.remove(0);
    assert_eq!(member["approvals"], json!([]), "the user was not asked: {member}");
    assert_eq!(member["allowed"]["downloads"], true, "{member}");
    let kept = answered(&member);
    assert_eq!((&kept["by"], &kept["answer"]), (&json!("settings"), &json!("allow")), "{member}");
    let (theirs, _) = requests(&provider);
    let done = results(theirs.last().expect("the sub-agent's last request"));
    assert!(done[0].contains("exit_code"), "the fetch ran: {done:#?}");
}
