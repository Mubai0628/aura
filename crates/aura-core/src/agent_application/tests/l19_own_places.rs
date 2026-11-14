//! A sub-agent in a worktree works in its copy and in its temporary folder as its own, through the
//! real model loop: its command writes the folder and deletes in the copy, another names the copy
//! in full, its file tools write and read the folder, and it reads a picture. None of it asks. The
//! main agent then deletes a file in that copy without asking, and one outside its workspace once
//! the user approves.
use super::l19_team::{begin, call, members, requests, results};
use super::l19_worktrees::{git, only_copy, prepare};
use super::*;
use provider_fixture::{MAIN_DELETED, OWN_DELETED, OWN_NOTE, OWN_PICTURE, Scenario, TeamPlan};

/// A 1×1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

#[tokio::test]
async fn a_sub_agent_works_in_its_own_places_unasked_and_the_main_agent_in_its_copy() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::OwnPlaces }).await;
    let (runtime, session, model, root, location) = prepare(&host, &provider).await;
    std::fs::write(root.join(MAIN_DELETED), "the main agent deletes this in the copy\n")
        .expect("a file for the main agent");
    std::fs::write(root.join(OWN_PICTURE), PNG).expect("a picture");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "more"]);
    let outside = host.root.path().join("outside-delete.txt");
    std::fs::write(&outside, "outside\n").expect("a file outside the workspace");
    let request = begin(
        &runtime,
        session,
        model,
        &format!(
            "清理 AURA_EXTERNAL_PATH={} AURA_COPIES={}",
            outside.display(),
            location.display()
        ),
    )
    .await;

    // The one question is the main agent's deletion outside its workspace.
    let approval = tokio::time::timeout(std::time::Duration::from_secs(90), async {
        loop {
            let team = members(&runtime, session).await;
            if let Some(asked) = team.first().map(|member| &member["approvals"])
                && asked.as_array().is_some_and(|ids| !ids.is_empty())
            {
                panic!("the sub-agent was asked: {team:?}");
            }
            let view: AgentRunProjection =
                serde_json::from_value(call(&runtime, "agent.status", json!(&request)).await)
                    .expect("status");
            if let Some(id) = view.pending_approval_ids.first().copied() {
                return id;
            }
            assert!(!view.turn.state.terminal(), "ended unasked: {:?}", view.turn.state);
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the deletion outside the workspace asks the user");
    let asked = call(
        &runtime,
        "agent.approval.get",
        json!({"session_id": session, "approval_id": approval}),
    )
    .await;
    assert!(asked.to_string().contains("outside-delete.txt"), "{asked}");

    // Everything before it ran unasked.
    let copy = only_copy(&location);
    assert!(!copy.join(OWN_DELETED).exists(), "the sub-agent's command deleted in its copy");
    assert!(!copy.join(MAIN_DELETED).exists(), "the main agent deleted in the copy");
    assert!(root.join(OWN_DELETED).exists() && root.join(MAIN_DELETED).exists());
    let (theirs, _) = requests(&provider);
    let done = results(theirs.last().expect("the sub-agent's last request"));
    assert_eq!(done.len(), 5, "{done:#?}");
    assert!(done[0].contains("aura-agents") && !done[0].contains("approval"), "{}", done[0]);
    assert!(done[1].contains("here"), "a folder named in full: {}", done[1]);
    assert!(done[2].contains("logical_hash"), "written in its temporary folder: {}", done[2]);
    assert!(done[3].contains(OWN_NOTE), "read in its temporary folder: {}", done[3]);
    assert!(done[4].contains("image/png") && done[4].contains("shown_to_model"), "{}", done[4]);

    call(
        &runtime,
        "agent.approval.resolve",
        json!({
            "session_id": session,
            "approval_id": approval,
            "expected_revision": asked["approval"]["revision"],
            "decision": "allow_once",
        }),
    )
    .await;
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    assert!(!outside.exists(), "the approved deletion outside the workspace ran");
    let (_, ours) = requests(&provider);
    let deleted = results(ours.last().expect("the conversation's last request"));
    assert_eq!(
        deleted.iter().filter(|result| result.contains("recycled")).count(),
        2,
        "{deleted:#?}"
    );
}
