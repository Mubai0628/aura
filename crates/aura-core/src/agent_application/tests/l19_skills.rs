//! Sub-agents and the user's Skills, through the real model loop: a sub-agent is offered the Skill
//! tools and the list of Skills as its main agent is, loads one and reads its files where Aura
//! keeps them without asking; one in a worktree has the Skills of the workspace it was made from.
use super::l19_team::{call, members, offered, requests, results, team, texts, turn};
use super::l19_worktrees::prepare as git_workspace;
use super::*;
use provider_fixture::{SKILL_NAME, Scenario, TeamPlan};
use std::path::{Path, PathBuf};

const INSTRUCTIONS: &str = "Name the entry file first.";

/// The Skill [`SKILL_NAME`] under `root`, with its instructions and one reference file; the
/// reference file's path.
fn skill(root: &Path) -> PathBuf {
    let folder = root.join(SKILL_NAME);
    std::fs::create_dir_all(folder.join("references")).expect("Skill folder");
    std::fs::write(
        folder.join("SKILL.md"),
        format!("---\nname: {SKILL_NAME}\ndescription: Reading code here\n---\n\n{INSTRUCTIONS}\n"),
    )
    .expect("SKILL.md");
    let reference = folder.join("references").join("guide.md");
    std::fs::write(&reference, format!("{}\n", provider_fixture::EXTERNAL_BODY_TOKEN))
        .expect("reference file");
    reference
}

/// The workspace a conversation is in.
async fn workspace_of(runtime: &crate::RuntimeState, session: SessionId) -> Value {
    let projection = call(
        runtime,
        "projection.snapshot",
        json!({"projection_kind": "workspace_session", "projection_scope": {"type": "global"}}),
    )
    .await;
    projection["payload"]["sessions"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s["session_id"] == json!(session))
        .map(|s| s["workspace_id"].clone())
        .expect("the conversation is listed")
}

#[tokio::test]
async fn a_sub_agent_loads_a_skill_and_reads_its_files_where_aura_keeps_them() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::Skill }).await;
    let (runtime, session, model) = team(&host, &provider).await;
    // Aura's own Skill folder, in the fixture's data (`AuraPaths::from_roots(.., root/local)`).
    let reference = skill(&host.root.path().join("local").join("skills").join("packages"));
    turn(&runtime, session, model, &format!("先看 AURA_EXTERNAL_PATH={}", reference.display()))
        .await;

    let (theirs, ours) = requests(&provider);
    // Offered the Skill tools and shown the Skills, as the conversation that started it is.
    let first = &theirs[0];
    let tools = offered(first);
    assert!(tools.contains("skills.load") && tools.contains("skills.search"), "{tools:?}");
    let sent = first.to_string();
    assert!(sent.contains("Skills you can load:"), "{first}");
    assert!(sent.contains(&format!("- {SKILL_NAME}")), "{first}");
    let role = texts(first, "system").join("\n");
    assert!(role.contains("load it with skills.load") && !role.contains("cannot load"), "{role}");
    let guide = texts(&ours[0], "system").join("\n");
    assert!(!guide.contains("cannot load Skills"), "{guide}");
    // The Skill loaded, then its file read where Aura keeps it: no refusal, nobody asked.
    let read = results(theirs.last().expect("the sub-agent's last request"));
    // Its report is its final answer, not a tool result.
    assert_eq!(read.len(), 2, "the Skill and its file: {read:?}");
    assert!(read[0].contains(INSTRUCTIONS), "the Skill: {}", read[0]);
    assert!(read[1].contains(provider_fixture::EXTERNAL_BODY_TOKEN), "its file: {}", read[1]);
    let team = members(&runtime, session).await;
    assert_eq!(team[0]["approvals"], json!([]));
    assert_eq!(team[0]["last_report"], "ready");
}

#[tokio::test]
async fn a_sub_agent_in_a_worktree_has_the_skills_of_the_workspace_it_was_made_from() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::WorktreeSkill }).await;
    let (runtime, session, model, root, _) = git_workspace(&host, &provider).await;
    // A Skill of the main workspace only: never committed, so the copy has no such folder.
    skill(&root.join(".agents").join("skills"));
    let workspace = workspace_of(&runtime, session).await;
    let listed = call(&runtime, "agent.skills.list", json!({"workspace_id": workspace})).await;
    call(
        &runtime,
        "agent.skills.trust_workspace",
        json!({"workspace_id": workspace, "expected_revision": listed["revision"]}),
    )
    .await;
    turn(&runtime, session, model, "改代码").await;

    let (theirs, _) = requests(&provider);
    let first = &theirs[0];
    assert!(first.to_string().contains(&format!("- {SKILL_NAME}")), "{first}");
    let read = results(theirs.last().expect("the sub-agent's last request"));
    assert!(read[0].contains(INSTRUCTIONS), "the Skill: {read:?}");
    let team = members(&runtime, session).await;
    assert!(team[0]["worktree"]["branch"].is_string(), "it worked in a worktree: {team:?}");
    assert_eq!(team[0]["last_report"], "ready", "{team:?}");
}
