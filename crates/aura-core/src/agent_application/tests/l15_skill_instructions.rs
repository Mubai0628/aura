//! Skills in the instructions before the conversation: the pinned Skills whole, then the names of
//! the other usable Skills, both ahead of the root AGENTS.md. A run's first step decides them, so
//! the block stays byte-identical through the turn; a Skill added afterwards is named from the
//! next turn on.
use super::agents_instructions::agents_block;
use super::*;

const PINNED_NOTE: &str = "PINNED-SKILL-NOTE: keep answers short.";

/// Imports a Skill folder into Aura through the real Settings route and returns its listed view.
async fn imported(
    runtime: &crate::RuntimeState,
    parent: &std::path::Path,
    name: &str,
    body: &str,
) -> Value {
    let folder = parent.join(name);
    std::fs::create_dir_all(&folder).expect("Skill folder");
    std::fs::write(
        folder.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Test {name}\n---\n\n{body}\n"),
    )
    .expect("SKILL.md");
    let listed = crate::r03_tests::success(runtime, "agent.skills.list", json!({})).await;
    crate::r03_tests::success(
        runtime,
        "agent.skills.import",
        json!({"path": folder, "expected_revision": listed["revision"]}),
    )
    .await;
    let listed = crate::r03_tests::success(runtime, "agent.skills.list", json!({})).await;
    listed["skills"]
        .as_array()
        .expect("skills")
        .iter()
        .find(|skill| skill["name"] == name)
        .cloned()
        .expect("imported")
}

async fn completed_turn(runtime: &crate::RuntimeState, session: SessionId, model: ModelProfileId) {
    let start = super::start(runtime, session, model, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    wait(runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
}

#[tokio::test]
async fn pinned_skills_and_the_names_of_the_others_come_before_agents_md_through_the_turn() {
    const READ: Scenario = Scenario::Sequence {
        calls: &[(
            "fs.read",
            r#"{"path":"AGENTS.md","range":{"range_kind":"full"},"explicit_encoding":null}"#,
        )],
    };
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(READ).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    std::fs::write(host.root.path().join("waiting-workspace").join("AGENTS.md"), "root rule\n")
        .expect("AGENTS.md");
    let sources = host.root.path().join("skill-sources");
    imported(&runtime, &sources, "zeta-review", "Review.").await;
    imported(&runtime, &sources, "alpha-docs", "Docs.").await;
    let notes = imported(&runtime, &sources, "notes", PINNED_NOTE).await;
    let listed = crate::r03_tests::success(&runtime, "agent.skills.list", json!({})).await;
    crate::r03_tests::success(
        &runtime,
        "agent.skills.set_pinned",
        json!({"id": notes["id"], "pinned": true, "expected_revision": listed["revision"]}),
    )
    .await;

    completed_turn(&runtime, session, model).await;
    let requests = provider.requests.lock().expect("requests").clone();
    assert!(requests.len() >= 2, "a step after the tool call");
    let first = agents_block(&requests[0]);
    let at = |text: &str| first.find(text).unwrap_or_else(|| panic!("{text} missing: {first}"));
    assert!(at(PINNED_NOTE) < at("- alpha-docs"), "{first}");
    assert!(at("- alpha-docs") < at("- zeta-review"), "{first}");
    assert!(at("- zeta-review") < at("root rule"), "{first}");
    assert!(!first.contains("- notes"), "a pinned Skill is not named again: {first}");
    let last = agents_block(requests.last().expect("a last request"));
    assert_eq!(last, first, "the same bytes through the turn");
}

#[tokio::test]
async fn a_skill_added_after_a_turn_is_named_from_the_next_turn() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    let sources = host.root.path().join("skill-sources");
    imported(&runtime, &sources, "alpha-docs", "Docs.").await;
    completed_turn(&runtime, session, model).await;
    imported(&runtime, &sources, "beta-tests", "Tests.").await;
    completed_turn(&runtime, session, model).await;
    let requests = provider.requests.lock().expect("requests").clone();
    let first = agents_block(&requests[0]);
    let last = agents_block(requests.last().expect("a last request"));
    assert!(first.contains("- alpha-docs") && !first.contains("beta-tests"), "{first}");
    assert!(last.contains("- alpha-docs") && last.contains("- beta-tests"), "{last}");
}
